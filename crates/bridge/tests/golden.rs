//! The golden vectors in tests/vectors and the fixtures in tests/fixtures (SPEC.md 14.3).
//! The self-test addon made them in the real game. These tests run on every OS.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;

use bridge::fixture::{self, PLACEHOLDER};
use bridge::ids::hex;
use bridge::vectors::{self, MANIFEST, Manifest, Shot, TEST_KEY, Vector};
use common::{Bits, bytes, fixture, load, lua, repo_path, screenshot_png, strip_rows};
use mlua::{Function, Table};

#[test]
fn every_committed_golden_vector_decodes_with_the_test_key() {
    let fixtures = repo_path("tests/fixtures");
    let real = fixture::real_fixtures(&fixtures).unwrap();
    if real.is_empty() {
        eprintln!(
            "SKIP: tests/vectors has no golden vectors yet. Run the self-test in the game (SPEC.md 14.3)."
        );
        return;
    }
    for (_, path) in real {
        let fixture = fixture::read(&path).unwrap();
        let dir = repo_path("tests/vectors").join(&fixture.build);

        let checked = vectors::check_all(&dir)
            .unwrap_or_else(|e| panic!("the vectors of {}: {e:#}", fixture.build));

        let golden = fixture.measured["shots"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|shot| shot["kind"] == "golden")
            .count();
        assert!(
            checked >= golden,
            "{} has {checked} vectors for {golden} golden shots",
            fixture.build
        );
    }
}

#[test]
fn the_placeholder_fixture_goes_once_a_real_fixture_exists() {
    let fixtures = repo_path("tests/fixtures");
    if fixture::real_fixtures(&fixtures).unwrap().is_empty() {
        return;
    }
    assert!(
        !fixtures.join(PLACEHOLDER).exists(),
        "a real fixture exists, so delete tests/fixtures/{PLACEHOLDER}"
    );
}

#[test]
fn the_fake_game_uses_the_placeholder_only_while_no_real_fixture_exists() {
    let fixtures = repo_path("tests/fixtures");
    let none_real = fixture::real_fixtures(&fixtures).unwrap().is_empty();

    let used = fixture();

    assert_eq!(used.placeholder, none_real);
}

/// A vector as the game would make it: the Lua codec signs the frame, and a scene at a
/// fractional cell size stands in for the screenshot.
fn lua_vector(dir: &std::path::Path, frame_id: u16, payload: &[u8]) -> Vector {
    let lua = lua(Bits::Unsigned);
    let ns = load(&lua, &["Sha256.lua", "Codec.lua"]);
    let frame_fn: Function = ns.get::<Table>("Codec").unwrap().get("Frame").unwrap();
    let args = (
        1_790_211_079u32,
        frame_id,
        lua.create_string(payload).unwrap(),
        lua.create_string(TEST_KEY).unwrap(),
    );
    let wire: mlua::String = frame_fn.call(args).unwrap();
    let file = format!("{frame_id:03}-test.png");
    fs::write(
        dir.join(&file),
        screenshot_png(&strip_rows(&wire.as_bytes())),
    )
    .unwrap();
    Vector {
        file,
        shot: Shot {
            kind: "golden".into(),
            name: "test".into(),
            frame_id,
            time: 1_790_211_079,
            payload: hex(payload),
            unix: Some(1_790_300_000),
            ui_parent_scale: Some(1.0),
            strip_effective_scale: Some(1.0),
        },
        width: 1280,
        height: 720,
    }
}

fn write_manifest(dir: &std::path::Path, vectors: Vec<Vector>) {
    let manifest = Manifest {
        build: "1.60.1.70009".into(),
        key: hex(TEST_KEY),
        vectors,
    };
    let text = serde_json::to_string_pretty(&manifest).unwrap();
    fs::write(dir.join(MANIFEST), text).unwrap();
}

#[test]
fn a_vector_that_the_lua_codec_signs_passes_the_golden_check() {
    let dir = tempfile::tempdir().unwrap();
    let vectors = [(1, 0), (2, 62), (3, 3200)]
        .map(|(id, len)| lua_vector(dir.path(), id, &bytes(u64::from(id), len)));
    write_manifest(dir.path(), vectors.to_vec());

    assert_eq!(vectors::check_all(dir.path()).unwrap(), 3);
}

#[test]
fn a_vector_whose_manifest_names_another_payload_fails_the_golden_check() {
    let dir = tempfile::tempdir().unwrap();
    let mut vector = lua_vector(dir.path(), 1, b"the real payload");
    vector.shot.payload = hex(b"another payload");
    write_manifest(dir.path(), vec![vector]);

    let error = vectors::check_all(dir.path()).unwrap_err();

    assert!(format!("{error:#}").contains("another frame"), "{error:#}");
}

#[test]
fn a_manifest_with_another_key_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let vector = lua_vector(dir.path(), 1, b"payload");
    let manifest = Manifest {
        build: "1.60.1.70009".into(),
        key: hex(b"0123456789abcdef0123456789abcdef"),
        vectors: vec![vector],
    };
    fs::write(
        dir.path().join(MANIFEST),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();

    assert!(vectors::check_all(dir.path()).is_err());
}
