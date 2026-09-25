//! The bridge state across a restart: `state.json` in the data folder (SPEC.md 8.3).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::fs_safe::write_atomic;
use crate::history::History;
use crate::relay::{AgentSession, ChatId, Job, MessageId};

const FILE: &str = "state.json";
/// 1000 seen ids and 30 records of 32 KiB fit in far less.
const MAX_FILE: u64 = 16 * 1024 * 1024;

/// A field that an older bridge did not write loads as its default.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct State {
    pub next_slot: usize,
    /// The replay store, oldest first.
    pub seen: Vec<(String, u32)>,
    pub records: Vec<SavedRecord>,
    /// The messages that wait for a run, in queue order.
    pub waiting: Vec<Job>,
    pub history: History,
    pub tokens: Vec<String>,
    pub retired: Vec<String>,
    pub restore_for: Option<String>,
    pub client_build: Option<String>,
    pub sessions: Vec<AgentSession>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SavedRecord {
    pub token: String,
    pub chat: ChatId,
    pub id: MessageId,
    pub status: SavedStatus,
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub enum SavedStatus {
    Working,
    Done,
    Error,
}

/// `None` when there is no state yet. A damaged file is an error, not a fresh start:
/// a fresh start forgets which messages ran.
pub fn load(dir: &Path) -> Result<Option<State>> {
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

pub fn save(dir: &Path, state: &State) -> Result<()> {
    write_atomic(dir, FILE, &serde_json::to_vec(state)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::Session;

    fn state() -> State {
        State {
            next_slot: 57,
            seen: vec![("tok".into(), 7)],
            records: vec![SavedRecord {
                token: "tok".into(),
                chat: ChatId("c1".into()),
                id: MessageId(7),
                status: SavedStatus::Done,
                text: "done \"quoted\"\n".into(),
            }],
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
            }],
            tokens: vec!["tok".into()],
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

    #[test]
    fn no_file_is_no_state() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap(), None);
    }

    #[test]
    fn a_damaged_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE), "{\"next_slot\": ").unwrap();
        assert!(load(dir.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_state_file_that_is_a_link_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere.json");
        fs::write(&target, "{}").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(FILE)).unwrap();
        assert!(load(dir.path()).is_err());
    }
}
