//! S18 and S19 against the real Lua 5.1: random chats go through `prepare_restore`
//! and the writer, and the file loads back as the same fields.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mlua::{Lua, Table};
use protocol::restore::{Chat, Entry, Role, prepare_restore, restore_body};

fn bytes(value: mlua::String) -> Vec<u8> {
    value.as_bytes().to_vec()
}

fuzz_target!(|data: &[u8]| {
    let parts: Vec<Vec<u8>> = data.split(|b| *b == 0xfe).map(<[u8]>::to_vec).collect();
    let part = |i: usize| parts.get(i).cloned().unwrap_or_default();
    let roles = [Role::User, Role::Agent, Role::Error];
    let chats: Vec<Chat> = (0..parts.len().min(20))
        .map(|i| Chat {
            id: part(i),
            name: part(i + 1),
            agent: part(i + 2),
            cwd: part(i + 3),
            history: (i..parts.len().min(i + 12))
                .map(|k| Entry {
                    role: roles[k % 3],
                    id: k as u32,
                    text: part(k),
                })
                .collect(),
        })
        .collect();
    let token = part(0);
    let token = &token[..token.len().min(32)];
    let chats = prepare_restore(&chats);
    let file = restore_body(token, &chats);
    assert!(file.len() <= 512 * 1024, "S19");

    let lua = Lua::new();
    lua.load(&file[..]).exec().expect("the restore file loads");
    let restore: Table = lua.globals().get("GnomishRelay_Restore").expect("one global table");
    assert_eq!(bytes(restore.get("token").unwrap()), token);
    let got: Table = restore.get("chats").unwrap();
    assert_eq!(got.raw_len(), chats.len());
    for (i, c) in chats.iter().enumerate() {
        let chat: Table = got.get(i + 1).unwrap();
        assert_eq!(bytes(chat.get("id").unwrap()), c.id);
        assert_eq!(bytes(chat.get("name").unwrap()), c.name);
        assert_eq!(bytes(chat.get("cwd").unwrap()), c.cwd);
        let history: Table = chat.get("history").unwrap();
        assert_eq!(history.raw_len(), c.history.len());
        for (k, e) in c.history.iter().enumerate() {
            let entry: Table = history.get(k + 1).unwrap();
            assert_eq!(bytes(entry.get("text").unwrap()), e.text);
            assert_eq!(entry.get::<u32>("id").unwrap(), e.id);
        }
    }
});
