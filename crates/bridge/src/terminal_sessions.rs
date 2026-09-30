//! The terminal sessions that the hooks report, each with at most one notice (SPEC.md
//! 10.3). The proved `apply_event` and `expire` of `protocol` change the table. The table
//! and the id counter live in `notices.json`, so a restart keeps them.

use std::path::{Path, PathBuf};

use anyhow::Result;
use protocol::live::{Notice, NoticeKind, Notices, Source as LiveSource, prepare_notices};
use protocol::notice::{MAX_REPO, MAX_TEXT, notice_text};
use protocol::sessions::{Event, EventKind, Session, apply_event, expire};
use serde::{Deserialize, Serialize};

use crate::fs_safe::{read_at_most, write_private};
use crate::spool::{Source, SpoolEvent, SpoolFile};

pub const STATE_FILE: &str = "notices.json";
/// 32 sessions with 700 bytes each fit well below this.
const MAX_STATE: u64 = 1 << 20;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum SavedKind {
    Waiting,
    Finished,
    Failed,
}

#[derive(Serialize, Deserialize)]
struct SavedNotice {
    id: u32,
    at: u32,
    source: Source,
    kind: SavedKind,
    repo: String,
    took: u32,
    text: String,
}

#[derive(Serialize, Deserialize)]
struct SavedSession {
    id: String,
    turn_started: u32,
    last: u32,
    notice: Option<SavedNotice>,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    last_id: u32,
    sessions: Vec<SavedSession>,
}

pub struct TerminalSessions {
    table: Vec<Session>,
    last_id: u32,
    dir: PathBuf,
}

fn live_source(source: Source) -> LiveSource {
    match source {
        Source::Claude => LiveSource::Claude,
        Source::Codex => LiveSource::Codex,
    }
}

fn saved_source(source: LiveSource) -> Source {
    match source {
        LiveSource::Claude => Source::Claude,
        LiveSource::Codex => Source::Codex,
    }
}

fn live_kind(kind: SavedKind) -> NoticeKind {
    match kind {
        SavedKind::Waiting => NoticeKind::Waiting,
        SavedKind::Finished => NoticeKind::Finished,
        SavedKind::Failed => NoticeKind::Failed,
    }
}

fn saved_kind(kind: NoticeKind) -> SavedKind {
    match kind {
        NoticeKind::Waiting => SavedKind::Waiting,
        NoticeKind::Finished => SavedKind::Finished,
        NoticeKind::Failed => SavedKind::Failed,
    }
}

fn event_kind(event: SpoolEvent) -> EventKind {
    match event {
        SpoolEvent::SessionStart => EventKind::SessionStart,
        SpoolEvent::TurnStart => EventKind::TurnStart,
        SpoolEvent::Waiting => EventKind::Waiting,
        SpoolEvent::Finished => EventKind::Finished,
        SpoolEvent::Failed => EventKind::Failed,
        SpoolEvent::SessionEnd => EventKind::SessionEnd,
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn to_saved(session: &Session) -> SavedSession {
    SavedSession {
        id: text(&session.id),
        turn_started: session.turn_started,
        last: session.last,
        notice: session.notice.as_ref().map(|n| SavedNotice {
            id: n.id,
            at: n.at,
            source: saved_source(n.source),
            kind: saved_kind(n.kind),
            repo: text(&n.repo),
            took: n.took,
            text: text(&n.text),
        }),
    }
}

fn from_saved(saved: SavedSession) -> Session {
    Session {
        id: saved.id.into_bytes(),
        turn_started: saved.turn_started,
        last: saved.last,
        notice: saved.notice.map(|n| Notice {
            id: n.id,
            at: n.at,
            source: live_source(n.source),
            kind: live_kind(n.kind),
            repo: n.repo.into_bytes(),
            took: n.took,
            text: n.text.into_bytes(),
        }),
    }
}

fn read_saved(dir: &Path) -> Result<Saved> {
    let Some(bytes) = read_at_most(&dir.join(STATE_FILE), MAX_STATE)? else {
        anyhow::bail!("{STATE_FILE} is bigger than {MAX_STATE} bytes");
    };
    Ok(serde_json::from_slice(&bytes)?)
}

impl TerminalSessions {
    /// A missing file is a first start. A damaged file loses the notices, and says why.
    pub fn load(dir: &Path) -> (TerminalSessions, Option<String>) {
        let first_start = !dir.join(STATE_FILE).exists();
        let (saved, problem) = match read_saved(dir) {
            Ok(saved) => (saved, None),
            Err(_) if first_start => (Saved::default(), None),
            Err(e) => (
                Saved::default(),
                Some(format!("{STATE_FILE} is damaged: {e:#}")),
            ),
        };
        let sessions = TerminalSessions {
            table: saved.sessions.into_iter().map(from_saved).collect(),
            last_id: saved.last_id,
            dir: dir.to_owned(),
        };
        (sessions, problem)
    }

    pub fn save(&self) -> Result<()> {
        let saved = Saved {
            last_id: self.last_id,
            sessions: self.table.iter().map(to_saved).collect(),
        };
        write_private(&self.dir, STATE_FILE, &serde_json::to_string(&saved)?)
    }

    /// Never less than the Unix time, so the ids after a wipe of the data folder never
    /// repeat the ids that the addon already showed.
    fn next_id(&mut self, now: u32) -> u32 {
        self.last_id = self.last_id.saturating_add(1).max(now);
        self.last_id
    }

    pub fn apply(&mut self, file: &SpoolFile, now: u32) {
        let event = Event {
            session: file.session.as_bytes().to_vec(),
            kind: event_kind(file.event),
            source: live_source(file.source),
            repo: notice_text(file.repo.as_bytes(), MAX_REPO),
            text: notice_text(file.text.as_bytes(), MAX_TEXT),
            id: self.next_id(now),
        };
        self.table = apply_event(&self.table, &event, now);
    }

    /// Returns whether a session or a turn ended.
    pub fn expire(&mut self, now: u32) -> bool {
        let before = (self.table.len(), self.busy());
        self.table = expire(&self.table, now);
        before != (self.table.len(), self.busy())
    }

    fn busy(&self) -> u32 {
        let running = self.table.iter().filter(|s| s.turn_started != 0).count();
        u32::try_from(running).unwrap_or(u32::MAX)
    }

    /// The notices for the live file: the newest 20, oldest first.
    pub fn notices(&self) -> Notices {
        let mut list: Vec<Notice> = self
            .table
            .iter()
            .filter_map(|s| s.notice.as_ref())
            .map(copy_notice)
            .collect();
        list.sort_by_key(|n| n.id);
        Notices {
            busy: self.busy(),
            open: u32::try_from(self.table.len()).unwrap_or(u32::MAX),
            list: prepare_notices(&list),
        }
    }
}

fn copy_notice(n: &Notice) -> Notice {
    Notice {
        id: n.id,
        at: n.at,
        source: n.source,
        kind: n.kind,
        repo: n.repo.clone(),
        took: n.took,
        text: n.text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::sessions::{MAX_SESSIONS, SESSION_SECONDS, TURN_SECONDS};

    const NOW: u32 = 1_790_300_000;

    fn file(session: &str, event: SpoolEvent, text: &str) -> SpoolFile {
        SpoolFile {
            v: 1,
            source: Source::Claude,
            event,
            session: session.into(),
            repo: "gnomish-relay".into(),
            text: text.into(),
        }
    }

    fn fresh() -> (tempfile::TempDir, TerminalSessions) {
        let dir = tempfile::tempdir().unwrap();
        let (sessions, problem) = TerminalSessions::load(dir.path());
        assert_eq!(problem, None);
        (dir, sessions)
    }

    fn kinds(sessions: &TerminalSessions) -> Vec<NoticeKind> {
        sessions.notices().list.iter().map(|n| n.kind).collect()
    }

    #[test]
    fn each_event_changes_the_session_as_the_table_of_the_spec() {
        let (_dir, mut sessions) = fresh();
        sessions.apply(&file("s1", SpoolEvent::SessionStart, ""), NOW);
        assert_eq!((sessions.notices().open, sessions.notices().busy), (1, 0));

        sessions.apply(&file("s1", SpoolEvent::TurnStart, ""), NOW + 1);
        assert_eq!(sessions.notices().busy, 1);

        sessions.apply(&file("s1", SpoolEvent::Waiting, "Allow Bash?"), NOW + 2);
        assert_eq!(kinds(&sessions), [NoticeKind::Waiting]);
        assert_eq!(sessions.notices().busy, 1, "the turn still runs");

        sessions.apply(&file("s1", SpoolEvent::Finished, "Done."), NOW + 241);
        let notices = sessions.notices();
        assert_eq!(kinds(&sessions), [NoticeKind::Finished]);
        assert_eq!((notices.list[0].took, notices.busy), (240, 0));

        sessions.apply(&file("s1", SpoolEvent::TurnStart, ""), NOW + 300);
        assert!(kinds(&sessions).is_empty(), "the user is at the terminal");

        sessions.apply(&file("s1", SpoolEvent::Failed, "overloaded"), NOW + 360);
        assert_eq!(sessions.notices().list[0].took, 60);

        sessions.apply(&file("s1", SpoolEvent::SessionEnd, ""), NOW + 400);
        assert_eq!(sessions.notices().open, 0);
        assert!(kinds(&sessions).is_empty());
    }

    #[test]
    fn a_session_start_removes_the_notice_of_its_session() {
        let (_dir, mut sessions) = fresh();
        sessions.apply(&file("s1", SpoolEvent::Finished, "Done."), NOW);
        sessions.apply(&file("s1", SpoolEvent::SessionStart, ""), NOW + 5);
        assert!(kinds(&sessions).is_empty());
    }

    #[test]
    fn a_finish_after_a_restart_of_the_bridge_took_zero() {
        let (_dir, mut sessions) = fresh();
        sessions.apply(&file("s1", SpoolEvent::Finished, "Done."), NOW);
        assert_eq!(sessions.notices().list[0].took, 0);
    }

    #[test]
    fn the_text_and_the_repo_are_cut_and_escaped() {
        let (_dir, mut sessions) = fresh();
        let mut event = file("s1", SpoolEvent::Waiting, "a|b\u{202E}c");
        event.repo = "r".repeat(100);
        sessions.apply(&event, NOW);
        let notice = &sessions.notices().list[0];
        assert_eq!(notice.text, b"a||bc");
        assert_eq!(notice.repo.len(), MAX_REPO);
    }

    #[test]
    fn notice_ids_grow_and_never_fall_below_the_time() {
        let (_dir, mut sessions) = fresh();
        sessions.apply(&file("s1", SpoolEvent::Waiting, "x"), NOW);
        sessions.apply(&file("s2", SpoolEvent::Waiting, "y"), NOW);
        let ids: Vec<u32> = sessions.notices().list.iter().map(|n| n.id).collect();
        assert_eq!(ids, [NOW, NOW + 1]);
    }

    #[test]
    fn the_table_and_the_counter_survive_a_restart() {
        let (dir, mut sessions) = fresh();
        sessions.apply(&file("s1", SpoolEvent::TurnStart, ""), NOW);
        sessions.apply(&file("s2", SpoolEvent::Waiting, "Allow Bash?"), NOW + 1);
        sessions.save().unwrap();

        let (mut again, problem) = TerminalSessions::load(dir.path());
        again.apply(&file("s3", SpoolEvent::Waiting, "z"), NOW);

        assert_eq!(problem, None);
        let notices = again.notices();
        assert_eq!((notices.open, notices.busy), (3, 1));
        assert_eq!(notices.list[0].text, b"Allow Bash?");
        assert_eq!(notices.list[1].id, NOW + 2);
    }

    #[test]
    fn a_damaged_file_starts_empty_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(STATE_FILE), b"{").unwrap();
        let (sessions, problem) = TerminalSessions::load(dir.path());
        assert!(problem.unwrap().contains("damaged"));
        assert_eq!(sessions.notices().open, 0);
    }

    #[test]
    fn an_old_turn_ends_and_an_old_session_goes() {
        let (_dir, mut sessions) = fresh();
        sessions.apply(&file("s1", SpoolEvent::TurnStart, ""), NOW);
        assert!(!sessions.expire(NOW + TURN_SECONDS));
        assert!(sessions.expire(NOW + TURN_SECONDS + 1));
        assert_eq!((sessions.notices().open, sessions.notices().busy), (1, 0));
        assert!(sessions.expire(NOW + SESSION_SECONDS + 1));
        assert_eq!(sessions.notices().open, 0);
    }

    #[test]
    fn the_live_file_holds_the_newest_20_notices_of_at_most_32_sessions() {
        let (_dir, mut sessions) = fresh();
        for n in 0..40u32 {
            sessions.apply(&file(&format!("s{n}"), SpoolEvent::Waiting, "x"), NOW + n);
        }
        let notices = sessions.notices();
        assert_eq!(notices.open as usize, MAX_SESSIONS);
        assert_eq!(notices.list.len(), 20);
        assert_eq!(notices.list[19].id, NOW + 39);
    }
}
