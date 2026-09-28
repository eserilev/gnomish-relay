//! The terminal sessions that the hooks report, each with at most one notice
//! (SPEC.md 10.3). S41 proves `apply_event`.

use crate::ascii::{bytes_equal, copy_bytes};
use crate::live::{Notice, NoticeKind, Source};

pub const MAX_SESSIONS: usize = 32;
/// A turn with no event for this long ends: the agent crashed or the user left.
pub const TURN_SECONDS: u32 = 1_800;
/// A session with no event for 12 hours ends: its terminal closed with no `SessionEnd`.
pub const SESSION_SECONDS: u32 = 43_200;

/// `turn_started` is 0 while no turn runs. `last` is the time of the latest event.
pub struct Session {
    pub id: Vec<u8>,
    pub turn_started: u32,
    pub last: u32,
    pub notice: Option<Notice>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    SessionStart,
    TurnStart,
    Waiting,
    Finished,
    Failed,
    SessionEnd,
}

/// One checked spool file. `repo` and `text` are outputs of `notice_text`, and `id`
/// is the id of the notice that the event makes.
pub struct Event {
    pub session: Vec<u8>,
    pub kind: EventKind,
    pub source: Source,
    pub repo: Vec<u8>,
    pub text: Vec<u8>,
    pub id: u32,
}

/// The index of the session, or `table.len()` for a new one.
fn find_session(table: &[Session], id: &[u8]) -> usize {
    let mut i = 0;
    while i < table.len() {
        if bytes_equal(&table[i].id, id) {
            return i;
        }
        i += 1;
    }
    table.len()
}

fn copy_notice(entry: &Notice) -> Notice {
    Notice {
        id: entry.id,
        at: entry.at,
        source: entry.source,
        kind: entry.kind,
        repo: copy_bytes(&entry.repo),
        took: entry.took,
        text: copy_bytes(&entry.text),
    }
}

fn copy_optional_notice(slot: &Option<Notice>) -> Option<Notice> {
    match slot {
        Some(n) => Some(copy_notice(n)),
        None => None,
    }
}

fn copy_session(session: &Session) -> Session {
    Session {
        id: copy_bytes(&session.id),
        turn_started: session.turn_started,
        last: session.last,
        notice: copy_optional_notice(&session.notice),
    }
}

/// At least 1 second, so 0 always means "no start seen".
fn took(started: u32, now: u32) -> u32 {
    if started == 0 {
        0
    } else if now <= started {
        1
    } else {
        now - started
    }
}

fn new_notice(event: &Event, kind: NoticeKind, took: u32, now: u32) -> Notice {
    Notice {
        id: event.id,
        at: now,
        source: event.source,
        kind,
        repo: copy_bytes(&event.repo),
        took,
        text: copy_bytes(&event.text),
    }
}

/// The session after `event`. `started` is its running turn, or 0.
fn next_session(started: u32, event: &Event, now: u32) -> Session {
    let (turn_started, latest) = match event.kind {
        EventKind::SessionStart | EventKind::SessionEnd => (0, None),
        EventKind::TurnStart => (now, None),
        EventKind::Waiting => (
            started,
            Some(new_notice(event, NoticeKind::Waiting, 0, now)),
        ),
        EventKind::Finished => (
            0,
            Some(new_notice(
                event,
                NoticeKind::Finished,
                took(started, now),
                now,
            )),
        ),
        EventKind::Failed => (
            0,
            Some(new_notice(
                event,
                NoticeKind::Failed,
                took(started, now),
                now,
            )),
        ),
    };
    Session {
        id: copy_bytes(&event.session),
        turn_started,
        last: now,
        notice: latest,
    }
}

fn started_at(table: &[Session], at: usize) -> u32 {
    if at < table.len() {
        table[at].turn_started
    } else {
        0
    }
}

fn notice_time(session: &Session) -> u32 {
    match &session.notice {
        Some(n) => n.at,
        None => 0,
    }
}

fn has_notice(session: &Session) -> bool {
    match &session.notice {
        Some(_) => true,
        None => false,
    }
}

/// True when `a` goes before `b` in a full table: a session with no notice goes
/// first, then the older one.
fn goes_before(a: &Session, b: &Session) -> bool {
    if has_notice(a) {
        has_notice(b) && notice_time(a) < notice_time(b)
    } else {
        has_notice(b) || a.last < b.last
    }
}

/// The session that a new session replaces in a full table (SPEC.md 10.3).
fn replaced_in_full_table(table: &[Session]) -> usize {
    let mut best = 0;
    let mut i = 1;
    while i < table.len() {
        if goes_before(&table[i], &table[best]) {
            best = i;
        }
        i += 1;
    }
    best
}

/// Where the session of an event goes: its own place, the end, or the place of
/// the session that it replaces.
fn target(table: &[Session], at: usize) -> usize {
    if at < table.len() || table.len() < MAX_SESSIONS {
        at
    } else {
        replaced_in_full_table(table)
    }
}

/// A copy of `table` without the session at `at`, and with `session` at the end.
fn with_session(table: &[Session], at: usize, session: Session) -> Vec<Session> {
    let mut out = without_session(table, at);
    out.push(session);
    out
}

/// A copy of `table` without the session at `at`. An `at` past the end removes nothing.
fn without_session(table: &[Session], at: usize) -> Vec<Session> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < table.len() {
        if i != at {
            out.push(copy_session(&table[i]));
        }
        i += 1;
    }
    out
}

fn is_end(kind: EventKind) -> bool {
    match kind {
        EventKind::SessionEnd => true,
        _ => false,
    }
}

/// The table after one event (SPEC.md 10.3). The session of the event moves to the end.
#[must_use]
pub fn apply_event(table: &[Session], event: &Event, now: u32) -> Vec<Session> {
    let at = find_session(table, &event.session);
    if is_end(event.kind) {
        return without_session(table, at);
    }
    let next = next_session(started_at(table, at), event, now);
    with_session(table, target(table, at), next)
}

fn is_expired(session: &Session, now: u32) -> bool {
    now > session.last && now - session.last > SESSION_SECONDS
}

fn turn_ended(session: &Session, now: u32) -> bool {
    now > session.last && now - session.last > TURN_SECONDS
}

/// The table at `now`: an old session goes, and an old turn ends. Its notice stays.
#[must_use]
pub fn expire(table: &[Session], now: u32) -> Vec<Session> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < table.len() {
        if !is_expired(&table[i], now) {
            let mut kept = copy_session(&table[i]);
            if turn_ended(&table[i], now) {
                kept.turn_started = 0;
            }
            out.push(kept);
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(session: &[u8], kind: EventKind, id: u32) -> Event {
        Event {
            session: session.to_vec(),
            kind,
            source: Source::Claude,
            repo: b"gnomish-relay".to_vec(),
            text: b"Done.".to_vec(),
            id,
        }
    }

    fn kind_of(session: &Session) -> Option<NoticeKind> {
        session.notice.as_ref().map(|n| n.kind)
    }

    fn apply_all(events: &[(&[u8], EventKind, u32)]) -> Vec<Session> {
        let mut table = Vec::new();
        for ((session, kind, now), id) in events.iter().zip(1..) {
            table = apply_event(&table, &event(session, *kind, id), *now);
        }
        table
    }

    #[test]
    fn a_finished_turn_leaves_a_notice_with_its_length() {
        let table = apply_all(&[
            (b"s1", EventKind::TurnStart, 100),
            (b"s1", EventKind::Finished, 340),
        ]);
        assert_eq!(table.len(), 1);
        let notice = table[0].notice.as_ref().unwrap();
        assert_eq!(
            (notice.kind, notice.took, notice.at),
            (NoticeKind::Finished, 240, 340)
        );
        assert_eq!(table[0].turn_started, 0);
    }

    #[test]
    fn a_turn_start_removes_the_notice_of_its_session() {
        let table = apply_all(&[
            (b"s1", EventKind::Waiting, 100),
            (b"s1", EventKind::TurnStart, 120),
        ]);
        assert_eq!(kind_of(&table[0]), None);
        assert_eq!(table[0].turn_started, 120);
    }

    #[test]
    fn waiting_keeps_the_turn_running() {
        let table = apply_all(&[
            (b"s1", EventKind::TurnStart, 100),
            (b"s1", EventKind::Waiting, 130),
        ]);
        assert_eq!(kind_of(&table[0]), Some(NoticeKind::Waiting));
        assert_eq!(table[0].turn_started, 100);
    }

    #[test]
    fn a_finish_with_no_start_took_zero_and_a_quick_one_took_one_second() {
        let table = apply_all(&[(b"s1", EventKind::Failed, 100)]);
        assert_eq!(table[0].notice.as_ref().unwrap().took, 0);
        let table = apply_all(&[
            (b"s2", EventKind::TurnStart, 100),
            (b"s2", EventKind::Finished, 100),
        ]);
        assert_eq!(table[0].notice.as_ref().unwrap().took, 1);
    }

    #[test]
    fn a_session_start_clears_the_notice_and_a_session_end_removes_the_session() {
        let table = apply_all(&[
            (b"s1", EventKind::Finished, 100),
            (b"s2", EventKind::Finished, 110),
            (b"s1", EventKind::SessionStart, 120),
        ]);
        assert_eq!(table.len(), 2);
        assert_eq!(kind_of(&table[1]), None);
        let table = apply_event(&table, &event(b"s2", EventKind::SessionEnd, 9), 130);
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].id, b"s1");
    }

    #[test]
    fn the_end_of_an_unknown_session_changes_nothing() {
        let table = apply_all(&[(b"s1", EventKind::Finished, 100)]);
        let table = apply_event(&table, &event(b"s9", EventKind::SessionEnd, 2), 110);
        assert_eq!(table.len(), 1);
        assert_eq!(kind_of(&table[0]), Some(NoticeKind::Finished));
    }

    fn full_table(with_notices: bool) -> Vec<Session> {
        let mut table = Vec::new();
        for n in 0..u32::try_from(MAX_SESSIONS).unwrap() {
            let kind = if with_notices || n % 2 == 0 {
                EventKind::Finished
            } else {
                EventKind::TurnStart
            };
            table = apply_event(
                &table,
                &event(format!("s{n}").as_bytes(), kind, n + 1),
                1000 + n,
            );
        }
        table
    }

    #[test]
    fn a_new_session_in_a_full_table_replaces_the_oldest_session_with_no_notice() {
        let table = full_table(false);
        let table = apply_event(&table, &event(b"new", EventKind::TurnStart, 99), 5000);
        assert_eq!(table.len(), MAX_SESSIONS);
        assert!(!table.iter().any(|s| s.id == b"s1"));
        assert!(table.iter().any(|s| s.id == b"s3"));
        assert_eq!(table.iter().filter(|s| s.notice.is_some()).count(), 16);
    }

    #[test]
    fn a_new_session_in_a_table_full_of_notices_replaces_the_oldest_notice() {
        let table = full_table(true);
        let table = apply_event(&table, &event(b"new", EventKind::Waiting, 99), 5000);
        assert_eq!(table.len(), MAX_SESSIONS);
        assert!(!table.iter().any(|s| s.id == b"s0"));
        assert!(table.iter().any(|s| s.id == b"s1"));
    }

    #[test]
    fn an_old_turn_ends_and_an_old_session_goes() {
        let table = apply_all(&[
            (b"s1", EventKind::TurnStart, 100),
            (b"s2", EventKind::TurnStart, 100 + SESSION_SECONDS),
            (b"s2", EventKind::Waiting, 100 + SESSION_SECONDS),
        ]);
        assert_eq!(table[1].turn_started, 100 + SESSION_SECONDS);
        let table = expire(&table, 102 + SESSION_SECONDS);
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].id, b"s2");
        let table = expire(&table, 102 + SESSION_SECONDS + TURN_SECONDS);
        assert_eq!(table[0].turn_started, 0);
        assert_eq!(kind_of(&table[0]), Some(NoticeKind::Waiting));
    }
}
