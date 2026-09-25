//! The saved sessions of Claude Code (SPEC.md 9.6): one JSONL file per session in
//! `<config dir>/projects/<folder>/<session id>.jsonl`. The rules follow `listSessions`,
//! `getSessionMessages`, and `forkSession` of the Claude Agent SDK, with no model call.
//!
//! The files are untrusted input: every read has a size limit, and a bad line is skipped.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};

use crate::agent::{MAX_PROMPT, MAX_REPLY, SessionInfo, exchange_text};
use crate::process::cut;

/// The SDK reads this much from each end of a file for the list.
const LIST_READ: u64 = 64 * 1024;
/// Only the newest files are read. The relay keeps 30 sessions after its folder check.
const MAX_LISTED_FILES: usize = 60;
/// The last exchange is near the end of the file.
const ATTACH_READ: u64 = 8 * 1024 * 1024;
/// A copy needs the whole file.
const MAX_FORK: u64 = 64 * 1024 * 1024;
const MAX_TITLE: usize = 200;
/// The entry types of the transcript. All other types are metadata.
const TRANSCRIPT: [&str; 5] = ["user", "assistant", "attachment", "system", "progress"];

/// `$CLAUDE_CONFIG_DIR/projects`, or `~/.claude/projects`. The agent sees
/// `CLAUDE_CONFIG_DIR` only when its `env` list names it, so the bridge does the same.
pub fn projects_dir(env: &[String], home: &Path) -> PathBuf {
    let named = env.iter().any(|name| name == "CLAUDE_CONFIG_DIR");
    let config = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) if named && !dir.is_empty() => PathBuf::from(dir),
        _ => home.join(".claude"),
    };
    config.join("projects")
}

pub fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    let lengths: Vec<usize> = groups.iter().map(|g| g.len()).collect();
    lengths == [8, 4, 4, 4, 12]
        && groups
            .iter()
            .all(|g| g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The file of a session, in any project folder. A session id is a UUID, so it names
/// one file.
pub fn find(projects: &Path, id: &str) -> Option<PathBuf> {
    if !is_uuid(id) {
        return None;
    }
    let name = format!("{id}.jsonl");
    project_folders(projects)
        .into_iter()
        .map(|folder| folder.join(&name))
        .find(|file| fs::symlink_metadata(file).is_ok_and(|m| m.is_file() && m.len() > 0))
}

fn project_folders(projects: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(projects) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect()
}

struct SessionFile {
    id: String,
    path: PathBuf,
    /// Unix seconds.
    modified: u32,
}

fn session_files(folder: &Path) -> Vec<SessionFile> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".jsonl").filter(|id| is_uuid(id)) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() || meta.len() == 0 {
            continue;
        }
        files.push(SessionFile {
            id: id.to_owned(),
            path: entry.path(),
            modified: unix_seconds(&meta),
        });
    }
    files
}

fn unix_seconds(meta: &fs::Metadata) -> u32 {
    let since = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    since.map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
}

/// The newest sessions of every project folder, newest first.
pub fn list(projects: &Path) -> Vec<SessionInfo> {
    let mut files: Vec<SessionFile> = project_folders(projects)
        .iter()
        .flat_map(|folder| session_files(folder))
        .collect();
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.id.cmp(&a.id)));
    files.truncate(MAX_LISTED_FILES);
    files.iter().filter_map(read_listed).collect()
}

fn read_listed(file: &SessionFile) -> Option<SessionInfo> {
    let (head, tail) = head_and_tail(&file.path).ok()?;
    let folder = file.path.parent()?;
    let is_continued = |next: &str| {
        let path = folder.join(format!("{next}.jsonl"));
        is_uuid(next) && read_start(&path, LIST_READ).is_ok_and(|t| t.contains("\"parentUuid\":"))
    };
    if continued_elsewhere(&tail, is_continued) {
        return None;
    }
    let sidecar = || sidecar_title(&folder.join(&file.id));
    let mut info = read_info(&head, &tail, sidecar)?;
    info.id.clone_from(&file.id);
    info.updated = file.modified;
    Some(info)
}

fn read_start(path: &Path, max: u64) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?.take(max).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The last `max` bytes, from the start of a line.
fn read_end(path: &Path, max: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let start = size.saturating_sub(max);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(max).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Ok(text);
    }
    // The first line starts before the read.
    Ok(text
        .split_once('\n')
        .map_or("", |(_, rest)| rest)
        .to_owned())
}

fn head_and_tail(path: &Path) -> std::io::Result<(String, String)> {
    Ok((read_start(path, LIST_READ)?, read_end(path, LIST_READ)?))
}

/// A `/rename` from another program can live next to the file, in
/// `<session id>/custom-title.json`.
fn sidecar_title(dir: &Path) -> Option<String> {
    let text = read_start(&dir.join("custom-title.json"), LIST_READ).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let title = value.get("customTitle")?.as_str()?.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

/// A session that went on in another file stays out of the list, as in the SDK: the
/// tail names the next session after the last real turn, and that file exists.
pub fn continued_elsewhere(tail: &str, next_exists: impl Fn(&str) -> bool) -> bool {
    for line in tail.lines().rev() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match text_of(&entry, "type") {
            Some("continued-in") => {
                return text_of(&entry, "continuedInSessionId").is_some_and(&next_exists);
            }
            Some("assistant") if ends_a_turn(&entry) => return false,
            Some("user") if matches!(prompt_of(&entry), Prompt::Text(_)) => return false,
            _ => {}
        }
    }
    false
}

fn ends_a_turn(entry: &Value) -> bool {
    let stop = entry
        .pointer("/message/stop_reason")
        .is_some_and(Value::is_string);
    stop && entry.get("isApiErrorMessage").and_then(Value::as_bool) != Some(true)
}

/// The list fields of one session from the first and the last 64 KiB of its file.
/// `sidecar` gives the title of `custom-title.json`, which counts only when the tail
/// has no title. The caller sets the id and the time.
pub fn read_info(
    head: &str,
    tail: &str,
    sidecar: impl FnOnce() -> Option<String>,
) -> Option<SessionInfo> {
    let first = head.lines().next().unwrap_or("");
    if first.contains("\"isSidechain\":true") || first.contains("\"isSidechain\": true") {
        return None;
    }
    let title = last_string(tail, "customTitle")
        .or_else(sidecar)
        .or_else(|| last_string(head, "customTitle"))
        .or_else(|| last_string(tail, "aiTitle"))
        .or_else(|| last_string(head, "aiTitle"))
        .or_else(|| last_string(tail, "lastPrompt"))
        .or_else(|| last_string(tail, "summary"))
        .or_else(|| first_prompt(head))?;
    let cwd = relocated_cwd(tail).or_else(|| first_string(head, "cwd"))?;
    Some(SessionInfo {
        id: String::new(),
        cwd,
        title: one_line(&title),
        updated: 0,
    })
}

fn relocated_cwd(tail: &str) -> Option<String> {
    tail.lines()
        .rev()
        .filter(|line| line.contains("\"type\":\"relocated\""))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|entry| text_of(&entry, "relocatedCwd").map(str::to_owned))
}

fn one_line(title: &str) -> String {
    let words: Vec<&str> = title.split_whitespace().collect();
    words.join(" ")
}

/// The first user prompt in the head, as the SDK finds it: a slash command counts only
/// when nothing else does.
fn first_prompt(head: &str) -> Option<String> {
    let mut command = None;
    for line in head.lines() {
        if !line.contains("\"type\":\"user\"")
            || line.contains("\"tool_result\"")
            || line.contains("\"isMeta\":true")
            || line.contains("\"isCompactSummary\":true")
        {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match prompt_of(&entry) {
            Prompt::Text(text) => return Some(text),
            Prompt::Command(name) => {
                command.get_or_insert(name);
            }
            Prompt::None => {}
        }
    }
    command
}

/// What one user entry means for a title or an exchange.
#[derive(Debug, PartialEq, Eq)]
pub enum Prompt {
    /// Words that the user typed, on one line, at most 200 characters.
    Text(String),
    /// A slash command, such as `/model`.
    Command(String),
    /// A tool result, a meta entry, or a note of Claude Code.
    None,
}

pub fn prompt_of(entry: &Value) -> Prompt {
    let meta = ["isMeta", "isCompactSummary"]
        .iter()
        .any(|key| entry.get(*key).and_then(Value::as_bool) == Some(true));
    if text_of(entry, "type") != Some("user") || meta {
        return Prompt::None;
    }
    let content = entry.pointer("/message/content").unwrap_or(&Value::Null);
    let texts: Vec<&str> = match content {
        Value::String(text) => vec![text.as_str()],
        Value::Array(blocks)
            if blocks
                .iter()
                .any(|b| text_of(b, "type") == Some("tool_result")) =>
        {
            return Prompt::None;
        }
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| text_of(b, "type") == Some("text"))
            .filter_map(|b| text_of(b, "text"))
            .collect(),
        _ => Vec::new(),
    };
    let mut command = None;
    for text in texts {
        match prompt_of_text(text) {
            Prompt::Text(text) => return Prompt::Text(text),
            Prompt::Command(name) => {
                command.get_or_insert(name);
            }
            Prompt::None => {}
        }
    }
    command.map_or(Prompt::None, Prompt::Command)
}

fn prompt_of_text(text: &str) -> Prompt {
    let text = text.replace('\n', " ");
    let text = text.trim();
    if let Some(name) = between(text, "<command-name>", "</command-name>") {
        return Prompt::Command(name.to_owned());
    }
    if let Some(input) = between(text, "<bash-input>", "</bash-input>") {
        return Prompt::Text(format!("! {}", input.trim()));
    }
    if text.is_empty() || starts_with_tag(text) || text.starts_with("[Request interrupted by user")
    {
        return Prompt::None;
    }
    if text.chars().count() <= MAX_TITLE {
        return Prompt::Text(text.to_owned());
    }
    let start: String = text.chars().take(MAX_TITLE).collect();
    Prompt::Text(format!("{}…", start.trim()))
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(&text[start..end])
}

/// Claude Code wraps its own notes in a tag, such as `<local-command-stdout>`.
fn starts_with_tag(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('<') else {
        return false;
    };
    let mut chars = rest.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_lowercase()) {
        return false;
    }
    let after = chars.find(|c| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-'));
    after.is_some_and(|c| c == '>' || c.is_whitespace())
}

/// The string value of the first `"key":"…"` in raw JSON text. The SDK reads the list
/// fields this way, so a cut line at either end of a read still counts.
pub fn first_string(text: &str, key: &str) -> Option<String> {
    let at = patterns(key)
        .iter()
        .filter_map(|p| text.find(p.as_str()).map(|at| at + p.len()))
        .min()?;
    string_at(text, at)
}

pub fn last_string(text: &str, key: &str) -> Option<String> {
    let at = patterns(key)
        .iter()
        .filter_map(|p| text.rfind(p.as_str()).map(|at| at + p.len()))
        .max()?;
    string_at(text, at)
}

fn patterns(key: &str) -> [String; 2] {
    [format!("\"{key}\":\""), format!("\"{key}\": \"")]
}

/// The JSON string that starts after its opening quote at `start`. Empty counts as none.
fn string_at(text: &str, start: usize) -> Option<String> {
    let rest = text.get(start..)?;
    let mut escaped = false;
    let end = rest.char_indices().find_map(|(i, c)| {
        let is_end = c == '"' && !escaped;
        escaped = c == '\\' && !escaped;
        is_end.then_some(i)
    })?;
    let value: String = serde_json::from_str(&format!("\"{}\"", &rest[..end])).ok()?;
    (!value.is_empty()).then_some(value)
}

fn text_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// The last exchange of a session file for an attach, read from its end.
pub fn read_last_exchange(path: &Path) -> Result<String, String> {
    let text = read_end(path, ATTACH_READ).map_err(|e| format!("Cannot read the session: {e}"))?;
    Ok(last_exchange(&text))
}

/// One transcript entry of a session file.
struct Entry {
    value: Value,
    uuid: String,
    parent: Option<String>,
}

impl Entry {
    fn kind(&self) -> Option<&str> {
        text_of(&self.value, "type")
    }

    fn is_message(&self) -> bool {
        matches!(self.kind(), Some("user" | "assistant"))
    }

    /// Not a subagent, a team member, or a meta entry.
    fn is_main(&self) -> bool {
        let flag = |key: &str| {
            self.value
                .get(key)
                .is_some_and(|v| v.as_bool() == Some(true))
        };
        !flag("isSidechain")
            && !flag("isMeta")
            && self.value.get("teamName").is_none_or(Value::is_null)
    }
}

fn transcript(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|v| text_of(v, "type").is_some_and(|t| TRANSCRIPT.contains(&t)))
        .filter_map(|value| {
            let uuid = text_of(&value, "uuid")?.to_owned();
            let parent = text_of(&value, "parentUuid").map(str::to_owned);
            Some(Entry {
                value,
                uuid,
                parent,
            })
        })
        .collect()
}

/// The prompt on the first line and the answer below, from the chain of the newest
/// leaf, as `getSessionMessages` builds it. Text of other branches stays out.
pub fn last_exchange(text: &str) -> String {
    let entries = transcript(text);
    let chain = main_chain(&entries);
    let mut answer_ids: Vec<usize> = Vec::new();
    let mut prompt = String::new();
    for &at in chain.iter().rev() {
        let entry = &entries[at];
        if entry.kind() == Some("assistant") {
            answer_ids.push(at);
            continue;
        }
        if let Prompt::Text(text) = prompt_of(&entry.value) {
            prompt = text;
            break;
        }
    }
    answer_ids.reverse();
    let answer = answer_text(&entries, &answer_ids);
    exchange_text(cut(&prompt, MAX_PROMPT), cut(&answer, MAX_REPLY))
}

/// Indexes of the chain from the root to the newest leaf.
fn main_chain(entries: &[Entry]) -> Vec<usize> {
    let by_uuid: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.uuid.as_str(), i))
        .collect();
    let parents: HashSet<&str> = entries.iter().filter_map(|e| e.parent.as_deref()).collect();
    let candidates: Vec<usize> = (0..entries.len())
        .filter(|&i| !parents.contains(entries[i].uuid.as_str()))
        .filter_map(|leaf| message_at_or_above(entries, &by_uuid, leaf))
        .collect();
    let main = candidates
        .iter()
        .copied()
        .filter(|&i| entries[i].is_main())
        .max();
    let Some(newest) = main.or_else(|| candidates.iter().copied().max()) else {
        return Vec::new();
    };
    let mut chain = walk_up(entries, &by_uuid, newest);
    chain.reverse();
    chain
}

/// The first user or assistant entry from `at` up.
fn message_at_or_above(
    entries: &[Entry],
    by_uuid: &HashMap<&str, usize>,
    at: usize,
) -> Option<usize> {
    walk_up(entries, by_uuid, at)
        .into_iter()
        .find(|&i| entries[i].is_message())
}

/// `at` and its parents, up to the root. A loop in the parents ends the walk.
fn walk_up(entries: &[Entry], by_uuid: &HashMap<&str, usize>, at: usize) -> Vec<usize> {
    let mut seen = HashSet::new();
    let mut chain = Vec::new();
    let mut next = Some(at);
    while let Some(i) = next.filter(|i| seen.insert(*i)) {
        chain.push(i);
        next = entries[i]
            .parent
            .as_deref()
            .and_then(|p| by_uuid.get(p).copied());
    }
    chain
}

/// Claude Code writes each block of one API message as its own entry, and a branch
/// can hold some of them. So the answer takes every entry of each message on the chain.
fn answer_text(entries: &[Entry], on_chain: &[usize]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut seen_messages: HashSet<&str> = HashSet::new();
    for &at in on_chain {
        let message_id = entries[at]
            .value
            .pointer("/message/id")
            .and_then(Value::as_str);
        let same_message: Vec<&Entry> = match message_id {
            Some(id) if !seen_messages.insert(id) => continue,
            Some(id) => entries
                .iter()
                .filter(|e| {
                    e.is_main()
                        && e.value.pointer("/message/id").and_then(Value::as_str) == Some(id)
                })
                .collect(),
            None => vec![&entries[at]],
        };
        parts.extend(same_message.iter().flat_map(|e| texts_of(&e.value)));
    }
    parts.join("\n\n")
}

fn texts_of(entry: &Value) -> Vec<&str> {
    let blocks = entry.pointer("/message/content").and_then(Value::as_array);
    blocks
        .into_iter()
        .flatten()
        .filter(|b| text_of(b, "type") == Some("text"))
        .filter_map(|b| text_of(b, "text"))
        .filter(|t| !t.trim().is_empty())
        .collect()
}

/// Copies a session into a new file next to it, as `forkSession` of the SDK does,
/// and returns the new session id.
pub fn fork(path: &Path, id: &str) -> Result<String, String> {
    let size = fs::metadata(path)
        .map_err(|e| format!("Cannot read the session: {e}"))?
        .len();
    if size > MAX_FORK {
        return Err("The session is too big to copy.".into());
    }
    let text = read_start(path, MAX_FORK).map_err(|e| format!("Cannot read the session: {e}"))?;
    let folder = path.parent().ok_or("The session has no folder.")?;
    let title = read_info(&text, &text, || sidecar_title(&folder.join(id))).map(|i| i.title);
    let new_id = new_uuid()?;
    let entries = fork_entries(&text, id, &new_id, title.as_deref(), &now_iso(), new_uuid)?;
    let mut lines = String::new();
    for entry in &entries {
        lines.push_str(&entry.to_string());
        lines.push('\n');
    }
    write_new_file(&folder.join(format!("{new_id}.jsonl")), lines.as_bytes())?;
    Ok(new_id)
}

/// The metadata that a copy keeps, from entries of the old session only.
#[derive(Default)]
struct Kept {
    replacements: Vec<Value>,
    atis: Option<String>,
    relocated: Option<String>,
    history_suppressed: bool,
}

fn kept_metadata(values: &[Value], old: &str) -> Kept {
    let mut kept = Kept::default();
    for value in values {
        let own = text_of(value, "sessionId") == Some(old);
        match text_of(value, "type") {
            Some("history-suppression") => kept.history_suppressed = true,
            Some("atis-latch") if own => {
                let atis = text_of(value, "atis")
                    .filter(|a| a.bytes().all(|b| (b'!'..=b'~').contains(&b)));
                if let Some(atis) = atis {
                    kept.atis = Some(atis.to_owned());
                }
            }
            Some("content-replacement") if own => {
                let list = value.get("replacements").and_then(Value::as_array);
                kept.replacements
                    .extend(list.into_iter().flatten().cloned());
            }
            Some("relocated") if own => {
                if let Some(cwd) = text_of(value, "relocatedCwd").filter(|c| !c.is_empty()) {
                    kept.relocated = Some(cwd.to_owned());
                }
            }
            _ => {}
        }
    }
    kept
}

/// The entries of the copy: every transcript entry of the main line gets a new uuid,
/// the new session id, and a `forkedFrom` note. Progress entries go, and their
/// children move up to the next parent. A new title ends the file.
pub fn fork_entries(
    text: &str,
    old: &str,
    new: &str,
    title: Option<&str>,
    now: &str,
    mut make_uuid: impl FnMut() -> Result<String, String>,
) -> Result<Vec<Value>, String> {
    let values: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let kept = kept_metadata(&values, old);
    let entries: Vec<Entry> = transcript(text)
        .into_iter()
        .filter(|e| e.value.get("isSidechain").and_then(Value::as_bool) != Some(true))
        .collect();
    let mut new_uuids: HashMap<String, String> = HashMap::new();
    for entry in &entries {
        new_uuids.insert(entry.uuid.clone(), make_uuid()?);
    }
    let by_uuid: HashMap<&str, &Entry> = entries.iter().map(|e| (e.uuid.as_str(), e)).collect();
    let copied: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.kind() != Some("progress"))
        .collect();
    if copied.is_empty() {
        return Err("The session has no messages to copy.".into());
    }
    let mut out = Vec::new();
    if kept.history_suppressed {
        out.push(json!({ "type": "history-suppression", "sessionId": new, "cause": "fork_inherit", "ts": now }));
    }
    for (n, entry) in copied.iter().enumerate() {
        let parent = copied_parent(entry, &by_uuid, &new_uuids);
        let is_last = n + 1 == copied.len();
        out.push(copied_entry(
            entry,
            old,
            new,
            parent,
            is_last.then_some(now),
            &new_uuids,
        ));
    }
    if !kept.replacements.is_empty() {
        out.push(json!({ "type": "content-replacement", "sessionId": new, "replacements": kept.replacements, "uuid": make_uuid()?, "timestamp": now }));
    }
    if let Some(atis) = kept.atis {
        out.push(json!({ "type": "atis-latch", "sessionId": new, "atis": atis }));
    }
    if let Some(cwd) = kept.relocated {
        out.push(json!({ "type": "relocated", "sessionId": new, "relocatedCwd": cwd }));
    }
    let title = format!("{} (fork)", title.unwrap_or("Forked session"));
    out.push(json!({ "type": "custom-title", "sessionId": new, "customTitle": title, "uuid": make_uuid()?, "timestamp": now }));
    Ok(out)
}

/// The new uuid of the first parent that is not a progress entry.
fn copied_parent(
    entry: &Entry,
    by_uuid: &HashMap<&str, &Entry>,
    new_uuids: &HashMap<String, String>,
) -> Value {
    let mut seen = HashSet::new();
    let mut next = entry.parent.as_deref();
    while let Some(uuid) = next {
        let Some(parent) = by_uuid.get(uuid) else {
            break;
        };
        if parent.kind() != Some("progress") || !seen.insert(uuid) {
            return new_uuids.get(uuid).map_or(Value::Null, |u| json!(u));
        }
        next = parent.parent.as_deref();
    }
    Value::Null
}

fn copied_entry(
    entry: &Entry,
    old: &str,
    new: &str,
    parent: Value,
    now: Option<&str>,
    new_uuids: &HashMap<String, String>,
) -> Value {
    let mut value = entry.value.clone();
    let Some(fields) = value.as_object_mut() else {
        return value;
    };
    let remap = |uuid: &Value| {
        uuid.as_str()
            .and_then(|u| new_uuids.get(u))
            .map_or(Value::Null, |u| json!(u))
    };
    if let Some(logical) = fields.get("logicalParentUuid").filter(|v| !v.is_null()) {
        let logical = remap(logical);
        fields.insert("logicalParentUuid".into(), logical);
    }
    if text_of(&entry.value, "subtype") == Some("model_refusal_fallback")
        && entry.kind() == Some("system")
    {
        fields.insert("neutralizedByFork".into(), json!(true));
    }
    if let Some(attachment) = fields.get_mut("attachment").and_then(Value::as_object_mut) {
        remap_attachment(attachment, new_uuids);
    }
    for key in [
        "teamName",
        "agentName",
        "sessionKind",
        "slug",
        "sourceToolAssistantUUID",
    ] {
        fields.remove(key);
    }
    fields.insert("uuid".into(), remap(&json!(entry.uuid)));
    fields.insert("parentUuid".into(), parent);
    fields.insert("sessionId".into(), json!(new));
    if let Some(now) = now {
        fields.insert("timestamp".into(), json!(now));
    }
    fields.insert("isSidechain".into(), json!(false));
    fields.insert(
        "forkedFrom".into(),
        json!({ "sessionId": old, "messageUuid": entry.uuid }),
    );
    value
}

fn remap_attachment(
    attachment: &mut serde_json::Map<String, Value>,
    new_uuids: &HashMap<String, String>,
) {
    match attachment.get("type").and_then(Value::as_str) {
        Some("deferred_tools_record") => {
            let Some(Value::Array(names)) = attachment.get("nameOnlyAnnouncements") else {
                return;
            };
            let mapped: Vec<Value> = names
                .iter()
                .filter_map(|n| n.as_str().and_then(|n| new_uuids.get(n)))
                .map(|u| json!(u))
                .collect();
            attachment.insert("nameOnlyAnnouncements".into(), Value::Array(mapped));
        }
        Some("queued_command") => {
            let source = attachment.get("source_uuid").and_then(Value::as_str);
            if let Some(new) = source.and_then(|s| new_uuids.get(s)) {
                attachment.insert("source_uuid".into(), json!(new));
            }
        }
        _ => {}
    }
}

/// Mode 0600, as Claude Code writes its sessions. The name is new, so `create_new`
/// never follows a link (SPEC.md 6.2, rule 7).
fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("Cannot copy the session: {e}"))?;
    file.write_all(bytes)
        .map_err(|e| format!("Cannot copy the session: {e}"))
}

/// A random UUID of version 4.
pub fn new_uuid() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("No random bytes from the OS: {e}"))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    });
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

fn now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    iso_time(now.as_millis())
}

/// `2026-09-25T06:46:21.432Z` for milliseconds since 1970, as JavaScript writes it.
pub fn iso_time(millis: u128) -> String {
    let seconds = i64::try_from(millis / 1000).unwrap_or(i64::MAX);
    let (days, second_of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Civil from days, by Howard Hinnant: March starts the year, so February is last.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        second_of_day / 3600,
        second_of_day % 3600 / 60,
        second_of_day % 60,
        millis % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b6ad9d2-1f2e-4c55-9a7e-2b1f4e6c8d01";
    const NEXT: &str = "5c1d0f7e-2a3b-4c4d-8e5f-6a7b8c9d0e1f";

    fn write_session(projects: &Path, folder: &str, id: &str, lines: &[&str]) -> PathBuf {
        let dir = projects.join(folder);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{id}.jsonl"));
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    fn user(uuid: &str, parent: Option<&str>, content: &str) -> String {
        json!({ "type": "user", "uuid": uuid, "parentUuid": parent, "cwd": "/w/app", "sessionId": ID,
                "message": { "role": "user", "content": content } })
        .to_string()
    }

    fn tool_result(uuid: &str, parent: &str) -> String {
        json!({ "type": "user", "uuid": uuid, "parentUuid": parent, "sessionId": ID,
                "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "t", "content": "ok" }] } })
        .to_string()
    }

    fn said(uuid: &str, parent: &str, message: &str, text: &str) -> String {
        json!({ "type": "assistant", "uuid": uuid, "parentUuid": parent, "sessionId": ID,
                "message": { "id": message, "role": "assistant", "content": [{ "type": "text", "text": text }] } })
        .to_string()
    }

    #[test]
    fn a_uuid_has_five_groups_of_hex_digits() {
        assert!(is_uuid(ID));
        assert!(!is_uuid("0b6ad9d2-1f2e-4c55-9a7e"));
        assert!(!is_uuid("0b6ad9d2-1f2e-4c55-9a7e-2b1f4e6c8d0g"));
        assert!(!is_uuid("../../etc/passwd"));
    }

    #[test]
    fn the_config_dir_of_the_env_list_counts_only_when_the_list_names_it() {
        let home = Path::new("/home/x");
        assert_eq!(
            projects_dir(&[], home),
            PathBuf::from("/home/x/.claude/projects")
        );
    }

    #[test]
    fn a_session_is_found_in_any_project_folder_by_its_id() {
        let projects = tempfile::tempdir().unwrap();
        let path = write_session(projects.path(), "-w-app", ID, &[&user("u1", None, "hi")]);
        assert_eq!(find(projects.path(), ID), Some(path));
        assert_eq!(find(projects.path(), NEXT), None);
        assert_eq!(find(projects.path(), "-w-app"), None, "not a uuid");
    }

    #[test]
    fn the_list_gives_the_folder_the_first_prompt_and_the_file_time() {
        let projects = tempfile::tempdir().unwrap();
        write_session(
            projects.path(),
            "-w-app",
            ID,
            &[&user("u1", None, "fix the\nbugs")],
        );
        fs::write(projects.path().join("-w-app").join("notes.jsonl"), "x").unwrap();
        let listed = list(projects.path());
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, ID);
        assert_eq!(listed[0].cwd, "/w/app");
        assert_eq!(listed[0].title, "fix the bugs");
        assert!(listed[0].updated > 1_700_000_000);
    }

    #[test]
    fn a_title_of_the_user_comes_before_a_title_of_the_model_and_the_prompt() {
        let head = user("u1", None, "first words");
        let ai = r#"{"type":"ai-title","aiTitle":"Model title","sessionId":"x"}"#;
        let custom = r#"{"type":"custom-title","customTitle":"My title","sessionId":"x"}"#;
        let title = |tail: &str| read_info(&head, tail, || None).unwrap().title;
        assert_eq!(title(&format!("{head}\n{ai}\n{custom}")), "My title");
        assert_eq!(title(&format!("{head}\n{ai}")), "Model title");
        assert_eq!(title(&head), "first words");
        let sidecar = read_info(&head, &head, || Some("Side title".into())).unwrap();
        assert_eq!(sidecar.title, "Side title");
    }

    #[test]
    fn a_sidecar_title_is_read_next_to_the_file() {
        let projects = tempfile::tempdir().unwrap();
        write_session(projects.path(), "-w-app", ID, &[&user("u1", None, "hi")]);
        let dir = projects.path().join("-w-app").join(ID);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("custom-title.json"),
            r#"{"customTitle":" Renamed "}"#,
        )
        .unwrap();
        assert_eq!(list(projects.path())[0].title, "Renamed");
    }

    #[test]
    fn a_subagent_file_or_a_session_with_no_folder_is_left_out() {
        let sidechain =
            r#"{"type":"user","isSidechain":true,"cwd":"/w","message":{"content":"x"}}"#;
        assert!(read_info(sidechain, sidechain, || None).is_none());
        let no_folder = r#"{"type":"user","uuid":"u","message":{"content":"x"}}"#;
        assert!(read_info(no_folder, no_folder, || None).is_none());
        let no_title = r#"{"type":"mode","cwd":"/w"}"#;
        assert!(read_info(no_title, no_title, || None).is_none());
    }

    #[test]
    fn a_moved_session_shows_its_new_folder() {
        let head = user("u1", None, "hi");
        let moved = r#"{"type":"relocated","relocatedCwd":"/w/new","sessionId":"x"}"#;
        let tail = format!("{head}\n{moved}");
        assert_eq!(read_info(&head, &tail, || None).unwrap().cwd, "/w/new");
    }

    #[test]
    fn a_session_that_went_on_in_another_file_is_left_out() {
        let projects = tempfile::tempdir().unwrap();
        let moved = format!(r#"{{"type":"continued-in","continuedInSessionId":"{NEXT}"}}"#);
        write_session(
            projects.path(),
            "-w",
            ID,
            &[&user("u1", None, "old"), &moved],
        );
        write_session(
            projects.path(),
            "-w",
            NEXT,
            &[&user("u2", Some("u1"), "new")],
        );
        let listed = list(projects.path());
        let ids: Vec<&str> = listed.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, [NEXT]);
        let later_prompt = format!("{moved}\n{}", user("u3", None, "more"));
        assert!(!continued_elsewhere(&later_prompt, |_| true));
    }

    #[test]
    fn a_string_field_is_read_from_raw_text_with_its_escapes() {
        let text = r#"{"cwd":"C:\\w\\a \"b\""} {"cwd": "/second"}"#;
        assert_eq!(first_string(text, "cwd").unwrap(), r#"C:\w\a "b""#);
        assert_eq!(last_string(text, "cwd").unwrap(), "/second");
        assert_eq!(first_string(r#"{"cwd":""}"#, "cwd"), None);
        assert_eq!(first_string(r#"{"cwd":"cut off"#, "cwd"), None);
    }

    #[test]
    fn a_prompt_skips_notes_and_tool_results_and_keeps_shell_input() {
        let entry = |content: Value| json!({ "type": "user", "message": { "content": content } });
        let text = |t: &str| prompt_of(&entry(json!(t)));
        assert_eq!(
            text("<command-name>/model</command-name>"),
            Prompt::Command("/model".into())
        );
        assert_eq!(
            text("<bash-input> ls -la </bash-input>"),
            Prompt::Text("! ls -la".into())
        );
        assert_eq!(
            text("<local-command-stdout>x</local-command-stdout>"),
            Prompt::None
        );
        assert_eq!(text("[Request interrupted by user]"), Prompt::None);
        assert_eq!(
            text("<3 is not a tag"),
            Prompt::Text("<3 is not a tag".into())
        );
        let result = entry(json!([{ "type": "tool_result", "content": "x" }]));
        assert_eq!(prompt_of(&result), Prompt::None);
        let meta = json!({ "type": "user", "isMeta": true, "message": { "content": "x" } });
        assert_eq!(prompt_of(&meta), Prompt::None);
        let Prompt::Text(cut) = text(&"é".repeat(250)) else {
            panic!("a long prompt is still a prompt");
        };
        assert_eq!(cut.chars().count(), 201);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn the_first_prompt_falls_back_to_a_slash_command() {
        let head = [
            user("u1", None, "<command-name>/model</command-name>"),
            user(
                "u2",
                Some("u1"),
                "<local-command-stdout>ok</local-command-stdout>",
            ),
        ]
        .join("\n");
        assert_eq!(read_info(&head, &head, || None).unwrap().title, "/model");
    }

    #[test]
    fn the_last_exchange_follows_the_newest_branch_and_skips_tool_results() {
        let text = [
            user("u1", None, "first question"),
            said("a1", "u1", "m1", "first answer"),
            user("u2", Some("a1"), "fix the\nbugs"),
            said("a2", "u2", "m2", "Looking."),
            tool_result("t1", "a2"),
            // A branch from a rewind: it ends before the newest leaf.
            said("x1", "u1", "m9", "dead branch"),
            said("a3", "t1", "m3", "All fixed."),
        ];
        let exchange = last_exchange(&text.join("\n"));
        assert_eq!(exchange, "fix the bugs\nLooking.\n\nAll fixed.");
    }

    #[test]
    fn the_answer_takes_every_entry_of_a_split_message() {
        let tool_use = json!({ "type": "assistant", "uuid": "a3", "parentUuid": "a1", "sessionId": ID,
                "message": { "id": "m1", "content": [{ "type": "tool_use", "name": "Bash", "input": {} }] } });
        let text = [
            user("u1", None, "go"),
            said("a1", "u1", "m1", "Part one."),
            said("a2", "u1", "m1", "Part two."),
            tool_use.to_string(),
        ];
        assert_eq!(
            last_exchange(&text.join("\n")),
            "go\nPart one.\n\nPart two."
        );
    }

    #[test]
    fn an_empty_or_broken_file_has_no_exchange() {
        assert_eq!(last_exchange(""), "");
        assert_eq!(last_exchange("not json\n{\"type\":\"user\"}"), "");
        let looped = [
            json!({ "type": "user", "uuid": "a", "parentUuid": "b", "message": { "content": "x" } })
                .to_string(),
            json!({ "type": "user", "uuid": "b", "parentUuid": "a", "message": { "content": "y" } })
                .to_string(),
        ];
        assert_eq!(last_exchange(&looped.join("\n")), "");
    }

    fn counter() -> impl FnMut() -> Result<String, String> {
        let mut n = 0;
        move || {
            n += 1;
            Ok(format!("new-{n}"))
        }
    }

    #[test]
    fn a_copy_gets_new_ids_and_skips_progress_and_subagents() {
        let progress =
            json!({ "type": "progress", "uuid": "p1", "parentUuid": "u1", "sessionId": ID });
        let sidechain = json!({ "type": "assistant", "uuid": "s1", "parentUuid": "u1", "isSidechain": true, "sessionId": ID });
        let mut answer: Value = serde_json::from_str(&said("a1", "p1", "m1", "done")).unwrap();
        answer["teamName"] = json!("team");
        let text = [
            r#"{"type":"mode","mode":"default","sessionId":"x"}"#.to_owned(),
            user("u1", None, "go"),
            progress.to_string(),
            sidechain.to_string(),
            answer.to_string(),
            r#"{"type":"ai-title","aiTitle":"Old","sessionId":"x"}"#.to_owned(),
        ]
        .join("\n");
        let out = fork_entries(&text, ID, NEXT, Some("Go"), "NOW", counter()).unwrap();
        let types: Vec<&str> = out.iter().filter_map(|e| e["type"].as_str()).collect();
        assert_eq!(types, ["user", "assistant", "custom-title"]);
        // The sidechain entry gets no uuid: new-1 is u1, new-2 is p1, new-3 is a1.
        assert_eq!(out[0]["uuid"], "new-1");
        assert_eq!(out[0]["parentUuid"], Value::Null);
        assert_eq!(out[1]["uuid"], "new-3");
        assert_eq!(
            out[1]["parentUuid"], "new-1",
            "the progress parent is skipped"
        );
        assert_eq!(out[1]["sessionId"], NEXT);
        assert_eq!(
            out[1]["timestamp"], "NOW",
            "the last entry has the time of the copy"
        );
        assert_eq!(
            out[1]["forkedFrom"],
            json!({ "sessionId": ID, "messageUuid": "a1" })
        );
        assert!(out[1].get("teamName").is_none());
        assert_eq!(out[2]["customTitle"], "Go (fork)");
    }

    #[test]
    fn a_copy_keeps_the_metadata_of_its_own_session_only() {
        let queued = json!({ "type": "attachment", "uuid": "q1", "parentUuid": "u1", "sessionId": ID,
                "attachment": { "type": "queued_command", "source_uuid": "u1" } });
        let deferred = json!({ "type": "attachment", "uuid": "d1", "parentUuid": "q1", "sessionId": ID,
                "attachment": { "type": "deferred_tools_record", "nameOnlyAnnouncements": ["u1", "gone"] } });
        let refusal = json!({ "type": "system", "subtype": "model_refusal_fallback", "uuid": "r1",
                "parentUuid": "d1", "logicalParentUuid": "q1", "sessionId": ID });
        let text = [
            user("u1", None, "go"),
            format!(r#"{{"type":"content-replacement","sessionId":"{ID}","replacements":[1]}}"#),
            r#"{"type":"content-replacement","sessionId":"other","replacements":[2]}"#.to_owned(),
            format!(r#"{{"type":"atis-latch","sessionId":"{ID}","atis":"abc"}}"#),
            format!(r#"{{"type":"relocated","sessionId":"{ID}","relocatedCwd":"/w/new"}}"#),
            r#"{"type":"history-suppression","sessionId":"other"}"#.to_owned(),
            queued.to_string(),
            deferred.to_string(),
            refusal.to_string(),
        ]
        .join("\n");
        let out = fork_entries(&text, ID, NEXT, None, "NOW", counter()).unwrap();
        let of = |kind: &str| out.iter().find(|e| e["type"] == kind).unwrap().clone();
        assert_eq!(out[0]["type"], "history-suppression");
        assert_eq!(of("content-replacement")["replacements"], json!([1]));
        assert_eq!(of("atis-latch")["atis"], "abc");
        assert_eq!(of("relocated")["relocatedCwd"], "/w/new");
        assert_eq!(of("custom-title")["customTitle"], "Forked session (fork)");
        let copied_queued = out.iter().find(|e| e["uuid"] == "new-2").unwrap();
        assert_eq!(copied_queued["attachment"]["source_uuid"], "new-1");
        let copied_deferred = out.iter().find(|e| e["uuid"] == "new-3").unwrap();
        assert_eq!(
            copied_deferred["attachment"]["nameOnlyAnnouncements"],
            json!(["new-1"])
        );
        let copied_refusal = of("system");
        assert_eq!(copied_refusal["neutralizedByFork"], true);
        assert_eq!(copied_refusal["logicalParentUuid"], "new-2");
    }

    #[test]
    fn a_session_with_no_messages_cannot_be_copied() {
        let text = r#"{"type":"mode","sessionId":"x"}"#;
        assert!(fork_entries(text, ID, NEXT, None, "NOW", counter()).is_err());
    }

    #[test]
    fn fork_writes_a_new_private_file_next_to_the_old_one() {
        let projects = tempfile::tempdir().unwrap();
        let old = write_session(projects.path(), "-w-app", ID, &[&user("u1", None, "go")]);
        let new = fork(&old, ID).unwrap();
        assert!(is_uuid(&new));
        let copy = find(projects.path(), &new).unwrap();
        assert_eq!(copy.parent(), old.parent());
        let listed = list(projects.path());
        let titles: Vec<&str> = listed.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"go (fork)"), "{titles:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&copy).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_new_uuid_has_version_4() {
        let id = new_uuid().unwrap();
        assert!(is_uuid(&id));
        assert_eq!(&id[14..15], "4");
        assert_ne!(id, new_uuid().unwrap());
    }

    #[test]
    fn a_time_prints_as_javascript_prints_it() {
        assert_eq!(iso_time(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_time(1_709_251_199_999), "2024-02-29T23:59:59.999Z");
        assert_eq!(iso_time(1_790_318_781_432), "2026-09-25T06:46:21.432Z");
    }

    #[test]
    fn the_last_exchange_of_a_big_file_is_read_from_its_end() {
        let projects = tempfile::tempdir().unwrap();
        let filler = said("a0", "u0", "m0", &"x".repeat(1024));
        let mut lines = vec![user("u0", None, "old")];
        lines.extend(std::iter::repeat_n(filler, 9 * 1024));
        lines.push(user("u1", Some("a0"), "new question"));
        lines.push(said("a1", "u1", "m1", "new answer"));
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let path = write_session(projects.path(), "-w", ID, &refs);
        assert_eq!(
            read_last_exchange(&path).unwrap(),
            "new question\nnew answer"
        );
    }
}
