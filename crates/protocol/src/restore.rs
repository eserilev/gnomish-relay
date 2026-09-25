//! The restore bundle after a saved-data wipe (SPEC.md 7.6). It has its own file,
//! `Restore.lua`, so the slot body keeps its own 1 MiB bound.

use crate::ascii::{push_bytes, push_decimal};
use crate::lua::lua_string;
use crate::record::MAX_ID_LEN;
use crate::slot::{cut, keep_from};

pub const MAX_CHATS: usize = 16;
pub const MAX_HISTORY: usize = 10;
pub const MAX_NAME: usize = 64;
pub const MAX_CWD: usize = 1024;
pub const MAX_ENTRY_TEXT: usize = 500;

#[derive(Clone, Copy)]
pub enum Role {
    User,
    Agent,
    Error,
}

pub struct Entry {
    pub role: Role,
    pub id: u32,
    pub text: Vec<u8>,
}

pub struct Chat {
    pub id: Vec<u8>,
    pub name: Vec<u8>,
    pub agent: Vec<u8>,
    pub cwd: Vec<u8>,
    pub history: Vec<Entry>,
}

const HEAD: [u8; 32] = *b"GnomishRelay_Restore = {token = ";
const CHATS: [u8; 12] = *b", chats = {\n";
const TAIL: [u8; 3] = *b"}}\n";
const ID: [u8; 6] = *b"{id = ";
const NAME: [u8; 9] = *b", name = ";
const AGENT: [u8; 10] = *b", agent = ";
const CWD: [u8; 8] = *b", cwd = ";
const HISTORY: [u8; 14] = *b", history = {\n";
const CHAT_END: [u8; 4] = *b"}},\n";
const ROLE: [u8; 8] = *b"{role = ";
const ENTRY_ID: [u8; 7] = *b", id = ";
const TEXT: [u8; 9] = *b", text = ";
const ENTRY_END: [u8; 3] = *b"},\n";
const USER: [u8; 6] = *b"\"user\"";
const AGENT_ROLE: [u8; 7] = *b"\"agent\"";
const ERROR: [u8; 7] = *b"\"error\"";

fn prepare_entry(entry: &Entry) -> Entry {
    Entry {
        role: entry.role,
        id: entry.id,
        text: cut(&entry.text, MAX_ENTRY_TEXT),
    }
}

fn prepare_history(history: &[Entry]) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut i = keep_from(history.len(), MAX_HISTORY);
    while i < history.len() {
        out.push(prepare_entry(&history[i]));
        i += 1;
    }
    out
}

fn prepare_chat(chat: &Chat) -> Chat {
    Chat {
        id: cut(&chat.id, MAX_ID_LEN),
        name: cut(&chat.name, MAX_NAME),
        agent: cut(&chat.agent, MAX_ID_LEN),
        cwd: cut(&chat.cwd, MAX_CWD),
        history: prepare_history(&chat.history),
    }
}

/// Keeps the last `MAX_CHATS` chats and the last `MAX_HISTORY` messages of each.
/// Cuts every string to its limit.
#[must_use]
pub fn prepare_restore(chats: &[Chat]) -> Vec<Chat> {
    let mut out = Vec::new();
    let mut i = keep_from(chats.len(), MAX_CHATS);
    while i < chats.len() {
        out.push(prepare_chat(&chats[i]));
        i += 1;
    }
    out
}

fn push_role(out: &mut Vec<u8>, role: Role) {
    match role {
        Role::User => push_bytes(out, &USER),
        Role::Agent => push_bytes(out, &AGENT_ROLE),
        Role::Error => push_bytes(out, &ERROR),
    }
}

fn push_entry(out: &mut Vec<u8>, entry: &Entry) {
    push_bytes(out, &ROLE);
    push_role(out, entry.role);
    push_bytes(out, &ENTRY_ID);
    push_decimal(out, entry.id);
    push_bytes(out, &TEXT);
    push_bytes(out, &lua_string(&entry.text));
    push_bytes(out, &ENTRY_END);
}

fn push_history(out: &mut Vec<u8>, history: &[Entry]) {
    let mut i = 0;
    while i < history.len() {
        push_entry(out, &history[i]);
        i += 1;
    }
}

fn push_chat(out: &mut Vec<u8>, chat: &Chat) {
    push_bytes(out, &ID);
    push_bytes(out, &lua_string(&chat.id));
    push_bytes(out, &NAME);
    push_bytes(out, &lua_string(&chat.name));
    push_bytes(out, &AGENT);
    push_bytes(out, &lua_string(&chat.agent));
    push_bytes(out, &CWD);
    push_bytes(out, &lua_string(&chat.cwd));
    push_bytes(out, &HISTORY);
    push_history(out, &chat.history);
    push_bytes(out, &CHAT_END);
}

/// Every hole is an escaped string or a number, as in the slot body. Takes the
/// output of `prepare_restore`. An empty token matches no addon.
#[must_use]
pub fn restore_body(token: &[u8], chats: &[Chat]) -> Vec<u8> {
    let mut out = Vec::new();
    push_bytes(&mut out, &HEAD);
    push_bytes(&mut out, &lua_string(token));
    push_bytes(&mut out, &CHATS);
    let mut i = 0;
    while i < chats.len() {
        push_chat(&mut out, &chats[i]);
        i += 1;
    }
    push_bytes(&mut out, &TAIL);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(role: Role, id: u32, text: &[u8]) -> Entry {
        Entry {
            role,
            id,
            text: text.to_vec(),
        }
    }

    fn chat(id: &[u8], history: Vec<Entry>) -> Chat {
        Chat {
            id: id.to_vec(),
            name: b"lighthouse".to_vec(),
            agent: b"claude".to_vec(),
            cwd: b"Code/x".to_vec(),
            history,
        }
    }

    #[test]
    fn a_bundle_is_the_fixed_table_with_escaped_strings() {
        let chats = [chat(
            b"c1",
            vec![
                entry(Role::User, 5, b"hi \"there\""),
                entry(Role::Agent, 5, b"hello"),
            ],
        )];
        assert_eq!(
            restore_body(b"tok", &chats),
            b"GnomishRelay_Restore = {token = \"tok\", chats = {\n\
              {id = \"c1\", name = \"lighthouse\", agent = \"claude\", cwd = \"Code/x\", history = {\n\
              {role = \"user\", id = 5, text = \"hi \\034there\\034\"},\n\
              {role = \"agent\", id = 5, text = \"hello\"},\n\
              }},\n}}\n"
        );
    }

    #[test]
    fn an_empty_bundle_has_no_chats() {
        assert_eq!(
            restore_body(b"", &[]),
            b"GnomishRelay_Restore = {token = \"\", chats = {\n}}\n"
        );
    }

    #[test]
    fn prepare_keeps_the_last_chats_and_the_last_messages() {
        let history = (0..15).map(|i| entry(Role::User, i, b"x")).collect();
        let mut chats: Vec<Chat> = (0..20).map(|_| chat(b"old", Vec::new())).collect();
        chats.push(chat(b"new", history));

        let prepared = prepare_restore(&chats);
        assert_eq!(prepared.len(), MAX_CHATS);
        let last = &prepared[MAX_CHATS - 1];
        assert_eq!(last.id, b"new");
        assert_eq!(last.history.len(), MAX_HISTORY);
        assert_eq!(last.history[0].id, 5);
    }

    #[test]
    fn prepare_cuts_every_string_to_its_limit() {
        let long = vec![b'a'; 5000];
        let chats = [Chat {
            id: long.clone(),
            name: long.clone(),
            agent: long.clone(),
            cwd: long.clone(),
            history: vec![entry(Role::Error, 1, &long)],
        }];
        let c = &prepare_restore(&chats)[0];
        assert_eq!(
            [c.id.len(), c.name.len(), c.agent.len(), c.cwd.len()],
            [MAX_ID_LEN, MAX_NAME, MAX_ID_LEN, MAX_CWD]
        );
        assert_eq!(c.history[0].text.len(), MAX_ENTRY_TEXT);
    }
}
