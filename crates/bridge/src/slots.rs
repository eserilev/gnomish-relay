//! The slot addons: the channel from the bridge into the game (SPEC.md 7.3).

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};
use protocol::apps::{App, push_slot_global};
use protocol::live::{live_body, no_notices};
use protocol::restore::restore_body;
use protocol::slot::{Reply, SLOT_WINDOW, SLOTS, prepare_replies, slot_body};

use crate::app_files::addon_name;
use crate::fs_safe::{check_real_dir, write_atomic_if_changed, write_atomic_unsynced};
use crate::wow_client::TOC_INTERFACE;

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
    pub fn empty(app: App, now: u32) -> Files {
        Files {
            body: slot_body(app, now, &[]),
            restore: restore_body(app, b"", &[]),
            live: live_body(app, &[], &[], &no_notices()),
        }
    }

    fn write(&self, dir: &Path, write: fn(&Path, &str, &[u8]) -> Result<()>) -> Result<()> {
        write(dir, BODY_FILE, &self.body)?;
        write(dir, RESTORE_FILE, &self.restore)?;
        write(dir, LIVE_FILE, &self.live)
    }
}

/// Adds the count of strips with a bad tag after the body (SPEC.md 7.3). A count of 0
/// adds nothing, so a normal body stays the proved one.
pub fn with_bad_tags(mut body: Vec<u8>, app: App, count: u32) -> Vec<u8> {
    if count == 0 {
        return body;
    }
    push_slot_global(&mut body, app);
    body.extend_from_slice(format!(".badTags = {count}\n").as_bytes());
    body
}

pub fn slot_name(app: App, n: usize) -> String {
    format!("{}_S{n:04}", addon_name(app))
}

/// The title is grey, and asks the player to leave the slot on: the addon list of the
/// game shows all 1000 slots.
fn toc(app: App, n: usize) -> String {
    let addon = addon_name(app);
    let title = match app {
        App::Relay => "Gnomish Relay",
        App::Timeways => "Timeways",
    };
    format!(
        "## Interface: {TOC_INTERFACE}\n## Title: |cff808080{title} reply slot {n:04} (leave on)|r\n\
         ## LoadOnDemand: 1\n## Dependencies: {addon}\n\n{BODY_FILE}\n{RESTORE_FILE}\n{LIVE_FILE}\n"
    )
}

/// Only an install with the game closed makes slots, so a publish into missing slots
/// never helps.
pub fn is_installed(addons: &Path, app: App) -> bool {
    check_real_dir(&addons.join(slot_name(app, 1))).is_ok()
}

/// Makes every slot folder. WoW finds an addon only if it exists at launch
/// (SPEC.md 7.2, rule 1), so run this with the game closed. It writes 3000 files, so it
/// skips the sync: after a power cut, a second install repairs them.
pub fn install(addons: &Path, app: App, files: &Files) -> Result<()> {
    check_real_dir(addons)?;
    for n in 1..=SLOTS {
        let name = slot_name(app, n);
        let dir = addons.join(&name);
        match fs::create_dir(&dir) {
            Err(e) if e.kind() != ErrorKind::AlreadyExists => {
                return Err(e).with_context(|| format!("cannot make {}", dir.display()));
            }
            _ => {}
        }
        write_atomic_unsynced(&dir, &format!("{name}.toc"), toc(app, n).as_bytes())?;
        files.write(&dir, write_atomic_unsynced)?;
    }
    Ok(())
}

/// Writes the files into the window of slots that starts at `next`, the next slot
/// that the addon reported (SPEC.md 7.3). Slots past the last one are skipped, and so
/// is a file that did not change.
/// `gnomish-relay say`: one done reply of the relay, from slot `next` on.
pub fn publish_reply(addons: &Path, reply: Reply, next: usize, now: u32) -> Result<()> {
    let files = Files {
        body: slot_body(App::Relay, now, &prepare_replies(&[reply])),
        ..Files::empty(App::Relay, now)
    };
    publish(addons, App::Relay, &files, next)
}

/// Writes the window of each game (SPEC.md 7.3). A slot of two windows holds the same
/// bytes, so its second write is skipped.
pub fn publish_windows(addons: &Path, app: App, files: &Files, windows: &[usize]) -> Result<()> {
    for next in windows {
        publish(addons, app, files, *next)?;
    }
    Ok(())
}

pub fn publish(addons: &Path, app: App, files: &Files, next: usize) -> Result<()> {
    let first = next.clamp(1, SLOTS);
    let last = (first + SLOT_WINDOW - 1).min(SLOTS);
    for n in first..=last {
        let dir = addons.join(slot_name(app, n));
        let not_ready =
            || format!("slot {n} is not ready. Run `gnomish-relay install` with the game closed.");
        files
            .write(&dir, write_atomic_if_changed)
            .with_context(not_ready)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_tag_count_sets_a_field_of_the_body_global() {
        let body = with_bad_tags(slot_body(App::Relay, 0, &[]), App::Relay, 3);
        assert!(body.ends_with(b"}}\nGnomishRelay_SlotData.badTags = 3\n"));
    }

    #[test]
    fn no_bad_tag_leaves_the_body_as_it_is() {
        let body = slot_body(App::Relay, 0, &[]);
        assert_eq!(with_bad_tags(body.clone(), App::Relay, 0), body);
    }
    use mlua::{Lua, Table};
    use protocol::slot::Status;

    fn files(text: &[u8]) -> Files {
        Files {
            body: body(text),
            ..Files::empty(App::Relay, 0)
        }
    }

    fn body(text: &[u8]) -> Vec<u8> {
        let reply = Reply {
            chat: b"c1".to_vec(),
            id: 7,
            status: Status::Done,
            text: text.to_vec(),
        };
        slot_body(App::Relay, 1_790_211_079, &prepare_replies(&[reply]))
    }

    #[test]
    fn install_makes_every_slot_with_its_toc() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), App::Relay, &files(b"")).unwrap();
        for n in [1, 42, SLOTS] {
            let name = slot_name(App::Relay, n);
            let toc =
                fs::read_to_string(addons.path().join(&name).join(format!("{name}.toc"))).unwrap();
            assert!(toc.contains("## LoadOnDemand: 1"));
            assert!(toc.ends_with("Inbox.lua\nRestore.lua\nLive.lua\n"));
        }
        assert_eq!(fs::read_dir(addons.path()).unwrap().count(), SLOTS);
    }

    #[test]
    fn a_said_reply_goes_into_the_slots_of_the_window_from_the_next_slot() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), App::Relay, &files(b"")).unwrap();
        let reply = Reply {
            chat: b"c1".to_vec(),
            id: 7,
            status: Status::Done,
            text: b"said by hand".to_vec(),
        };

        publish_reply(addons.path(), reply, 5, 1_790_211_079).unwrap();

        let inbox = |n| {
            let dir = addons.path().join(slot_name(App::Relay, n));
            fs::read_to_string(dir.join(BODY_FILE)).unwrap()
        };
        assert!(!inbox(4).contains("said by hand"));
        assert!(inbox(5).contains("said by hand"));
        assert!(inbox(5 + SLOT_WINDOW - 1).contains("said by hand"));
    }

    #[test]
    fn a_publish_writes_the_window_of_each_game() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), App::Relay, &files(b"")).unwrap();

        publish_windows(addons.path(), App::Relay, &files(b"both"), &[500, 3]).unwrap();

        let inbox = |n| {
            let dir = addons.path().join(slot_name(App::Relay, n));
            fs::read_to_string(dir.join(BODY_FILE)).unwrap()
        };
        assert!(inbox(3).contains("both"));
        assert!(inbox(500).contains("both"));
        assert!(!inbox(100).contains("both"));
    }

    #[test]
    fn slot_names_have_four_digits() {
        assert_eq!(slot_name(App::Relay, 1), "GnomishRelay_S0001");
        assert_eq!(slot_name(App::Relay, 1000), "GnomishRelay_S1000");
    }

    #[test]
    fn timeways_slots_have_their_own_names_and_depend_on_timeways() {
        assert_eq!(slot_name(App::Timeways, 7), "Timeways_S0007");
        let toc = toc(App::Timeways, 7);
        assert!(toc.contains("## Dependencies: Timeways\n"));
    }

    #[test]
    fn a_slot_shows_in_the_addon_list_as_a_grey_reply_slot_to_leave_on() {
        let toc = toc(App::Relay, 42);

        assert!(toc.contains("## Title: |cff808080Gnomish Relay reply slot 0042 (leave on)|r\n"));
    }

    #[test]
    fn slots_count_as_installed_only_after_an_install() {
        let addons = tempfile::tempdir().unwrap();
        assert!(!is_installed(addons.path(), App::Timeways));
        install(
            addons.path(),
            App::Timeways,
            &Files::empty(App::Timeways, 0),
        )
        .unwrap();
        assert!(is_installed(addons.path(), App::Timeways));
        assert!(!is_installed(addons.path(), App::Relay));
    }

    #[test]
    fn publish_writes_only_the_window_from_the_next_slot() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), App::Relay, &files(b"")).unwrap();
        publish(addons.path(), App::Relay, &files(b"hello"), 100).unwrap();
        let read =
            |n| fs::read(addons.path().join(slot_name(App::Relay, n)).join(BODY_FILE)).unwrap();
        assert_eq!(read(99), body(b""));
        assert_eq!(read(100), body(b"hello"));
        assert_eq!(read(100 + SLOT_WINDOW - 1), body(b"hello"));
        assert_eq!(read(100 + SLOT_WINDOW), body(b""));
    }

    #[test]
    fn publish_writes_a_changed_file_and_leaves_an_unchanged_one_alone() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), App::Relay, &files(b"")).unwrap();
        let slot = addons.path().join(slot_name(App::Relay, 1));
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        for file in [BODY_FILE, RESTORE_FILE] {
            let open = fs::File::options().write(true).open(slot.join(file));
            open.unwrap().set_modified(old).unwrap();
        }

        publish(addons.path(), App::Relay, &files(b"hello"), 1).unwrap();

        let modified = |file| fs::metadata(slot.join(file)).unwrap().modified().unwrap();
        assert_ne!(modified(BODY_FILE), old);
        assert_eq!(modified(RESTORE_FILE), old);
    }

    #[test]
    fn the_window_stops_at_the_last_slot() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), App::Relay, &files(b"")).unwrap();
        publish(addons.path(), App::Relay, &files(b"hello"), SLOTS - 2).unwrap();
        let file = addons
            .path()
            .join(slot_name(App::Relay, SLOTS))
            .join(BODY_FILE);
        assert_eq!(fs::read(file).unwrap(), body(b"hello"));
    }

    #[test]
    fn a_next_slot_of_zero_starts_at_the_first_slot() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), App::Relay, &files(b"")).unwrap();
        publish(addons.path(), App::Relay, &files(b"hello"), 0).unwrap();
        let file = addons.path().join(slot_name(App::Relay, 1)).join(BODY_FILE);
        assert_eq!(fs::read(file).unwrap(), body(b"hello"));
    }

    #[test]
    fn publish_before_install_is_an_error() {
        let addons = tempfile::tempdir().unwrap();
        assert!(publish(addons.path(), App::Relay, &files(b"hello"), 1).is_err());
    }

    #[test]
    fn a_published_body_runs_in_lua_5_1_and_gives_the_reply_back() {
        let addons = tempfile::tempdir().unwrap();
        install(addons.path(), App::Relay, &files(b"")).unwrap();
        let text = b"line \"one\"\nline |two| \\ \xff end";
        publish(addons.path(), App::Relay, &files(text), 1).unwrap();

        let code = fs::read(addons.path().join(slot_name(App::Relay, 1)).join(BODY_FILE)).unwrap();
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
        install(addons.path(), App::Relay, &files(b"")).unwrap();
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
        let restore = restore_body(App::Relay, b"tok", &prepare_restore(&chats));
        let with_restore = Files {
            restore,
            ..files(b"")
        };
        publish(addons.path(), App::Relay, &with_restore, 1).unwrap();

        let code = fs::read(
            addons
                .path()
                .join(slot_name(App::Relay, 1))
                .join(RESTORE_FILE),
        )
        .unwrap();
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
        install(addons.path(), App::Relay, &files(b"")).unwrap();
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
            live: live_body(
                App::Relay,
                &[],
                &prepare_requests(&[request]),
                &no_notices(),
            ),
            ..files(b"")
        };
        publish(addons.path(), App::Relay, &with_live, 1).unwrap();

        let code = fs::read(addons.path().join(slot_name(App::Relay, 1)).join(LIVE_FILE)).unwrap();
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
