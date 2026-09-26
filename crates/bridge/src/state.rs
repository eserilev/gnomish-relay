//! The bridge state across a restart: `state.json` in the data folder (SPEC.md 8.3). The
//! Timeways lane keeps its own `state.json` in `timeways/` (SPEC.md 9.7, decision 4).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::fs_safe::write_atomic;
use crate::history::History;
use crate::lane::LaneState;
pub use crate::lane::{SavedRecord, SavedStatus};
use crate::relay::{AgentSession, Job};

const FILE: &str = "state.json";
/// 1000 seen ids and 30 records of 32 KiB fit in far less.
const MAX_FILE: u64 = 16 * 1024 * 1024;

/// A field that an older bridge did not write loads as its default. The lane part is
/// flattened, so the file keeps the shape that it had before lanes.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct State {
    #[serde(flatten)]
    pub lane: LaneState,
    /// The messages that wait for a run, in queue order.
    pub waiting: Vec<Job>,
    pub history: History,
    pub restore_for: Option<String>,
    pub sessions: Vec<AgentSession>,
}

/// `None` when there is no state yet. A damaged file is an error, not a fresh start:
/// a fresh start forgets which messages ran.
pub fn load<T: DeserializeOwned>(dir: &Path) -> Result<Option<T>> {
    let path = dir.join(FILE);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    if !meta.is_file() || meta.len() > MAX_FILE {
        bail!("{} is not a state file", path.display());
    }
    let text = fs::read_to_string(&path)?;
    let state = serde_json::from_str(&text).with_context(|| {
        format!(
            "{} is damaged. Move it away to start with no state.",
            path.display()
        )
    })?;
    Ok(Some(state))
}

pub fn save<T: Serialize>(dir: &Path, state: &T) -> Result<()> {
    write_atomic(dir, FILE, &serde_json::to_vec(state)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lane::{ChatId, MessageId};
    use crate::relay::Session;

    fn state() -> State {
        State {
            lane: LaneState {
                next_slot: 57,
                seen: vec![("tok".into(), 7)],
                records: vec![SavedRecord {
                    token: "tok".into(),
                    chat: ChatId("c1".into()),
                    id: MessageId(7),
                    status: SavedStatus::Done,
                    text: "done \"quoted\"\n".into(),
                }],
                tokens: vec!["tok".into()],
                ..LaneState::default()
            },
            waiting: vec![Job {
                token: "tok".into(),
                chat: ChatId("c1".into()),
                id: MessageId(8),
                agent: "claude".into(),
                permission: crate::config::Permission::AutoEdit,
                cwd: "/home/x".into(),
                session: Session::Resume,
                resume: None,
                text: "next".into(),
                work: crate::relay::Work::Attach {
                    session: "s1".into(),
                    fork: true,
                },
            }],
            restore_for: Some("new".into()),
            ..State::default()
        }
    }

    #[test]
    fn a_saved_state_loads_the_same() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &state()).unwrap();
        assert_eq!(load(dir.path()).unwrap(), Some(state()));
    }

    /// A file that the bridge wrote before lanes, field for field.
    #[test]
    fn a_state_file_from_before_lanes_loads_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let old = r#"{"next_slot":57,"seen":[["tok",7]],"records":[{"token":"tok","chat":"c1","id":7,"status":"Done","text":"done"}],"waiting":[],"history":{"chats":[]},"tokens":["tok"],"retired":["gone"],"restore_for":"new","client_build":"70009","sessions":[{"chat":"c1","agent":"claude","cwd":"/x","id":"s1"}]}"#;
        fs::write(dir.path().join(FILE), old).unwrap();
        let state: State = load(dir.path()).unwrap().unwrap();
        assert_eq!(state.lane.next_slot, 57);
        assert_eq!(state.lane.seen, [("tok".to_owned(), 7)]);
        assert_eq!(state.lane.records[0].text, "done");
        assert_eq!(state.lane.tokens, ["tok"]);
        assert_eq!(state.lane.retired, ["gone"]);
        assert_eq!(state.lane.client_build.as_deref(), Some("70009"));
        assert_eq!(state.restore_for.as_deref(), Some("new"));
        assert_eq!(state.sessions[0].id, "s1");
        save(dir.path(), &state).unwrap();
        let again: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join(FILE)).unwrap()).unwrap();
        assert_eq!(
            again,
            serde_json::from_str::<serde_json::Value>(old).unwrap()
        );
    }

    #[test]
    fn a_saved_timeways_state_loads_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let lane = LaneState {
            next_slot: 3,
            seen: vec![("tw".into(), 1)],
            ..LaneState::default()
        };
        save(dir.path(), &lane).unwrap();
        assert_eq!(load::<LaneState>(dir.path()).unwrap(), Some(lane));
    }

    #[test]
    fn no_file_is_no_state() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load::<State>(dir.path()).unwrap(), None);
    }

    #[test]
    fn a_damaged_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE), "{\"next_slot\": ").unwrap();
        assert!(load::<State>(dir.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_state_file_that_is_a_link_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere.json");
        fs::write(&target, "{}").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(FILE)).unwrap();
        assert!(load::<State>(dir.path()).is_err());
    }
}
