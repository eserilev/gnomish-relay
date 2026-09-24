//! Runs the addon in a real Lua 5.1, with WoW's `bit` library as a Lua shim.

#![allow(dead_code)]
// each test file uses a different part
// Clippy sees a shared test module as normal code, so its test exceptions miss it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use mlua::{Lua, Table};

#[derive(Clone, Copy)]
pub enum Bits {
    /// What WoW's `bit` returns.
    Unsigned,
    /// What the `bit` of `LuaJIT` returns.
    Signed,
}

pub fn repo_file(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

pub fn lua(bits: Bits) -> Lua {
    let lua = Lua::new();
    let mode = match bits {
        Bits::Unsigned => "unsigned",
        Bits::Signed => "signed",
    };
    let bit: Table = lua
        .load(repo_file("addon/tests/bit.lua"))
        .set_name("bit.lua")
        .call(mode)
        .unwrap();
    lua.globals().set("bit", bit).unwrap();
    lua
}

/// Loads addon files the way WoW does: each file gets the addon name and the shared table.
pub fn load(lua: &Lua, files: &[&str]) -> Table {
    let ns = lua.create_table().unwrap();
    load_into(lua, &ns, files);
    ns
}

pub fn load_into(lua: &Lua, ns: &Table, files: &[&str]) {
    for file in files {
        let src = repo_file(&format!("addon/GnomishRelay/{file}"));
        lua.load(src)
            .set_name(*file)
            .call::<()>(("GnomishRelay", ns.clone()))
            .unwrap();
    }
}

/// Deterministic test bytes.
pub fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x.to_be_bytes()[3]
        })
        .collect()
}
