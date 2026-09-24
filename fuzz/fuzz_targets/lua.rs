//! S8 against the real Lua 5.1 parser: every literal loads back as the same bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mlua::Lua;
use protocol::lua::lua_string;

// A fresh state per input: one shared state got 18 times slower as it aged.
fuzz_target!(|data: &[u8]| {
    let lua = Lua::new();
    let version: String = lua.load("return _VERSION").eval().expect("Lua starts");
    assert_eq!(version, "Lua 5.1");

    let mut code = b"return ".to_vec();
    code.extend(lua_string(data));
    let back: mlua::String = lua.load(&code[..]).eval().expect("the literal loads");
    assert_eq!(&*back.as_bytes(), data);
});
