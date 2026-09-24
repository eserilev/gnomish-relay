//! Lua string literals for the slot files. WoW runs a slot file as Lua code, so a
//! bad escape lets an agent reply run code in the game. See `SPEC.md` 14.1, S8.

/// A double-quoted Lua 5.1 literal that reads back as exactly `bytes`.
#[must_use]
pub fn lua_string(bytes: &[u8]) -> Vec<u8> {
    todo!()
}
