//! Checks the Lean model of the Lua 5.1 lexer (`proofs/Protocol/Spec/Lua.lean`)
//! against the real Lua 5.1, on any literal, not only the ones `lua_string` writes.
//! `read_body` is a line-by-line copy of `luaReadBody`. Keep the two in step.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mlua::Lua;

fn is_newline(b: u8) -> bool {
    b == b'\n' || b == b'\r'
}

/// `readDigits`: at most three digits.
fn read_digits(s: &[u8]) -> (u32, &[u8]) {
    let mut value = 0;
    let mut count = 0;
    let mut rest = s;
    while count < 3 {
        let Some((&b, tail)) = rest.split_first() else {
            break;
        };
        if !b.is_ascii_digit() {
            break;
        }
        value = 10 * value + u32::from(b - b'0');
        count += 1;
        rest = tail;
    }
    (value, rest)
}

/// `luaReadBody`: the string and the bytes after the closing quote, or `None` where
/// Lua raises a lexer error.
fn read_body(delim: u8, s: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let mut out = Vec::new();
    let mut s = s;
    loop {
        let (&c, rest) = s.split_first()?;
        if c == delim {
            return Some((out, rest));
        }
        if is_newline(c) {
            return None;
        }
        if c != b'\\' {
            out.push(c);
            s = rest;
            continue;
        }
        let (&e, rest) = rest.split_first()?;
        let simple = match e {
            b'a' => Some(7),
            b'b' => Some(8),
            b'f' => Some(12),
            b'n' => Some(10),
            b'r' => Some(13),
            b't' => Some(9),
            b'v' => Some(11),
            _ => None,
        };
        if let Some(b) = simple {
            out.push(b);
            s = rest;
        } else if is_newline(e) {
            out.push(b'\n');
            s = match rest.split_first() {
                Some((&f, tail)) if is_newline(f) && f != e => tail,
                _ => rest,
            };
        } else if e.is_ascii_digit() {
            let (value, after) = read_digits(&s[1..]);
            out.push(u8::try_from(value).ok()?);
            s = after;
        } else {
            out.push(e);
            s = rest;
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let lua = Lua::new();
    let mut src = b"return \"".to_vec();
    src.extend_from_slice(data);
    match read_body(b'"', data) {
        Some((string, rest)) => {
            let end = src.len() - rest.len();
            let back: mlua::String = lua
                .load(&src[..end])
                .eval()
                .expect("Lua reads what the model reads");
            assert_eq!(&*back.as_bytes(), &string[..]);
        }
        None => {
            let result: mlua::Result<mlua::Value> = lua.load(&src[..]).eval();
            assert!(result.is_err(), "the model says error, but Lua reads it");
        }
    }
});
