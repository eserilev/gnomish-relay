//! The Lua strip encoder against the proved Rust decoder (SPEC.md 14.3, differential tests).

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Bits, bytes, load, lua};
use hmac::{Hmac, Mac};
use mlua::{Function, Lua, Table};
use protocol::cell::encode_cells;
use protocol::frame::{decode_frame, signed_len};
use protocol::record::parse_records;
use sha2::{Digest, Sha256};

fn codec(bits: Bits) -> (Lua, Table) {
    let lua = lua(bits);
    let ns = load(&lua, &["Sha256.lua", "Codec.lua"]);
    (lua, ns)
}

fn call_bytes(f: &Function, args: impl mlua::IntoLuaMulti) -> Vec<u8> {
    f.call::<mlua::String>(args).unwrap().as_bytes().to_vec()
}

fn rust_hmac(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

#[test]
fn sha256_matches_rust_at_every_padding_edge() {
    for bits in [Bits::Unsigned, Bits::Signed] {
        let (lua, ns) = codec(bits);
        let sha: Function = ns.get("Sha256").unwrap();
        for len in [0, 1, 55, 56, 63, 64, 65, 119, 120, 300] {
            let msg = bytes(len as u64, len);
            let got = call_bytes(&sha, lua.create_string(&msg).unwrap());
            assert_eq!(got, Sha256::digest(&msg).to_vec(), "length {len}");
        }
    }
}

#[test]
fn hmac_matches_rust_for_a_full_strip_and_long_keys() {
    for bits in [Bits::Unsigned, Bits::Signed] {
        let (lua, ns) = codec(bits);
        let hmac: Function = ns.get("HmacSha256").unwrap();
        for (key_len, msg_len) in [(32, 0), (32, 3221), (64, 100), (100, 100)] {
            let key = bytes(7, key_len);
            let msg = bytes(8, msg_len);
            let args = (
                lua.create_string(&key).unwrap(),
                lua.create_string(&msg).unwrap(),
            );
            assert_eq!(
                call_bytes(&hmac, args),
                rust_hmac(&key, &msg),
                "key {key_len}, msg {msg_len}"
            );
        }
    }
}

#[test]
fn a_lua_frame_decodes_in_rust_with_a_valid_tag() {
    let (lua, ns) = codec(Bits::Unsigned);
    let frame_fn: Function = ns.get::<Table>("Codec").unwrap().get("Frame").unwrap();
    let key = bytes(1, 32);
    for (seed, len) in [(1, 0), (2, 1), (3, 777), (4, 3200)] {
        let payload = bytes(seed, len);
        let args = (
            1_790_211_079u32,
            70_000u32,
            lua.create_string(&payload).unwrap(),
            lua.create_string(&key).unwrap(),
        );
        let wire = call_bytes(&frame_fn, args);
        let frame = decode_frame(&wire)
            .ok()
            .unwrap_or_else(|| panic!("length {len} does not decode"));
        assert_eq!(frame.time, 1_790_211_079);
        assert_eq!(u32::from(frame.frame_id), 70_000 - 65_536);
        assert_eq!(frame.payload, payload);
        assert_eq!(
            frame.tag[..],
            rust_hmac(&key, &wire[..signed_len(&frame)])[..8]
        );
    }
}

#[test]
fn a_payload_over_3200_bytes_makes_no_frame() {
    let (lua, ns) = codec(Bits::Unsigned);
    let frame_fn: Function = ns.get::<Table>("Codec").unwrap().get("Frame").unwrap();
    let payload = lua.create_string(vec![b'x'; 3201]).unwrap();
    let got: mlua::Value = frame_fn.call((1, 1, payload, "key")).unwrap();
    assert!(got.is_nil());
}

#[test]
fn lua_records_parse_in_rust_and_separators_inside_fields_become_spaces() {
    let (lua, ns) = codec(Bits::Unsigned);
    let payload: mlua::String = lua
        .load(
            r#"
            local ns = ...
            return ns.Codec.Payload({
                { token = "tok1", chat = "c1", id = 12, cwd = "/code", flags = "n;agent=claude",
                  name = "a\30b\31c", text = "keeps\31us, drops\30rs" },
                { token = "tok1", chat = "c2", id = 0, text = "" },
            })
            "#,
        )
        .call(ns)
        .unwrap();
    let records = parse_records(&payload.as_bytes())
        .ok()
        .expect("the payload parses");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].token, b"tok1");
    assert_eq!(records[0].chat, b"c1");
    assert_eq!(records[0].id, 12);
    assert_eq!(records[0].cwd, b"/code");
    assert_eq!(records[0].flags, b"n;agent=claude");
    assert_eq!(records[0].name, b"a b c");
    assert_eq!(records[0].text, b"keeps\x1fus, drops rs");
    assert_eq!(records[1].id, 0);
    assert_eq!(records[1].text, b"");
}

#[test]
fn lua_cells_match_the_rust_cells() {
    let (lua, ns) = codec(Bits::Unsigned);
    let cells_fn: Function = ns.get::<Table>("Codec").unwrap().get("Cells").unwrap();
    for len in [0, 1, 2, 3, 4, 3221] {
        let data = bytes(len as u64 + 50, len);
        let cells: Vec<u8> = cells_fn.call(lua.create_string(&data).unwrap()).unwrap();
        assert_eq!(cells, encode_cells(&data), "length {len}");
    }
}

#[test]
fn the_largest_frame_fits_in_the_strip_after_two_calibration_rows() {
    let (lua, ns) = codec(Bits::Unsigned);
    let codec: Table = ns.get("Codec").unwrap();
    let frame: mlua::String = codec
        .get::<Function>("Frame")
        .unwrap()
        .call((1, 1, lua.create_string(vec![b'x'; 3200]).unwrap(), "key"))
        .unwrap();
    let rows: Vec<Vec<u8>> = codec
        .get::<Function>("StripRows")
        .unwrap()
        .call(frame)
        .unwrap();
    assert!(rows.len() <= codec.get::<usize>("MAX_ROWS").unwrap());
    assert_eq!(rows[0][..9], [0, 1, 2, 3, 4, 5, 6, 7, 0]);
    assert_eq!(rows[1][..9], [7, 6, 5, 4, 3, 2, 1, 0, 7]);
}
