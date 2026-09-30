//! The terminal sessions that the hooks report, each with at most one notice (SPEC.md
//! 10.3). The proved `apply_event` and `expire` of `protocol` change the table. The table
//! and the id counter live in `notices.json`, so a restart keeps them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use protocol::live::{Notice, NoticeKind, Notices, Source as LiveSource, prepare_notices};
use protocol::notice::{MAX_REPO, MAX_TEXT, notice_text};
use protocol::sessions::{Event, EventKind, MAX_SESSIONS, Session, apply_event, expire};
use serde::{Deserialize, Serialize};

use crate::fs_safe::{read_at_most, write_private};
use crate::spool::{Source, SpoolEvent, SpoolFile, is_session_id};

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

/// Ids run ahead of the Unix time only when events come faster than one a second.
const FUTURE_SLACK: u32 = 86_400;

fn not_far_ahead(value: u32, now: u32) -> bool {
    value <= now.saturating_add(FUTURE_SLACK)
}

/// `notice_text` doubles each `|`, so a text that it made comes back the same from one
/// more pass over its single pipes. Any other text is not one that it made.
fn made_by_notice_text(text: &str, max: usize) -> bool {
    notice_text(&single_pipes(text.as_bytes()), max) == text.as_bytes()
}

fn single_pipes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        out.push(bytes[i]);
        let doubled = bytes[i] == b'|' && bytes.get(i + 1) == Some(&b'|');
        i += if doubled { 2 } else { 1 };
    }
    out
}

fn notice_is_sound(notice: &SavedNotice, now: u32) -> bool {
    not_far_ahead(notice.id, now)
        && made_by_notice_text(&notice.repo, MAX_REPO)
        && made_by_notice_text(&notice.text, MAX_TEXT)
}

/// Any local process of the user can change the file. S40 and S41 hold for a loaded
/// session only when it passes the checks of a new event.
fn session_is_sound(session: &SavedSession, now: u32) -> bool {
    is_session_id(&session.id)
        && not_far_ahead(session.last, now)
        && not_far_ahead(session.turn_started, now)
        && session
            .notice
            .as_ref()
            .is_none_or(|n| notice_is_sound(n, now))
}

/// The sound sessions with unique ids, at most `MAX_SESSIONS` of them, the newest kept.
fn sound_sessions(saved: Vec<SavedSession>, now: u32) -> Vec<SavedSession> {
    let mut sound: Vec<SavedSession> = saved
        .into_iter()
        .filter(|s| session_is_sound(s, now))
        .collect();
    sound.sort_by_key(|s| std::cmp::Reverse(s.last));
    let mut seen = HashSet::new();
    sound.retain(|s| seen.insert(s.id.clone()));
    sound.truncate(MAX_SESSIONS);
    sound.reverse();
    sound
}

/// A new id is above every loaded one, so it never repeats an id that the addon showed.
fn sound_last_id(saved: u32, sessions: &[SavedSession], now: u32) -> u32 {
    let last = if not_far_ahead(saved, now) { saved } else { 0 };
    let notice_ids = sessions
        .iter()
        .filter_map(|s| s.notice.as_ref())
        .map(|n| n.id);
    notice_ids.fold(last, u32::max)
}

impl TerminalSessions {
    /// A missing file is a first start. A damaged file loses the notices, and says why.
    pub fn load(dir: &Path, now: u32) -> (TerminalSessions, Option<String>) {
        let first_start = !dir.join(STATE_FILE).exists();
        let (saved, mut problem) = match read_saved(dir) {
            Ok(saved) => (saved, None),
            Err(_) if first_start => (Saved::default(), None),
            Err(e) => (
                Saved::default(),
                Some(format!("{STATE_FILE} is damaged: {e:#}")),
            ),
        };
        let count = saved.sessions.len();
        let sessions = sound_sessions(saved.sessions, now);
        let dropped = count - sessions.len();
        if dropped > 0 {
            problem = Some(format!("{STATE_FILE}: dropped {dropped} damaged sessions"));
        }
        if !not_far_ahead(saved.last_id, now) {
            problem = Some(format!("{STATE_FILE}: dropped a last id from the future"));
        }
        let terminal = TerminalSessions {
            last_id: sound_last_id(saved.last_id, &sessions, now),
            table: sessions.into_iter().map(from_saved).collect(),
            dir: dir.to_owned(),
        };
        (terminal, problem)
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
    use protocol::sessions::{SESSION_SECONDS, TURN_SECONDS};

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
        let (sessions, problem) = TerminalSessions::load(dir.path(), NOW);
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

        let (mut again, problem) = TerminalSessions::load(dir.path(), NOW);
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
        let (sessions, problem) = TerminalSessions::load(dir.path(), NOW);
        assert!(problem.unwrap().contains("damaged"));
        assert_eq!(sessions.notices().open, 0);
    }

    fn saved_notice(id: u32, text: &str) -> serde_json::Value {
        serde_json::json!({ "id": id, "at": NOW, "source": "claude", "kind": "waiting",
                            "repo": "r", "took": 0, "text": text })
    }

    fn saved_session(id: &str, last: u32, notice: &serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "id": id, "turn_started": 0, "last": last, "notice": notice })
    }

    fn load_saved(
        last_id: u32,
        sessions: &[serde_json::Value],
    ) -> (TerminalSessions, Option<String>) {
        let dir = tempfile::tempdir().unwrap();
        let saved = serde_json::json!({ "last_id": last_id, "sessions": sessions });
        std::fs::write(dir.path().join(STATE_FILE), saved.to_string()).unwrap();
        TerminalSessions::load(dir.path(), NOW)
    }

    #[test]
    fn a_saved_pipe_stays_doubled_once_after_each_restart() {
        let (dir, mut sessions) = fresh();
        sessions.apply(&file("s1", SpoolEvent::Waiting, "a|b"), NOW);
        sessions.save().unwrap();

        let (again, problem) = TerminalSessions::load(dir.path(), NOW);
        again.save().unwrap();
        let (third, _) = TerminalSessions::load(dir.path(), NOW);

        assert_eq!(problem, None);
        assert_eq!(third.notices().list[0].text, b"a||b");
    }

    #[test]
    fn a_loaded_notice_that_notice_text_did_not_make_is_dropped() {
        let (sessions, problem) = load_saved(
            NOW,
            &[
                saved_session("s1", NOW, &saved_notice(NOW - 3, "a|Hitem:1|h[x]|h")),
                saved_session("s2", NOW, &saved_notice(NOW - 2, "a\u{202E}b")),
                saved_session("s3", NOW, &saved_notice(NOW - 1, "tab\there")),
                saved_session("bad id!", NOW, &serde_json::Value::Null),
                saved_session("s4", NOW, &saved_notice(NOW, "fine || text")),
            ],
        );

        let notices = sessions.notices();
        assert_eq!(notices.open, 1);
        assert_eq!(notices.list[0].text, b"fine || text");
        assert!(problem.unwrap().contains("dropped 4 damaged sessions"));
    }

    #[test]
    fn a_loaded_table_that_does_not_fit_is_cut_to_32_sessions() {
        let mut saved: Vec<serde_json::Value> = (0..40u32)
            .map(|n| saved_session(&format!("s{n}"), NOW - 100 + n, &serde_json::Value::Null))
            .collect();
        saved.push(saved_session("s39", NOW - 200, &serde_json::Value::Null));

        let (sessions, problem) = load_saved(NOW, &saved);

        assert_eq!(sessions.notices().open as usize, MAX_SESSIONS);
        assert!(sessions.table.iter().all(|s| s.last >= NOW - 100 + 8));
        assert!(problem.is_some());
    }

    #[test]
    fn a_saved_id_far_in_the_future_is_dropped_so_new_ids_still_grow() {
        let (mut sessions, problem) = load_saved(
            u32::MAX,
            &[saved_session("s1", NOW, &saved_notice(u32::MAX, "x"))],
        );

        sessions.apply(&file("s2", SpoolEvent::Waiting, "y"), NOW);
        sessions.apply(&file("s3", SpoolEvent::Waiting, "z"), NOW);

        let ids: Vec<u32> = sessions.notices().list.iter().map(|n| n.id).collect();
        assert_eq!(ids, [NOW, NOW + 1]);
        assert!(problem.is_some());
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
