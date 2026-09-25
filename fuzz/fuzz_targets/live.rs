//! S20 and S21 against the real Lua 5.1: random progress and requests go through
//! the prepare steps and the writer, and the file loads back as the same fields.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mlua::{Lua, Table};
use protocol::live::{OptionKind, PermOption, Progress, Request, live_body, prepare_progress, prepare_requests};

const KINDS: [(OptionKind, &str); 4] = [
    (OptionKind::AllowOnce, "allow_once"),
    (OptionKind::AllowAlways, "allow_always"),
    (OptionKind::RejectOnce, "reject_once"),
    (OptionKind::RejectAlways, "reject_always"),
];

fn bytes(value: mlua::String) -> Vec<u8> {
    value.as_bytes().to_vec()
}

fuzz_target!(|data: &[u8]| {
    // 0xfe divides strings, so any byte, a quote or a newline too, goes into a field.
    let parts: Vec<Vec<u8>> = data.split(|b| *b == 0xfe).map(<[u8]>::to_vec).collect();
    let part = |i: usize| parts.get(i).cloned().unwrap_or_default();
    let progress: Vec<Progress> = (0..parts.len().min(40))
        .map(|i| Progress {
            chat: part(i),
            id: i as u32,
            lines: (i..parts.len().min(i + 7)).map(part).collect(),
        })
        .collect();
    let requests: Vec<Request> = (0..parts.len().min(6))
        .map(|i| Request {
            request: part(i),
            chat: part(i + 1),
            id: u32::MAX - i as u32,
            text: part(i + 2),
            options: (0..(part(i).len() % 6))
                .map(|k| PermOption {
                    id: part(i + k),
                    kind: KINDS[k % 4].0,
                    label: part(k),
                })
                .collect(),
        })
        .collect();
    let progress = prepare_progress(&progress);
    let requests = prepare_requests(&requests);
    let file = live_body(&progress, &requests);
    assert!(file.len() <= 256 * 1024, "S21");

    let lua = Lua::new();
    lua.load(&file[..]).exec().expect("the live file loads");
    let live: Table = lua.globals().get("GnomishRelay_Live").expect("one global table");
    let got: Table = live.get("progress").unwrap();
    assert_eq!(got.raw_len(), progress.len());
    for (i, p) in progress.iter().enumerate() {
        let entry: Table = got.get(i + 1).unwrap();
        assert_eq!(bytes(entry.get("chat").unwrap()), p.chat);
        assert_eq!(entry.get::<u32>("id").unwrap(), p.id);
        let lines: Vec<mlua::String> = entry.get("lines").unwrap();
        assert_eq!(lines.into_iter().map(bytes).collect::<Vec<_>>(), p.lines);
    }
    let got: Table = live.get("permissions").unwrap();
    assert_eq!(got.raw_len(), requests.len());
    for (i, r) in requests.iter().enumerate() {
        let entry: Table = got.get(i + 1).unwrap();
        assert_eq!(bytes(entry.get("request").unwrap()), r.request);
        assert_eq!(bytes(entry.get("text").unwrap()), r.text);
        let options: Table = entry.get("options").unwrap();
        assert_eq!(options.raw_len(), r.options.len());
        for (k, o) in r.options.iter().enumerate() {
            let option: Table = options.get(k + 1).unwrap();
            assert_eq!(bytes(option.get("label").unwrap()), o.label);
            let kind: String = option.get("kind").unwrap();
            assert!(KINDS.iter().any(|(_, word)| *word == kind));
        }
    }
});
