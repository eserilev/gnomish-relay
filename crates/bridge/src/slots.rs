//! The slot addons: the channel from the bridge into the game (SPEC.md 7.3).

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};
use protocol::live::live_body;
use protocol::restore::restore_body;
use protocol::slot::{SLOT_WINDOW, SLOTS, slot_body};

use crate::fs_safe::{check_real_dir, write_atomic, write_atomic_unsynced};

pub const BODY_FILE: &str = "Inbox.lua";
pub const RESTORE_FILE: &str = "Restore.lua";
pub const LIVE_FILE: &str = "Live.lua";

/// The three Lua files of a slot: the replies, the restore bundle, and the live file.
pub struct Files {
    pub body: Vec<u8>,
    pub restore: Vec<u8>,
    pub live: Vec<u8>,
}

impl Files {
    /// No replies, no restore, and no activity.
    pub fn empty(now: u32) -> Files {
        Files {
            body: slot_body(now, &[]),
            restore: restore_body(b"", &[]),
            live: live_body(&[], &[]),
        }
    }

    fn write(&self, dir: &Path, write: fn(&Path, &str, &[u8]) -> Result<()>) -> Result<()> {
        write(dir, BODY_FILE, &self.body)?;
        write(dir, RESTORE_FILE, &self.restore)?;
        write(dir, LIVE_FILE, &self.live)
    }
}

pub fn slot_name(n: usize) -> String {
    format!("GnomishRelay_S{n:04}")
}

fn toc(name: &str) -> String {
    format!(
        "## Interface: 16001\n## Title: {name}\n## LoadOnDemand: 1\n\
         ## Dependencies: GnomishRelay\n\n{BODY_FILE}\n{RESTORE_FILE}\n{LIVE_FILE}\n"
    )
}

/// Makes every slot folder. WoW finds an addon only if it exists at launch
/// (SPEC.md 7.2, rule 1), so run this with the game closed. It writes 3000 files, so it
/// skips the sync: after a power cut, a second install repairs them.
pub fn install(addons: &Path, files: &Files) -> Result<()> {
    check_real_dir(addons)?;
    for n in 1..=SLOTS {
        let name = slot_name(n);
        let dir = addons.join(&name);
        match fs::create_dir(&dir) {
            Err(e) if e.kind() != ErrorKind::AlreadyExists => {
                return Err(e).with_context(|| format!("cannot make {}", dir.display()));
            }
            _ => {}
        }
        write_atomic_unsynced(&dir, &format!("{name}.toc"), toc(&name).as_bytes())?;
        files.write(&dir, write_atomic_unsynced)?;
    }
    Ok(())
}

/// Writes the files into the window of slots that starts at `next`, the next slot
/// that the addon reported (SPEC.md 7.3). Slots past the last one are skipped.
pub fn publish(addons: &Path, files: &Files, next: usize) -> Result<()> {
    let first = next.clamp(1, SLOTS);
    let last = (first + SLOT_WINDOW - 1).min(SLOTS);
    for n in first..=last {
        let dir = addons.join(slot_name(n));
        let not_ready =
            || format!("slot {n} is not ready. Run `gnomish-relay install` with the game closed.");
        files.write(&dir, write_atomic).with_context(not_ready)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Lua, Table};
    use protocol::slot::{Reply, Status, prepare_replies};

    fn files(text: &[u8]) -> Files {
        Files {
            body: body(text),
            ..Files::empty(0)
        }
    }

    fn body(text: &[u8]) -> Vec<u8> {
        let reply = Reply {
            chat: b"c1".to_vec(),
            id: 7,
            status: Status::Done,
            text: text.to_vec(),
        };
        slot_body(1_790_211_079, &prepare_replies(&[reply]))
    }

    #[test]
    fn install_makes_every_slot_with_its_toc() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), &files(b"")).unwrap();
        for n in [1, 42, SLOTS] {
            let name = slot_name(n);
            let toc =
                fs::read_to_string(addons.path().join(&name).join(format!("{name}.toc"))).unwrap();
            assert!(toc.contains("## LoadOnDemand: 1"));
            assert!(toc.ends_with("Inbox.lua\nRestore.lua\nLive.lua\n"));
        }
        assert_eq!(fs::read_dir(addons.path()).unwrap().count(), SLOTS);
    }

    #[test]
    fn slot_names_have_four_digits() {
        assert_eq!(slot_name(1), "GnomishRelay_S0001");
        assert_eq!(slot_name(1000), "GnomishRelay_S1000");
    }

    #[test]
    fn publish_writes_only_the_window_from_the_next_slot() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), &files(b"")).unwrap();
        publish(addons.path(), &files(b"hello"), 100).unwrap();
        let read = |n| fs::read(addons.path().join(slot_name(n)).join(BODY_FILE)).unwrap();
        assert_eq!(read(99), body(b""));
        assert_eq!(read(100), body(b"hello"));
        assert_eq!(read(100 + SLOT_WINDOW - 1), body(b"hello"));
        assert_eq!(read(100 + SLOT_WINDOW), body(b""));
    }

    #[test]
    fn the_window_stops_at_the_last_slot() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), &files(b"")).unwrap();
        publish(addons.path(), &files(b"hello"), SLOTS - 2).unwrap();
        let file = addons.path().join(slot_name(SLOTS)).join(BODY_FILE);
        assert_eq!(fs::read(file).unwrap(), body(b"hello"));
    }

    #[test]
    fn a_next_slot_of_zero_starts_at_the_first_slot() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), &files(b"")).unwrap();
        publish(addons.path(), &files(b"hello"), 0).unwrap();
        let file = addons.path().join(slot_name(1)).join(BODY_FILE);
        assert_eq!(fs::read(file).unwrap(), body(b"hello"));
    }

    #[test]
    fn publish_before_install_is_an_error() {
        let addons = tempfile::tempdir().unwrap();
        assert!(publish(addons.path(), &files(b"hello"), 1).is_err());
    }

    #[test]
    fn a_published_body_runs_in_lua_5_1_and_gives_the_reply_back() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), &files(b"")).unwrap();
        let text = b"line \"one\"\nline |two| \\ \xff end";
        publish(addons.path(), &files(text), 1).unwrap();

        let code = fs::read(addons.path().join(slot_name(1)).join(BODY_FILE)).unwrap();
        let lua = Lua::new();
        lua.load(&code[..]).exec().unwrap();
        let data: Table = lua.globals().get("GnomishRelay_SlotData").unwrap();
        let reply: Table = data.get::<Table>("replies").unwrap().get(1).unwrap();

        assert_eq!(data.get::<u32>("proto").unwrap(), 1);
        assert_eq!(reply.get::<String>("status").unwrap(), "done");
        assert_eq!(
            &*reply.get::<mlua::String>("text").unwrap().as_bytes(),
            text
        );
    }

    #[test]
    fn a_published_restore_file_runs_in_lua_5_1_and_gives_the_chats_back() {
        use protocol::restore::{Chat, Entry, Role, prepare_restore};
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), &files(b"")).unwrap();
        let text = b"}} GnomishRelay_SlotData = nil \"\n\xff";
        let chats = [Chat {
            id: b"c1".to_vec(),
            name: b"x\" end".to_vec(),
            agent: b"claude".to_vec(),
            cwd: b"Code".to_vec(),
            history: vec![Entry {
                role: Role::User,
                id: 9,
                text: text.to_vec(),
            }],
        }];
        let restore = restore_body(b"tok", &prepare_restore(&chats));
        let with_restore = Files {
            restore,
            ..files(b"")
        };
        publish(addons.path(), &with_restore, 1).unwrap();

        let code = fs::read(addons.path().join(slot_name(1)).join(RESTORE_FILE)).unwrap();
        let lua = Lua::new();
        lua.load(&code[..]).exec().unwrap();
        let data: Table = lua.globals().get("GnomishRelay_Restore").unwrap();
        let chat: Table = data.get::<Table>("chats").unwrap().get(1).unwrap();
        let entry: Table = chat.get::<Table>("history").unwrap().get(1).unwrap();

        assert_eq!(data.get::<String>("token").unwrap(), "tok");
        assert_eq!(chat.get::<String>("name").unwrap(), "x\" end");
        assert_eq!(entry.get::<u32>("id").unwrap(), 9);
        assert_eq!(
            &*entry.get::<mlua::String>("text").unwrap().as_bytes(),
            text
        );
    }

    #[test]
    fn a_published_live_file_runs_in_lua_5_1_and_gives_the_request_back() {
        use protocol::live::{OptionKind, PermOption, Request, prepare_requests};
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), &files(b"")).unwrap();
        let text = b"rm -rf ~ \"}} GnomishRelay_Live = nil\n\x1b[0m\xff";
        let request = Request {
            request: b"p1a".to_vec(),
            chat: b"c1".to_vec(),
            id: 4,
            text: text.to_vec(),
            options: vec![PermOption {
                id: b"o1".to_vec(),
                kind: OptionKind::AllowOnce,
                label: b"\"}}".to_vec(),
            }],
        };
        let with_live = Files {
            live: live_body(&[], &prepare_requests(&[request])),
            ..files(b"")
        };
        publish(addons.path(), &with_live, 1).unwrap();

        let code = fs::read(addons.path().join(slot_name(1)).join(LIVE_FILE)).unwrap();
        let lua = Lua::new();
        lua.load(&code[..]).exec().unwrap();
        let live: Table = lua.globals().get("GnomishRelay_Live").unwrap();
        let request: Table = live.get::<Table>("permissions").unwrap().get(1).unwrap();
        assert_eq!(
            &*request.get::<mlua::String>("text").unwrap().as_bytes(),
            text
        );
        let option: Table = request.get::<Table>("options").unwrap().get(1).unwrap();
        assert_eq!(option.get::<String>("kind").unwrap(), "allow_once");
    }
}
