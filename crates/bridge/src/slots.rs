//! The slot addons: the channel from the bridge into the game (SPEC.md 7.3).

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};
use protocol::slot::{SLOT_WINDOW, SLOTS};

use crate::fs_safe::{check_real_dir, write_atomic};

pub const BODY_FILE: &str = "Inbox.lua";

pub fn slot_name(n: usize) -> String {
    format!("GnomishRelay_S{n:04}")
}

fn toc(name: &str) -> String {
    format!(
        "## Interface: 16001\n## Title: {name}\n## LoadOnDemand: 1\n\
         ## Dependencies: GnomishRelay\n\n{BODY_FILE}\n"
    )
}

/// Makes every slot folder. WoW finds an addon only if it exists at launch
/// (SPEC.md 7.2, rule 1), so run this with the game closed.
pub fn install(addons: &Path, body: &[u8]) -> Result<()> {
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
        write_atomic(&dir, &format!("{name}.toc"), toc(&name).as_bytes())?;
        write_atomic(&dir, BODY_FILE, body)?;
    }
    Ok(())
}

/// Writes the body into the window of slots that starts at `next`, the next slot
/// that the addon reported (SPEC.md 7.3). Slots past the last one are skipped.
pub fn publish(addons: &Path, body: &[u8], next: usize) -> Result<()> {
    let first = next.clamp(1, SLOTS);
    let last = (first + SLOT_WINDOW - 1).min(SLOTS);
    for n in first..=last {
        let dir = addons.join(slot_name(n));
        write_atomic(&dir, BODY_FILE, body).with_context(|| {
            format!("slot {n} is not ready. Run `gnomish-relay install` with the game closed.")
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Lua, Table};
    use protocol::slot::{Reply, Status, prepare_replies, slot_body};

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
        install(addons.path(), &body(b"")).unwrap();
        for n in [1, 42, SLOTS] {
            let name = slot_name(n);
            let toc =
                fs::read_to_string(addons.path().join(&name).join(format!("{name}.toc"))).unwrap();
            assert!(toc.contains("## LoadOnDemand: 1"));
            assert!(toc.ends_with("Inbox.lua\n"));
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
        install(addons.path(), &body(b"")).unwrap();
        publish(addons.path(), &body(b"hello"), 100).unwrap();
        let read = |n| fs::read(addons.path().join(slot_name(n)).join(BODY_FILE)).unwrap();
        assert_eq!(read(99), body(b""));
        assert_eq!(read(100), body(b"hello"));
        assert_eq!(read(100 + SLOT_WINDOW - 1), body(b"hello"));
        assert_eq!(read(100 + SLOT_WINDOW), body(b""));
    }

    #[test]
    fn the_window_stops_at_the_last_slot() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), &body(b"")).unwrap();
        publish(addons.path(), &body(b"hello"), SLOTS - 2).unwrap();
        let file = addons.path().join(slot_name(SLOTS)).join(BODY_FILE);
        assert_eq!(fs::read(file).unwrap(), body(b"hello"));
    }

    #[test]
    fn a_next_slot_of_zero_starts_at_the_first_slot() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), &body(b"")).unwrap();
        publish(addons.path(), &body(b"hello"), 0).unwrap();
        let file = addons.path().join(slot_name(1)).join(BODY_FILE);
        assert_eq!(fs::read(file).unwrap(), body(b"hello"));
    }

    #[test]
    fn publish_before_install_is_an_error() {
        let addons = tempfile::tempdir().unwrap();
        assert!(publish(addons.path(), &body(b"hello"), 1).is_err());
    }

    #[test]
    fn a_published_body_runs_in_lua_5_1_and_gives_the_reply_back() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), &body(b"")).unwrap();
        let text = b"line \"one\"\nline |two| \\ \xff end";
        publish(addons.path(), &body(text), 1).unwrap();

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
}
