//! Any sequence of frames and run results through the bridge state machine. It
//! checks the promises of models/transport.qnt on the real code: no message runs
//! twice, the body never holds more than 30 records, and no job leaves the root.
//! Restarts through `state.json` come at random places, and the promises still hold.
//! A Timeways lane runs next to it (SPEC.md 9.7): each frame goes to one lane, a
//! Timeways record never becomes a job, and each Timeways message reaches the story once.
#![no_main]

use std::collections::HashSet;

use bridge::config::{Permission, Policy};
use bridge::relay::{Folders, Relay};
use bridge::timeways::Timeways;
use libfuzzer_sys::fuzz_target;
use protocol::record::Record;

const CHATS: [&str; 3] = ["c1", "c2", "relay"];
const FOLDERS: [&str; 5] = ["", "sub", "../..", "/etc", "a/../../b"];
const FLAGS: [&str; 10] = [
    "",
    "n",
    "stop",
    "h",
    "h;restored",
    "level=full-auto",
    "perm=p1:o1:0123456789abcdef",
    "agent=codex",
    "read=1,2,3,4",
    "next=9",
];

/// A Timeways record carries this text, so a job with it came from the wrong lane.
const STORY: &[u8] = b"story";

fn record(bytes: &[u8]) -> Record {
    let pick = |i: usize| usize::from(bytes.get(i).copied().unwrap_or(0));
    Record {
        token: if pick(0) % 5 == 0 {
            b"other".to_vec()
        } else {
            b"tok".to_vec()
        },
        chat: CHATS[pick(1) % CHATS.len()].as_bytes().to_vec(),
        id: u32::from(bytes.get(2).copied().unwrap_or(0) % 40),
        cwd: FOLDERS[pick(3) % FOLDERS.len()].as_bytes().to_vec(),
        flags: FLAGS[pick(4) % FLAGS.len()].as_bytes().to_vec(),
        name: Vec::new(),
        text: b"x".to_vec(),
    }
}

fn policy() -> Policy {
    Policy {
        folders: Folders {
            roots: vec![b"/r".to_vec()],
            base: b"/r".to_vec(),
        },
        agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
        default_agent: "claude".into(),
    }
}

fuzz_target!(|data: &[u8]| {
    let mut relay = Relay::new(policy());
    let mut timeways = Timeways::new();
    let mut ran = HashSet::new();
    let mut told = HashSet::new();
    let mut now = 1_790_211_079u32;
    for step in data.chunks(6) {
        now += u32::from(step[0] % 20);
        if step[0] % 3 == 0 {
            while let Some(job) = relay.next_job() {
                assert_ne!(job.text.as_bytes(), STORY, "a Timeways record ran");
                assert!(
                    ran.insert((job.token.clone(), job.id)),
                    "a message ran twice"
                );
                assert!(
                    job.permission != Permission::FullAuto,
                    "the game raised the level"
                );
                assert!(
                    job.cwd == "/r" || job.cwd.starts_with("/r/"),
                    "left the root: {}",
                    job.cwd
                );
                let result = if step.len() > 1 && step[1] % 2 == 0 {
                    Ok("ok".into())
                } else {
                    Err("no".into())
                };
                relay.keep_session(&job, Some(format!("s{}", job.id.0)));
                relay.finish(&job, result);
            }
        } else if step[0] % 7 == 1 {
            // No run is in progress here, so a restart changes nothing.
            let saved = relay.to_state();
            relay = Relay::from_state(policy(), relay.to_state());
            assert_eq!(relay.to_state(), saved);
            timeways = Timeways::from_state(timeways.to_state());
        } else if step[0] % 2 == 0 {
            let records: Vec<Record> = step[1..].chunks(5).map(record).collect();
            relay.on_frame(&records, now);
        } else {
            let records: Vec<Record> = step[1..]
                .chunks(5)
                .map(|bytes| Record {
                    text: STORY.to_vec(),
                    ..record(bytes)
                })
                .collect();
            timeways.on_frame(&records, now);
        }
        for message in timeways.take_messages() {
            assert!(
                told.insert((message.token.clone(), message.id)),
                "a Timeways message reached the story twice"
            );
            timeways.answer(&message, Err("no story".into()));
        }
        assert!(relay.unread() <= protocol::slot::MAX_REPLIES);
        assert!(timeways.unread() <= protocol::slot::MAX_REPLIES);
    }
});
