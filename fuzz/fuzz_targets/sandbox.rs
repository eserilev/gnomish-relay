//! S31 and S32 on the compiled code, and the two tools that make the policy real.
//! The input is `chat NUL temp NUL deny NUL deny ...`.
//!
//! - The policy writes only the chat folder and the temp folder, each one clean and not
//!   hidden, and it hides each `deny` folder.
//! - Each field reads back from its SBPL literal, as the model of `Spec/Sbpl.lean` reads.
//! - The Seatbelt profile holds exactly the expected literals, in order: no path ends its
//!   literal early or adds a rule. Between the writable paths and the hidden paths come
//!   only the pinned paths and the folders above a guarded path.
//! - The `bwrap` arguments bind each writable path and each pinned path, and end with the
//!   command.
#![no_main]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use bridge::action_input::{DESKTOP_PATHS, DESKTOP_WRITES};
use bridge::command_sandbox::{Walls, bwrap_args, seatbelt_profile};
use bridge::story_sandbox::Sandbox;
use libfuzzer_sys::fuzz_target;
use protocol::sandbox::{is_hidden, sandbox_policy};
use protocol::sbpl::sbpl_string;

fn parts(path: &[u8]) -> Vec<&[u8]> {
    path.split(|&b| b == b'/')
        .filter(|p| !p.is_empty())
        .collect()
}

fn is_clean(path: &[u8]) -> bool {
    let parts = parts(path);
    let joined: Vec<u8> = parts.iter().flat_map(|p| [&b"/"[..], p].concat()).collect();
    !parts.is_empty() && joined == path && parts.iter().all(|&p| p != b"." && p != b"..")
}

fn patterns(list: &[&str]) -> Vec<Vec<u8>> {
    list.iter().map(|p| p.as_bytes().to_vec()).collect()
}

/// The string reader of `Spec/Sbpl.lean`, after the opening quote.
fn read_body(bytes: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let c = *bytes.get(i)?;
        i += 1;
        match c {
            0 => return None,
            b'"' => return Some((out, &bytes[i..])),
            b'\\' => {
                let e = *bytes.get(i)?;
                i += 1;
                match e {
                    0 | b'0'..=b'7' | b'x' | b'X' => return None,
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
    }
}

fn read_literal(bytes: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let (first, rest) = bytes.split_first()?;
    if *first != b'"' {
        return None;
    }
    read_body(rest)
}

/// Every literal of a profile, in order. `None` if one does not read.
fn literals(mut profile: &[u8]) -> Option<Vec<Vec<u8>>> {
    let mut found = Vec::new();
    while let Some(at) = profile.iter().position(|&b| b == b'"') {
        let (text, rest) = read_literal(&profile[at..])?;
        found.push(text);
        profile = rest;
    }
    Some(found)
}

#[cfg(unix)]
fn path_of(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(unix)]
fn bytes_of(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

fn check_policy(chat: &[u8], temp: &[u8], deny: &[Vec<u8>]) {
    let policy = sandbox_policy(
        chat,
        temp,
        deny,
        &patterns(DESKTOP_PATHS),
        &patterns(DESKTOP_WRITES),
    );
    for w in &policy.writable {
        assert!(w == chat || w == temp);
        assert!(is_clean(w));
        assert!(!is_hidden(&policy, w));
    }
    for d in deny {
        assert!(is_hidden(&policy, d));
    }
}

fn check_escape(bytes: &[u8]) {
    let Some(literal) = sbpl_string(bytes) else {
        assert!(bytes.contains(&0));
        return;
    };
    let mut with_rest = literal.clone();
    with_rest.extend_from_slice(b" (allow default)");
    let (text, rest) = read_literal(&with_rest).expect("the literal reads");
    assert_eq!(text, bytes);
    assert_eq!(rest, b" (allow default)");
}

fn check_tools(walls: &Walls, command: &str) {
    let shell = Path::new("/bin/bash");
    let args: Vec<OsString> = bwrap_args(walls, Path::new("/"), shell, command);
    let tail: [OsString; 4] = ["--".into(), shell.into(), "-c".into(), command.into()];
    assert_eq!(args[args.len() - 4..], tail);
    for w in &walls.writable {
        let bound = args
            .windows(3)
            .any(|a| a[0] == "--bind" && a[1] == w.as_os_str() && a[2] == w.as_os_str());
        assert!(bound);
    }
    for p in &walls.pinned {
        let bound = args.windows(3).any(|a| {
            (a[0] == "--bind" || a[0] == "--ro-bind")
                && a[1] == p.as_os_str()
                && a[2] == p.as_os_str()
        });
        assert!(bound);
    }
    let Ok(profile) = seatbelt_profile(walls) else {
        return;
    };
    let found = literals(&profile).expect("every literal of the profile reads");
    let fixed = 7;
    let writable: Vec<Vec<u8>> = walls.writable.iter().map(|p| bytes_of(p)).collect();
    let hidden: Vec<Vec<u8>> = walls.hidden.iter().map(|p| bytes_of(p)).collect();
    assert_eq!(found[fixed..fixed + writable.len()], writable[..]);
    let from = fixed + writable.len();
    let at = (from..=found.len() - hidden.len())
        .find(|&i| found[i..i + hidden.len()] == hidden[..])
        .expect("the hidden paths follow");
    for literal in &found[from..at] {
        assert!(is_fixed(walls, &path_of(literal)));
    }
}

/// A pinned path, or a folder strictly between a writable path and a guarded path in it.
fn is_fixed(walls: &Walls, path: &Path) -> bool {
    if walls.pinned.iter().any(|p| p == path) {
        return true;
    }
    let guarded = walls.hidden.iter().chain(&walls.pinned);
    guarded.into_iter().any(|g| {
        walls.writable.iter().any(|w| {
            g.starts_with(w) && path.starts_with(w) && path != w && g.starts_with(path) && g != path
        })
    })
}

fuzz_target!(|data: &[u8]| {
    let mut fields = data.split(|&b| b == 0);
    let chat = fields.next().unwrap_or_default();
    let temp = fields.next().unwrap_or_default();
    let deny: Vec<Vec<u8>> = fields.map(<[u8]>::to_vec).collect();
    check_policy(chat, temp, &deny);
    check_escape(data);
    let walls = Walls {
        tool: Sandbox::Seatbelt,
        writable: vec![path_of(chat), path_of(temp)],
        temp: path_of(temp),
        hidden: deny.iter().map(|d| path_of(d)).collect(),
        pinned: deny.iter().map(|d| path_of(d)).collect(),
        empty: PathBuf::from("/data/empty"),
        proxy: None,
        local_ports: Vec::new(),
        overlays: Vec::new(),
    };
    check_tools(&walls, &String::from_utf8_lossy(data));
});
