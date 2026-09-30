//! The bridge state machine. It follows `models/transport.qnt`: records stay in the
//! body until the addon reads them, a full body refuses new messages, and every
//! message runs at most once. No I/O here.

use std::collections::{BTreeMap, BTreeSet};

use protocol::apps::App;
use protocol::folder::resolve_folder;
use protocol::frame::{MAX_AGE, MAX_AHEAD};
use protocol::live::Notices;
use protocol::rate::{ChatQueue, MAX_QUEUE, enqueue};
use protocol::record::Record;
use protocol::restore::{prepare_restore, restore_body};
use protocol::slot::Status;

use serde::{Deserialize, Serialize};

use crate::accounts::Accounts;
use crate::activity::{self, Activity};
use crate::agent::{Choice, SessionId, SessionInfo};
use crate::always_rules::RuleLine;
use crate::chat_branch::ChatWorktree;
use crate::config::{DEFAULT_MAX_PARALLEL_RUNS, Permission, Policy};
use crate::desktop::Notice;
use crate::flags::{self, GitFlag, ListKind, TransportFlags};
use crate::folder_list::folder_reply;
use crate::folder_path::{folder_request, native_folder, path_bytes, relative_folder};
use crate::folder_trust::{Untrusted, check_text};
use crate::folder_walk::Snapshot;
use crate::git_actions::{Effect, GitAction};
use crate::git_blocks::{
    RunBlocks, blocks, error_with_blocks, plain_error, with_blocks, without_blocks,
};
use crate::history::{ChatLog, History, Speaker};
pub use crate::lane::{ChatId, MessageId};
use crate::lane::{Lane, NotAdmitted, keep_last};
use crate::new_folder::{NewFolderError, is_folder_name};
use crate::reply::{render_reply, with_usage};
use crate::run_changes::{Outcome as ChangeOutcome, RunChanges};
use crate::settings_list::{BridgeSettings, HookLine, settings_reply};
use crate::state::State;
use crate::usage::Usage;

const BAD_FOLDER: &str =
    "That folder isn't allowed. Pick another one, or add it to allowed_roots in config.toml.";
pub const BAD_AGENT: &str =
    "That agent isn't in config.toml. Pick another one in Settings, or add it on your desktop.";
const STOPPED: &str = "Stopped.";
const RESTARTED: &str = "Stopped: the desktop app restarted.";
/// The agent sessions of the chats with the latest runs.
const MAX_SESSIONS: usize = 64;
/// The sessions in one list for the game, newest first.
const MAX_LISTED: usize = 30;
const MAX_TITLE: usize = 100;
/// A session that changed this recently is probably open in a terminal.
const ACTIVE_FOR: u32 = 300;
const NO_SESSION: &str = "That session is gone. Open Resume to pick another.";
const UNKNOWN_ACTION: &str =
    "The desktop app doesn't know that action. Update it: run gnomish-relay update.";
/// The runs whose change summary Commit and Revert can still act on.
const MAX_CHANGES: usize = 32;

/// The agent session of a chat. The next message of the chat resumes it, if its
/// agent and its folder are the same (SPEC.md 9.5).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentSession {
    pub chat: ChatId,
    pub agent: String,
    pub cwd: String,
    pub id: SessionId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Session {
    New,
    Resume,
}

/// What a job does. Only a prompt reaches the model.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Work {
    #[default]
    Prompt,
    /// The saved sessions of every agent, for Resume in the game.
    ListSessions,
    /// The folder tree of the roots, for the folder browser.
    ListFolders,
    /// What the bridge allows, for the Settings and Diag tabs.
    ListSettings,
    /// A new chat continues this session.
    Attach {
        session: SessionId,
        #[serde(rename = "fork")]
        open: Open,
    },
    /// A git action of the player on the chat (SPEC.md 9.11). The bridge runs it, never
    /// an agent.
    Git(GitAction),
}

/// Where a run of a chat works (SPEC.md 9.11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchPlan {
    /// In the chat folder.
    Plain,
    /// In a new own branch, named after the chat.
    Make {
        name: String,
    },
    Use(ChatWorktree),
}

/// How an attach opens a saved session. A session that is open in a terminal gets a
/// fork, so the two never write into one session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "bool", into = "bool")]
pub enum Open {
    Same,
    Fork,
}

// `state.json` keeps the form of older bridges: `"fork": true`.
impl From<bool> for Open {
    fn from(fork: bool) -> Open {
        if fork { Open::Fork } else { Open::Same }
    }
}

impl From<Open> for bool {
    fn from(open: Open) -> bool {
        open == Open::Fork
    }
}

/// A session of the last list. The game can resume only these, so the folder check
/// of the list also guards every resume.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Listed {
    agent: String,
    id: SessionId,
    /// The folder in the form that jobs use.
    cwd: String,
    /// The folder relative to the base, as the game sends it back.
    folder: String,
    title: String,
    updated: u32,
}

/// Where agents can work (SPEC.md 6.2, rule 1). A folder from the game is relative
/// to `base`, and must stay inside one of `roots`.
pub struct Folders {
    pub roots: Vec<Vec<u8>>,
    pub base: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub token: String,
    pub chat: ChatId,
    pub id: MessageId,
    pub agent: String,
    /// A job from an older state file has none, and gets the strictest level.
    #[serde(default)]
    pub permission: Permission,
    /// The level that the chat asked for. The config can lower it (S6).
    #[serde(default)]
    pub asked: Permission,
    pub cwd: String,
    pub session: Session,
    /// The agent session to resume, set when the job starts.
    #[serde(default)]
    pub resume: Option<SessionId>,
    pub text: String,
    #[serde(default)]
    pub work: Work,
    /// The bridge makes the last part of `cwd` before the run (SPEC.md 9.9).
    #[serde(default)]
    pub new_folder: bool,
}

impl Job {
    pub fn resume_id(&self) -> Option<&str> {
        self.resume.as_ref().map(SessionId::as_str)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Duplicate,
    /// Not marked as seen, so the addon sends it again later.
    Refused,
    /// Seen, and answered with an error. It never runs.
    BadFolder,
    /// Seen, and answered with an error: the config has no such agent.
    BadAgent,
    /// Seen, and answered with an error: the last list had no such session.
    BadSession,
    /// Seen, and answered with the update text of its app (SPEC.md 7.7).
    WrongVersion,
    /// Seen, and answered with an error: a git action that this bridge does not know.
    BadAction,
    Control,
}

/// The coding app over its lane: the policy, the queues, the agent sessions, and the
/// history for a restore.
pub struct Relay {
    policy: Policy,
    lane: Lane,
    queues: BTreeMap<ChatId, ChatQueue>,
    jobs: BTreeMap<(ChatId, MessageId), Queued>,
    /// Chats with a run in progress, also a list.
    running: BTreeSet<ChatId>,
    /// Chats with a run of an agent in progress. Only these count for the limit.
    agent_runs: BTreeSet<ChatId>,
    max_runs: usize,
    /// The number of the next message that the relay takes (SPEC.md 8.2).
    arrivals: u64,
    history: History,
    /// The new token after a saved-data wipe, until it reports `restored`.
    restore_for: Option<String>,
    accounts: Accounts,
    sessions: Vec<AgentSession>,
    /// Chats whose run in progress got a Stop. The bridge signals each run.
    cancels: Vec<ChatId>,
    /// The "Always allow" rules that the Settings tab removed, by id.
    rule_removals: Vec<String>,
    /// Chats whose run waited for an answer when a new message came (SPEC.md 9.3).
    interrupts: Vec<ChatId>,
    /// Chats that the game deleted. A run of one that ends later leaves no trace.
    deleted: Vec<ChatId>,
    listed: Vec<Listed>,
    activity: Activity,
    /// The tags of the frames that came, with the time of their first sight.
    frames: Vec<(u32, FrameTag)>,
    /// The chats that asked for an own branch, with their names (SPEC.md 9.11).
    own_branch: BTreeMap<ChatId, String>,
    worktrees: Vec<ChatWorktree>,
    /// The worktrees of deleted chats, which the bridge cleans up.
    cleanups: Vec<ChatWorktree>,
    /// The last runs with a change summary.
    changes: Vec<RunChanges>,
    /// In the form of the resolver. With it, a folder in the home folder under no root
    /// runs after a click on the desktop (SPEC.md 9.12).
    home: Option<Vec<u8>>,
}

/// A waiting message, with its place in the order of arrival across all chats.
struct Queued {
    arrival: u64,
    job: Job,
}

impl Work {
    /// A list is short, so it never waits for the limit on parallel runs (SPEC.md 8.2).
    fn is_agent_run(&self) -> bool {
        matches!(self, Work::Prompt | Work::Attach { .. })
    }
}

/// The tag of a signed frame (SPEC.md 6.3). Two frames with the same tag are one frame.
pub type FrameTag = [u8; 8];

/// A frame passes the freshness check (S11) for at most this long after its first sight:
/// its time is at most `MAX_AHEAD` in the future then, and it is stale `MAX_AGE` later.
const FRAME_MEMORY: u32 = MAX_AGE + MAX_AHEAD;

/// Stop, delete, a permission answer, a rule removal, and a hello.
fn is_control(r: &Record) -> bool {
    let coding = flags::coding(&r.flags);
    flags::transport(&r.flags).hello
        || coding.stop
        || coding.delete
        || coding.perm.is_some()
        || coding.remove_rule.is_some()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A tab or a line break inside a field would break the lines of a list.
fn field(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The folder in the form of the resolver, or `None` outside every root.
pub fn in_roots(folders: &Folders, path: &std::path::Path) -> Option<Vec<u8>> {
    let mut target = path_bytes(path);
    // A Windows path starts with its drive. The resolver takes it as absolute only
    // with a `/` first.
    if !target.starts_with(b"/") {
        target.insert(0, b'/');
    }
    resolve_folder(&folders.roots, &folders.base, &target)
}

/// A new folder comes from the browser: a relative path whose last part is a name.
/// The bridge checks the rest on the disk when the run starts.
fn is_new_folder_request(cwd: &[u8]) -> bool {
    let Ok(cwd) = std::str::from_utf8(cwd) else {
        return false;
    };
    let last = cwd.rsplit(['/', '\\']).next().unwrap_or_default();
    !cwd.starts_with(['/', '\\']) && is_folder_name(last)
}

fn cut_chars(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl Relay {
    pub fn new(policy: Policy) -> Relay {
        Relay {
            policy,
            lane: Lane::new(App::Relay),
            queues: BTreeMap::new(),
            jobs: BTreeMap::new(),
            running: BTreeSet::new(),
            agent_runs: BTreeSet::new(),
            max_runs: DEFAULT_MAX_PARALLEL_RUNS,
            arrivals: 0,
            history: History::default(),
            restore_for: None,
            accounts: Accounts::default(),
            sessions: Vec::new(),
            cancels: Vec::new(),
            rule_removals: Vec::new(),
            interrupts: Vec::new(),
            deleted: Vec::new(),
            listed: Vec::new(),
            activity: Activity::default(),
            frames: Vec::new(),
            own_branch: BTreeMap::new(),
            worktrees: Vec::new(),
            cleanups: Vec::new(),
            changes: Vec::new(),
            home: None,
        }
    }

    /// A folder in `home` under no root then waits for a click on the desktop, and does
    /// not end as an error (SPEC.md 9.12).
    pub fn take_new_folders(&mut self, home: &std::path::Path) {
        let mut home = path_bytes(home);
        // A Windows path starts with its drive. The resolver takes it as absolute only
        // with a `/` first.
        if !home.starts_with(b"/") {
            home.insert(0, b'/');
        }
        self.home = Some(home);
    }

    /// Where the folder browser looks: the roots, and the home folder when a click on the
    /// desktop can add a folder there (SPEC.md 9.12).
    fn browse_area(&self) -> Folders {
        let mut roots = self.policy.folders.roots.clone();
        roots.extend(self.home.clone());
        Folders {
            roots,
            base: self.policy.folders.base.clone(),
        }
    }

    /// A root that a click on the desktop added.
    pub fn add_root(&mut self, root: &std::path::Path) {
        let mut root = path_bytes(root);
        if !root.starts_with(b"/") {
            root.insert(0, b'/');
        }
        if !self.policy.folders.roots.contains(&root) {
            self.policy.folders.roots.push(root);
        }
    }

    pub fn set_max_runs(&mut self, max_runs: usize) {
        self.max_runs = max_runs;
    }

    pub fn client_build(&self) -> Option<&str> {
        self.lane.client_build()
    }

    pub fn addon_version(&self) -> Option<u32> {
        self.lane.addon_version()
    }

    pub fn next_slot(&self) -> usize {
        self.lane.next_slot()
    }

    pub fn next_slots(&self) -> Vec<usize> {
        self.lane.next_slots()
    }

    /// `token` is the token in the saved variables file that changed.
    pub fn reset_window(&mut self, token: Option<&str>) {
        self.lane.reset_window(token);
    }

    /// The saved variables file of `account` holds `token`. A new token in a folder that
    /// held another one is a wipe, so the older token retires (SPEC.md 7.6). Returns it.
    pub fn saw_saved_token(&mut self, account: &str, token: &str) -> Option<String> {
        let old = self.accounts.replaced_token(account, token)?;
        self.lane.retire(&old);
        Some(old)
    }

    pub fn unread(&self) -> usize {
        self.lane.unread()
    }

    /// Takes one signed frame. A frame that came before, also before a restart, applies no
    /// control record and no report again, so a replayed strip cannot stop or delete a
    /// later run. Its messages
    /// go through the replay store as always: a refused message gets its next chance.
    pub fn on_tagged_frame(&mut self, tag: FrameTag, records: &[Record], now: u32) -> Vec<Outcome> {
        if self.first_sight(tag, now) {
            return self.on_frame(records, now);
        }
        records
            .iter()
            .map(|r| {
                if is_control(r) {
                    Outcome::Duplicate
                } else {
                    self.on_record(r, now)
                }
            })
            .collect()
    }

    fn first_sight(&mut self, tag: FrameTag, now: u32) -> bool {
        self.frames
            .retain(|(seen, _)| now.saturating_sub(*seen) <= FRAME_MEMORY);
        if self.frames.iter().any(|(_, known)| *known == tag) {
            return false;
        }
        self.frames.push((now, tag));
        true
    }

    /// Takes one frame. The flags of its first record carry the report of the addon.
    pub fn on_frame(&mut self, records: &[Record], now: u32) -> Vec<Outcome> {
        if let Some(first) = records.first() {
            self.take_report(&text(&first.token), &flags::transport(&first.flags));
        }
        records.iter().map(|r| self.on_record(r, now)).collect()
    }

    fn take_report(&mut self, token: &str, flags: &TransportFlags) {
        self.lane.take_report(token, flags);
        self.take_restore_report(token, flags);
    }

    /// A hello from a new token starts a restore, and its `restored` flag ends it. A
    /// second account sends the same hello, so only the saved variables retire a token
    /// (SPEC.md 7.6).
    fn take_restore_report(&mut self, token: &str, flags: &TransportFlags) {
        if flags.restored && self.restore_for.as_deref() == Some(token) {
            self.restore_for = None;
            return;
        }
        if !flags.hello || self.lane.knows_token(token) {
            return;
        }
        if self.lane.has_tokens() && !self.history.is_empty() {
            self.restore_for = Some(token.to_owned());
        }
        self.lane.add_token(token);
    }

    /// Stop, delete, a permission answer, and a rule removal start no run.
    fn took_control(&mut self, chat: &ChatId, flags: &flags::CodingFlags) -> bool {
        if flags.stop {
            self.stop(chat);
        } else if flags.delete {
            self.delete(chat.clone());
        } else if let Some(answer) = &flags.perm {
            self.activity.answer(chat, answer);
        } else if let Some(id) = &flags.remove_rule {
            self.rule_removals.push(id.clone());
        } else {
            return false;
        }
        true
    }

    fn on_record(&mut self, r: &Record, now: u32) -> Outcome {
        let chat = ChatId::new(text(&r.chat));
        if flags::transport(&r.flags).hello {
            return Outcome::Control;
        }
        let flags = flags::coding(&r.flags);
        if self.took_control(&chat, &flags) {
            return Outcome::Control;
        }
        if let Err(outcome) = self.admit(r, &chat, now) {
            return outcome;
        }
        if let Some(update) = self.lane.update_text() {
            return self.refuse(r, &chat, update.into(), Outcome::WrongVersion);
        }
        if let Some(kind) = flags.list {
            return self.enqueue_list(r, chat, kind);
        }
        if let Some(session) = &flags.attach {
            return self.attach(r, chat, session, now);
        }
        if let Some(git) = flags.git.clone() {
            return self.enqueue_git(r, chat, git);
        }
        if flags.own_branch {
            self.own_branch.insert(chat.clone(), text(&r.name));
        }
        let job = match self.prompt_job(r, &chat, flags) {
            Ok(job) => job,
            Err(outcome) => return outcome,
        };
        let waits = self.running.contains(&chat) && self.activity.waits(&chat);
        let outcome = self.enqueue_job(job);
        // The player never has to answer an old question first. A run that only works
        // keeps working, and the message waits (SPEC.md 9.3).
        if outcome == Outcome::Accepted && waits {
            self.interrupts.push(chat);
        }
        outcome
    }

    /// Answers a seen message with an error. It never runs.
    fn refuse(&mut self, r: &Record, chat: &ChatId, error: String, outcome: Outcome) -> Outcome {
        self.set_record(&text(&r.token), chat, MessageId(r.id), Status::Error, error);
        outcome
    }

    /// The history keeps the message, also when the job is refused.
    fn prompt_job(
        &mut self,
        r: &Record,
        chat: &ChatId,
        flags: flags::CodingFlags,
    ) -> Result<Job, Outcome> {
        let agent = flags
            .agent
            .unwrap_or_else(|| self.policy.default_agent.clone());
        self.add_to_history(r, chat, &agent);
        let cwd = match self.prompt_folder(&r.cwd) {
            Ok(cwd) => cwd,
            Err(refused) => return Err(self.refuse(r, chat, refused, Outcome::BadFolder)),
        };
        // Only the first message of a chat makes its folder (SPEC.md 9.9).
        let new_folder = flags.new_folder && flags.new_session;
        if new_folder && !is_new_folder_request(&r.cwd) {
            let bad = NewFolderError::BadName.text();
            return Err(self.refuse(r, chat, bad, Outcome::BadFolder));
        }
        let Some(&permission) = self.policy.agents.get(&agent) else {
            return Err(self.refuse(r, chat, BAD_AGENT.into(), Outcome::BadAgent));
        };
        Ok(Job {
            token: text(&r.token),
            chat: chat.clone(),
            id: MessageId(r.id),
            agent,
            permission: permission.ceiling(flags.level),
            asked: flags.level.unwrap_or(permission),
            cwd: text(&native_folder(cwd, cfg!(windows))),
            session: if flags.new_session {
                Session::New
            } else {
                Session::Resume
            },
            resume: None,
            text: text(&r.text),
            work: Work::Prompt,
            new_folder,
        })
    }

    fn add_to_history(&mut self, r: &Record, chat: &ChatId, agent: &str) {
        let log = ChatLog {
            chat: chat.clone(),
            name: text(&r.name),
            agent: agent.to_owned(),
            cwd: text(&r.cwd),
            lines: Vec::new(),
        };
        self.history
            .add_message(log, MessageId(r.id), &text(&r.text));
    }

    /// A folder inside a root, or a new folder that passes the rules of its text. The
    /// start of the run checks the real folder (SPEC.md 9.12).
    fn prompt_folder(&self, raw: &[u8]) -> Result<Vec<u8>, String> {
        if let Some(cwd) = self.game_folder(raw) {
            return Ok(cwd);
        }
        let (Some(home), Some(request)) = (&self.home, folder_request(raw, cfg!(windows))) else {
            return Err(BAD_FOLDER.into());
        };
        let base = &self.policy.folders.base;
        let Some(cwd) = resolve_folder(std::slice::from_ref(home), base, &request) else {
            return Err(Untrusted::OutsideHome.text().into());
        };
        check_text(home, &cwd).map_err(|why| why.text().to_owned())?;
        Ok(cwd)
    }

    /// The folder in the form of the resolver, or `None` outside every root.
    fn game_folder(&self, raw: &[u8]) -> Option<Vec<u8>> {
        let folders = &self.policy.folders;
        let request = folder_request(raw, cfg!(windows))?;
        resolve_folder(&folders.roots, &folders.base, &request)
    }

    fn enqueue_list(&mut self, r: &Record, chat: ChatId, kind: ListKind) -> Outcome {
        let base = self.policy.folders.base.clone();
        self.enqueue_job(Job {
            token: text(&r.token),
            chat,
            id: MessageId(r.id),
            agent: self.policy.default_agent.clone(),
            permission: Permission::Ask,
            asked: Permission::Ask,
            cwd: text(&native_folder(base, cfg!(windows))),
            session: Session::New,
            resume: None,
            text: String::new(),
            work: match kind {
                ListKind::Sessions => Work::ListSessions,
                ListKind::Folders => Work::ListFolders,
                ListKind::Settings => Work::ListSettings,
            },
            new_folder: false,
        })
    }

    /// A git action waits in the queue of its chat, behind a run of the chat. It never
    /// ends a wait for an answer: only a message to the agent does (SPEC.md 9.3).
    fn enqueue_git(&mut self, r: &Record, chat: ChatId, git: GitFlag) -> Outcome {
        let GitFlag::Action(action) = git else {
            return self.refuse(r, &chat, UNKNOWN_ACTION.into(), Outcome::BadAction);
        };
        let Some(cwd) = self.game_folder(&r.cwd) else {
            return self.refuse(r, &chat, BAD_FOLDER.into(), Outcome::BadFolder);
        };
        self.enqueue_job(Job {
            token: text(&r.token),
            chat,
            id: MessageId(r.id),
            agent: self.policy.default_agent.clone(),
            permission: Permission::Ask,
            asked: Permission::Ask,
            cwd: text(&native_folder(cwd, cfg!(windows))),
            session: Session::Resume,
            resume: None,
            text: text(&r.text),
            work: Work::Git(action),
            new_folder: false,
        })
    }

    fn attach(&mut self, r: &Record, chat: ChatId, session: &str, now: u32) -> Outcome {
        let Some(listed) = self
            .listed
            .iter()
            .find(|l| l.id.as_str() == session)
            .cloned()
        else {
            return self.refuse(r, &chat, NO_SESSION.into(), Outcome::BadSession);
        };
        self.enqueue_job(Job {
            token: text(&r.token),
            chat,
            id: MessageId(r.id),
            agent: listed.agent,
            permission: Permission::Ask,
            asked: Permission::Ask,
            cwd: listed.cwd,
            session: Session::New,
            resume: None,
            text: String::new(),
            work: Work::Attach {
                session: listed.id,
                open: if now.saturating_sub(listed.updated) < ACTIVE_FOR {
                    Open::Fork
                } else {
                    Open::Same
                },
            },
            new_folder: false,
        })
    }

    /// Marks the message as seen, or says why not. Every refusal of the chat queue
    /// comes before the lane marks it as seen: a refused message must not count as seen.
    fn admit(&mut self, r: &Record, chat: &ChatId, now: u32) -> Result<(), Outcome> {
        let queued = self.queues.get(chat).map_or(0, |q| q.ids.len());
        if queued >= MAX_QUEUE {
            return Err(Outcome::Refused);
        }
        // Jobs wait under their chat and id. A second token with the same pair waits
        // until the first job leaves, or it overwrites the first job.
        let waiting = self.jobs.get(&(chat.clone(), MessageId(r.id)));
        if waiting.is_some_and(|queued| queued.job.token.as_bytes() != r.token) {
            return Err(Outcome::Refused);
        }
        self.lane.admit(&r.token, r.id, now).map_err(|e| match e {
            NotAdmitted::Refused => Outcome::Refused,
            NotAdmitted::Duplicate => Outcome::Duplicate,
        })
    }

    fn enqueue_job(&mut self, job: Job) -> Outcome {
        let queue = self
            .queues
            .remove(&job.chat)
            .unwrap_or(ChatQueue { ids: Vec::new() });
        let Some(queue) = enqueue(queue, job.id.0) else {
            return Outcome::Refused;
        };
        self.queues.insert(job.chat.clone(), queue);
        self.set_record(
            &job.token,
            &job.chat,
            job.id,
            Status::Working,
            String::new(),
        );
        let arrival = self.arrivals;
        self.arrivals += 1;
        self.jobs
            .insert((job.chat.clone(), job.id), Queued { arrival, job });
        Outcome::Accepted
    }

    /// The waiting messages of a chat end as errors. A run in progress goes on.
    fn stop(&mut self, chat: &ChatId) {
        if self.running.contains(chat) {
            self.cancels.push(chat.clone());
        }
        let Some(queue) = self.queues.remove(chat) else {
            return;
        };
        for id in queue.ids {
            let id = MessageId(id);
            self.activity.end(chat, id);
            if let Some(Queued { job, .. }) = self.jobs.remove(&(chat.clone(), id)) {
                self.set_record(&job.token, chat, job.id, Status::Error, STOPPED.into());
            }
        }
    }

    /// The game has no chat to show a reply in, so a reply of the chat could never be
    /// read and would stay in the body for good (SPEC.md 7.3).
    fn delete(&mut self, chat: ChatId) {
        self.stop(&chat);
        self.lane.remove_chat(&chat);
        self.sessions.retain(|s| s.chat != chat);
        self.history.remove(&chat);
        self.own_branch.remove(&chat);
        self.changes.retain(|c| c.chat != chat);
        // A run in progress still works in the worktree, so its end hands it over.
        if !self.running.contains(&chat) {
            self.clean_up_worktree_of(&chat);
        }
        self.deleted.push(chat);
        keep_last(&mut self.deleted, MAX_SESSIONS);
    }

    fn clean_up_worktree_of(&mut self, chat: &ChatId) {
        let (gone, kept) = std::mem::take(&mut self.worktrees)
            .into_iter()
            .partition(|w| &w.chat == chat);
        self.worktrees = kept;
        self.cleanups.extend(gone);
    }

    fn is_deleted(&self, chat: &ChatId) -> bool {
        self.deleted.contains(chat)
    }

    /// The first waiting message of each chat with no run in progress, oldest first.
    fn heads(&self) -> Vec<&Queued> {
        let mut heads: Vec<&Queued> = self
            .queues
            .iter()
            .filter(|(chat, _)| !self.running.contains(*chat))
            .filter_map(|(chat, q)| self.jobs.get(&(chat.clone(), MessageId(*q.ids.first()?))))
            .collect();
        heads.sort_by_key(|queued| queued.arrival);
        heads
    }

    fn at_limit(&self) -> bool {
        self.agent_runs.len() >= self.max_runs
    }

    /// The oldest message that can start now: a list always, a run below the limit.
    pub fn next_job(&mut self) -> Option<Job> {
        let at_limit = self.at_limit();
        let next = self
            .heads()
            .into_iter()
            .find(|queued| !at_limit || !queued.job.work.is_agent_run())?;
        let (chat, id) = (next.job.chat.clone(), next.job.id);
        self.queues.get_mut(&chat)?.ids.remove(0);
        let Queued { mut job, .. } = self.jobs.remove(&(chat.clone(), id))?;
        self.activity.end(&chat, id);
        if job.work.is_agent_run() {
            self.agent_runs.insert(chat.clone());
        }
        if job.session == Session::Resume {
            // An own branch keeps its session in the worktree, where its runs work.
            let folder = self
                .worktree_of(&chat)
                .map_or(job.cwd.clone(), |w| w.folder.clone());
            job.resume = self
                .sessions
                .iter()
                .find(|s| s.chat == job.chat && s.agent == job.agent && s.cwd == folder)
                .map(|s| s.id.clone());
        }
        self.running.insert(chat);
        Some(job)
    }

    /// Each message that waits for the limit, and not for its own chat, says so in the
    /// game. Returns true when a line changed.
    pub fn show_waiting(&mut self) -> bool {
        if !self.at_limit() {
            return false;
        }
        let waiting: Vec<(ChatId, MessageId)> = self
            .heads()
            .into_iter()
            .filter(|queued| queued.job.work.is_agent_run())
            .map(|queued| (queued.job.chat.clone(), queued.job.id))
            .collect();
        let running = self.agent_runs.len();
        let mut changed = false;
        for (ahead, (chat, id)) in waiting.iter().enumerate() {
            changed |= self
                .activity
                .wait(chat, *id, activity::waiting_line(running, ahead));
        }
        changed
    }

    fn end_run(&mut self, chat: &ChatId) {
        self.running.remove(chat);
        self.agent_runs.remove(chat);
        if self.is_deleted(chat) {
            self.clean_up_worktree_of(chat);
        }
    }

    pub fn take_rule_removals(&mut self) -> Vec<String> {
        std::mem::take(&mut self.rule_removals)
    }

    pub fn take_cancels(&mut self) -> Vec<ChatId> {
        std::mem::take(&mut self.cancels)
    }

    pub fn take_interrupts(&mut self) -> Vec<ChatId> {
        std::mem::take(&mut self.interrupts)
    }

    pub fn keep_session(&mut self, job: &Job, id: Option<SessionId>) {
        let Some(id) = id else {
            return;
        };
        if self.is_deleted(&job.chat) {
            return;
        }
        self.sessions.retain(|s| s.chat != job.chat);
        self.sessions.push(AgentSession {
            chat: job.chat.clone(),
            agent: job.agent.clone(),
            cwd: job.cwd.clone(),
            id,
        });
        keep_last(&mut self.sessions, MAX_SESSIONS);
    }

    /// The first progress line of a run says its level, so the game shows the level
    /// that applies, not the one that the chat asked for (SPEC.md 9.3).
    pub fn begin(&mut self, job: &Job) {
        self.show_level(&job.chat, job.id, job.permission, job.asked);
    }

    pub fn show_level(
        &mut self,
        chat: &ChatId,
        id: MessageId,
        level: Permission,
        asked: Permission,
    ) {
        let line = activity::level_line(level, asked);
        self.activity.begin(chat, id, line);
    }

    /// The level of the config for `agent`, after a raise on the desktop wrote it.
    pub fn set_level(&mut self, agent: &str, level: Permission) {
        if let Some(known) = self.policy.agents.get_mut(agent) {
            *known = level;
        }
    }

    pub fn step(&mut self, chat: &ChatId, id: MessageId, line: String) {
        self.activity.step(chat, id, line);
    }

    pub fn desktop(&mut self, chat: &ChatId, id: MessageId, notice: Notice) {
        self.activity.desktop(chat, id, notice);
    }

    pub fn ask(
        &mut self,
        chat: &ChatId,
        id: MessageId,
        text: Vec<u8>,
        choices: Vec<Choice>,
        now: u32,
    ) -> String {
        self.activity.ask(chat, id, text, choices, now)
    }

    pub fn withdraw(&mut self, chat: &ChatId, id: MessageId) {
        self.activity.withdraw(chat, id);
    }

    pub fn take_answers(&mut self) -> Vec<(String, Option<usize>)> {
        self.activity.take_answers()
    }

    pub fn is_asked(&self, request: &str) -> bool {
        self.activity.is_open(request)
    }

    pub fn live_file(&self, notices: &Notices) -> Vec<u8> {
        self.activity.file(notices)
    }

    pub fn finish(&mut self, job: &Job, result: Result<String, String>) {
        self.finish_run(job, result, RunBlocks::default(), None);
    }

    /// The reply of a run, with the usage line and the blocks of the bridge (SPEC.md
    /// 7.3.1, 9.10, 9.11).
    pub fn finish_run(
        &mut self,
        job: &Job,
        result: Result<String, String>,
        run: RunBlocks,
        usage: Option<&Usage>,
    ) {
        self.activity.end(&job.chat, job.id);
        self.end_run(&job.chat);
        if self.is_deleted(&job.chat) {
            return;
        }
        let added = blocks(&run);
        let (status, text, history) = match result {
            Ok(text) => {
                let rendered = render_reply(&job.work, &with_level_note(job, text));
                let text = with_blocks(&rendered, &added);
                let text = match usage {
                    Some(usage) => with_usage(&text, usage),
                    None => text,
                };
                let history = without_blocks(&text);
                (Status::Done, text, history)
            }
            // A restored error shows as plain text, so its history gets no marker.
            Err(text) if !added.is_empty() => (
                Status::Error,
                error_with_blocks(&text, &added),
                plain_error(&text),
            ),
            Err(text) => (Status::Error, plain_error(&text), plain_error(&text)),
        };
        if let Some(changes) = run.changes {
            self.keep_changes(changes);
        }
        self.record_with_history(&job.token, &job.chat, job.id, status, text, &history);
    }

    fn keep_changes(&mut self, changes: RunChanges) {
        self.changes
            .retain(|c| (c.chat != changes.chat) || (c.id != changes.id));
        self.changes.push(changes);
        keep_last(&mut self.changes, MAX_CHANGES);
    }

    /// The end of a git action. Its reply is text of the bridge, with blocks when it
    /// has them.
    pub fn finish_git(&mut self, job: &Job, result: Result<String, String>, effect: &Effect) {
        self.activity.end(&job.chat, job.id);
        self.end_run(&job.chat);
        if self.is_deleted(&job.chat) {
            return;
        }
        self.apply_effect(&job.chat, effect);
        let (status, text) = match result {
            Ok(text) => (Status::Done, text),
            Err(text) => (Status::Error, plain_error(&text)),
        };
        self.set_record(&job.token, &job.chat, job.id, status, text);
    }

    fn apply_effect(&mut self, chat: &ChatId, effect: &Effect) {
        let (id, outcome) = match effect {
            Effect::Nothing => return,
            Effect::Discarded => {
                self.worktrees.retain(|w| &w.chat != chat);
                self.sessions.retain(|s| &s.chat != chat);
                return;
            }
            Effect::Committed(id) => (id, ChangeOutcome::Committed),
            Effect::Reverted(id) => (id, ChangeOutcome::Reverted),
        };
        let found = self
            .changes
            .iter_mut()
            .find(|c| &c.chat == chat && c.id == *id);
        if let Some(changes) = found {
            changes.outcome = outcome;
        }
    }

    /// The record of a run that Commit or Revert names.
    pub fn changes_of(&self, chat: &ChatId, id: MessageId) -> Option<&RunChanges> {
        self.changes.iter().find(|c| &c.chat == chat && c.id == id)
    }

    pub fn worktree_of(&self, chat: &ChatId) -> Option<&ChatWorktree> {
        self.worktrees.iter().find(|w| &w.chat == chat)
    }

    /// A worktree that is gone makes way for a new one.
    pub fn branch_plan(&self, chat: &ChatId) -> BranchPlan {
        if let Some(worktree) = self.worktree_of(chat) {
            return BranchPlan::Use(worktree.clone());
        }
        match self.own_branch.get(chat) {
            Some(name) => BranchPlan::Make { name: name.clone() },
            None => BranchPlan::Plain,
        }
    }

    /// Keeps the worktree that a run made, or forgets one that is gone.
    pub fn set_worktree(&mut self, chat: &ChatId, worktree: Option<ChatWorktree>) {
        self.worktrees.retain(|w| &w.chat != chat);
        if self.is_deleted(chat) {
            self.cleanups.extend(worktree);
            return;
        }
        self.worktrees.extend(worktree);
    }

    /// The worktrees of deleted chats.
    pub fn take_cleanups(&mut self) -> Vec<ChatWorktree> {
        std::mem::take(&mut self.cleanups)
    }

    /// Keeps the sessions whose folder is in a root, and answers the list request with
    /// one line per session (SPEC.md 9.6).
    pub fn finish_list(
        &mut self,
        job: &Job,
        found: Result<Vec<(String, SessionInfo)>, String>,
        now: u32,
    ) {
        self.activity.end(&job.chat, job.id);
        self.end_run(&job.chat);
        let found = match found {
            Ok(found) => found,
            Err(e) => {
                self.set_record(&job.token, &job.chat, job.id, Status::Error, e);
                return;
            }
        };
        let mut listed: Vec<Listed> = found
            .into_iter()
            .filter_map(|(agent, info)| self.to_listed(agent, info))
            .collect();
        listed.sort_by_key(|l| std::cmp::Reverse(l.updated));
        listed.truncate(MAX_LISTED);
        self.listed = listed;
        let text = self.list_text(now);
        self.set_record(&job.token, &job.chat, job.id, Status::Done, text);
    }

    fn to_listed(&self, agent: String, info: SessionInfo) -> Option<Listed> {
        if !flags::is_session_id(&info.id) {
            return None;
        }
        let folders = &self.policy.folders;
        let resolved = in_roots(folders, std::path::Path::new(&info.cwd))?;
        Some(Listed {
            agent,
            id: SessionId::from(info.id),
            folder: text(&relative_folder(&folders.base, &resolved)),
            cwd: text(&native_folder(resolved, cfg!(windows))),
            title: info.title,
            updated: info.updated,
        })
    }

    /// Tab-separated: agent, session, age in seconds, 1 if active, the chat that has
    /// it or nothing, the folder, the name of the folder, and the title.
    fn list_text(&self, now: u32) -> String {
        let mut lines = Vec::new();
        for l in &self.listed {
            let chat = self.sessions.iter().find(|s| s.id == l.id);
            let age = now.saturating_sub(l.updated);
            let name = l
                .cwd
                .rsplit(['/', '\\'])
                .find(|p| !p.is_empty())
                .unwrap_or("");
            let fields = [
                l.agent.clone(),
                l.id.to_string(),
                age.to_string(),
                if age < ACTIVE_FOR { "1" } else { "0" }.to_owned(),
                chat.map_or(String::new(), |s| s.chat.to_string()),
                field(&l.folder),
                field(name),
                field(cut_chars(&l.title, MAX_TITLE)),
            ];
            lines.push(fields.join("\t"));
        }
        lines.join("\n")
    }

    /// Answers a folder list with the folder tree (SPEC.md 9.9).
    pub fn finish_folders(&mut self, job: &Job, snapshot: &Snapshot) {
        self.activity.end(&job.chat, job.id);
        self.end_run(&job.chat);
        let text = folder_reply(&self.browse_area(), snapshot);
        self.set_record(&job.token, &job.chat, job.id, Status::Done, text);
    }

    /// Answers a settings list with the values of the bridge (SPEC.md 13.1).
    pub fn finish_settings(
        &mut self,
        job: &Job,
        settings: &BridgeSettings,
        rules: &[RuleLine],
        hooks: &[HookLine],
    ) {
        self.end_run(&job.chat);
        let text = settings_reply(settings, &self.policy, rules, hooks);
        self.set_record(&job.token, &job.chat, job.id, Status::Done, text);
    }

    /// Puts the record of a message at the newest place with its new state. The history
    /// of a restore keeps the text without the blocks of the bridge.
    fn set_record(
        &mut self,
        token: &str,
        chat: &ChatId,
        id: MessageId,
        status: Status,
        text: String,
    ) {
        let history = without_blocks(&text);
        self.record_with_history(token, chat, id, status, text, &history);
    }

    /// `history` is the text that a restore brings back (SPEC.md 7.6).
    fn record_with_history(
        &mut self,
        token: &str,
        chat: &ChatId,
        id: MessageId,
        status: Status,
        text: String,
        history: &str,
    ) {
        let speaker = match status {
            Status::Working => None,
            Status::Done => Some(Speaker::Agent),
            Status::Error => Some(Speaker::Error),
        };
        if let Some(speaker) = speaker {
            self.history.add_reply(chat, speaker, id, history);
        }
        self.lane.set_record(token, chat, id, status, text);
    }

    pub fn to_state(&self) -> State {
        State {
            lane: self.lane.to_state(),
            waiting: self.waiting_jobs(),
            history: self.history.clone(),
            restore_for: self.restore_for.clone(),
            sessions: self.sessions.clone(),
            frames: self.frames.clone(),
            accounts: self.accounts.clone(),
            own_branch: self.own_branch.clone().into_iter().collect(),
            worktrees: self.worktrees.clone(),
            changes: self.changes.clone(),
        }
    }

    /// Oldest first, so a restart keeps the order of arrival.
    fn waiting_jobs(&self) -> Vec<Job> {
        let mut waiting: Vec<&Queued> = self.jobs.values().collect();
        waiting.sort_by_key(|queued| queued.arrival);
        waiting
            .into_iter()
            .map(|queued| queued.job.clone())
            .collect()
    }

    /// A run that was in progress at the stop ends as an error. It never runs again:
    /// it can have changed files already.
    pub fn from_state(policy: Policy, state: State) -> Relay {
        let mut relay = Relay::new(policy);
        relay.lane = Lane::from_state(App::Relay, state.lane);
        relay.history = state.history;
        relay.restore_for = state.restore_for;
        relay.accounts = state.accounts;
        relay.sessions = state.sessions;
        relay.frames = state.frames;
        relay.own_branch = state.own_branch.into_iter().collect();
        relay.worktrees = state.worktrees;
        relay.changes = state.changes;
        for job in state.waiting {
            let queue = relay
                .queues
                .entry(job.chat.clone())
                .or_insert(ChatQueue { ids: Vec::new() });
            queue.ids.push(job.id.0);
            let arrival = relay.arrivals;
            relay.arrivals += 1;
            relay
                .jobs
                .insert((job.chat.clone(), job.id), Queued { arrival, job });
        }
        let jobs = &relay.jobs;
        let ended = relay
            .lane
            .end_working(|chat, id| jobs.contains_key(&(chat.clone(), id)), RESTARTED);
        for (chat, id) in ended {
            relay
                .history
                .add_reply(&chat, Speaker::Error, id, RESTARTED);
        }
        relay
    }

    /// An empty token matches no addon, so the file stays harmless with no restore.
    pub fn restore_file(&self) -> Vec<u8> {
        let Some(token) = &self.restore_for else {
            return restore_body(App::Relay, b"", &[]);
        };
        restore_body(
            App::Relay,
            token.as_bytes(),
            &prepare_restore(&self.history.to_restore()),
        )
    }

    pub fn body(&self, now: u32) -> Vec<u8> {
        self.lane.body(now)
    }
}

/// The addon cannot tell this note from agent text, so it is for the player only.
fn with_level_note(job: &Job, text: String) -> String {
    if job.work != Work::Prompt || job.permission >= job.asked {
        return text;
    }
    let level = job.permission.word();
    format!("(Ran at {level}, the most that config.toml allows.)\n\n{text}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::live::no_notices;

    const NOW: u32 = 1_790_211_079;

    fn policy() -> Policy {
        Policy {
            folders: Folders {
                roots: vec![b"/home/x/Code".to_vec()],
                base: b"/home/x/Code".to_vec(),
            },
            agents: [
                ("claude".to_owned(), Permission::AutoEdit),
                ("codex".to_owned(), Permission::Ask),
            ]
            .into(),
            default_agent: "claude".into(),
        }
    }

    fn relay() -> Relay {
        Relay::new(policy())
    }

    fn record_in(cwd: &str, chat: &str, id: u32, flags: &str, text: &str) -> Record {
        Record {
            token: b"tok".to_vec(),
            chat: chat.as_bytes().to_vec(),
            id,
            cwd: cwd.as_bytes().to_vec(),
            flags: flags.as_bytes().to_vec(),
            name: Vec::new(),
            text: text.as_bytes().to_vec(),
        }
    }

    fn record(chat: &str, id: u32, flags: &str, text: &str) -> Record {
        record_in("", chat, id, flags, text)
    }

    fn body(relay: &Relay) -> String {
        String::from_utf8(relay.body(NOW)).unwrap()
    }

    fn run_all(relay: &mut Relay) -> Vec<Job> {
        let mut jobs = Vec::new();
        while let Some(job) = relay.next_job() {
            relay.finish(&job, Ok(format!("echo: {}", job.text)));
            jobs.push(job);
        }
        jobs
    }

    #[test]
    fn a_message_runs_once_even_when_the_strip_is_read_twice() {
        let mut relay = relay();
        let frame = [record("c1", 1, "agent=codex;n", "hi")];
        assert_eq!(relay.on_frame(&frame, NOW), [Outcome::Accepted]);
        assert_eq!(relay.on_frame(&frame, NOW), [Outcome::Duplicate]);
        let jobs = run_all(&mut relay);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].agent, "codex");
        assert_eq!(jobs[0].session, Session::New);
    }

    fn limited(max_runs: usize) -> Relay {
        let mut relay = relay();
        relay.set_max_runs(max_runs);
        relay
    }

    fn live(relay: &Relay) -> String {
        String::from_utf8(relay.live_file(&no_notices())).unwrap()
    }

    fn ids(jobs: &[Job]) -> Vec<MessageId> {
        jobs.iter().map(|j| j.id).collect()
    }

    #[test]
    fn a_message_over_the_parallel_limit_waits_until_a_run_ends() {
        let mut relay = limited(2);
        relay.on_frame(
            &[
                record("c1", 1, "", "a"),
                record("c2", 2, "", "b"),
                record("c3", 3, "", "c"),
            ],
            NOW,
        );
        let first = relay.next_job().unwrap();
        relay.next_job().unwrap();

        assert!(relay.next_job().is_none());
        relay.finish(&first, Ok(String::new()));
        assert_eq!(relay.next_job().unwrap().id, MessageId(3));
    }

    #[test]
    fn the_default_limit_is_three_parallel_runs() {
        let mut relay = relay();
        let frame: Vec<Record> = (1..=4)
            .map(|id| record(&format!("c{id}"), id, "", "x"))
            .collect();
        relay.on_frame(&frame, NOW);

        let started = (0..4).filter_map(|_| relay.next_job()).count();

        assert_eq!(started, 3);
    }

    #[test]
    fn waiting_messages_start_in_arrival_order_across_chats() {
        let mut relay = limited(1);
        relay.on_frame(&[record("c9", 1, "", "first")], NOW);
        relay.on_frame(&[record("c5", 2, "", "second")], NOW);
        relay.on_frame(&[record("c1", 3, "", "third")], NOW);

        let jobs = run_all(&mut relay);

        assert_eq!(ids(&jobs), [MessageId(1), MessageId(2), MessageId(3)]);
    }

    #[test]
    fn a_list_never_waits_for_the_parallel_limit() {
        let mut relay = limited(1);
        relay.on_frame(
            &[record("c1", 1, "", "busy"), record("c2", 2, "", "waits")],
            NOW,
        );
        relay.next_job().unwrap();
        relay.on_frame(&[record("settings", 3, "list=settings", "")], NOW);

        let next = relay.next_job().unwrap();

        assert_eq!(next.work, Work::ListSettings);
        assert!(relay.next_job().is_none());
    }

    #[test]
    fn a_message_that_waits_for_the_limit_shows_the_running_chats_and_those_ahead() {
        let mut relay = limited(2);
        let frame: Vec<Record> = (1..=4)
            .map(|id| record(&format!("c{id}"), id, "", "x"))
            .collect();
        relay.on_frame(&frame, NOW);
        relay.next_job().unwrap();
        relay.next_job().unwrap();
        assert!(relay.next_job().is_none());

        let changed = relay.show_waiting();

        assert!(changed);
        let live = live(&relay);
        assert!(
            live.contains(r#"id = 3, lines = {"Waiting: 2 other chats are running", }"#),
            "{live}"
        );
        assert!(
            live.contains(
                r#"id = 4, lines = {"Waiting: 2 other chats are running, 1 ahead of this one", }"#
            ),
            "{live}"
        );
        assert!(!relay.show_waiting());
    }

    #[test]
    fn the_waiting_line_goes_when_the_message_starts() {
        let mut relay = limited(1);
        relay.on_frame(&[record("c1", 1, "", "a"), record("c2", 2, "", "b")], NOW);
        let first = relay.next_job().unwrap();
        relay.show_waiting();
        relay.finish(&first, Ok(String::new()));

        let second = relay.next_job().unwrap();
        relay.begin(&second);

        let live = live(&relay);
        assert!(!live.contains("Waiting:"), "{live}");
        assert!(live.contains(r#"lines = {"Level: auto-edit", }"#), "{live}");
    }

    #[test]
    fn a_message_that_waits_only_for_its_own_chat_shows_no_waiting_line() {
        let mut relay = limited(1);
        relay.on_frame(&[record("c1", 1, "", "a"), record("c1", 2, "", "b")], NOW);
        relay.next_job().unwrap();

        relay.show_waiting();

        assert!(!live(&relay).contains("Waiting:"));
    }

    #[test]
    fn stop_ends_a_message_that_waits_for_the_limit_and_its_line() {
        let mut relay = limited(1);
        relay.on_frame(&[record("c1", 1, "", "a"), record("c2", 2, "", "b")], NOW);
        relay.next_job().unwrap();
        relay.show_waiting();

        relay.on_frame(&[record("c2", 0, "stop", "")], NOW);

        assert!(!live(&relay).contains("Waiting:"));
        assert!(body(&relay).contains(r#"id = 2, status = "error", text = "Stopped.""#));
    }

    #[test]
    fn the_arrival_order_survives_a_restart() {
        let mut relay = limited(1);
        relay.on_frame(&[record("c1", 1, "", "running")], NOW);
        relay.next_job().unwrap();
        relay.on_frame(&[record("c9", 2, "", "first")], NOW);
        relay.on_frame(&[record("c5", 3, "", "second")], NOW);

        let mut restarted = restart(&relay);
        restarted.set_max_runs(1);
        restarted.on_frame(&[record("c0", 4, "", "third")], NOW);
        let jobs = run_all(&mut restarted);

        assert_eq!(ids(&jobs), [MessageId(2), MessageId(3), MessageId(4)]);
    }

    #[test]
    fn a_chat_runs_its_messages_in_order_one_at_a_time() {
        let mut relay = relay();
        relay.on_frame(
            &[
                record("c1", 1, "", "a"),
                record("c1", 2, "", "b"),
                record("c2", 3, "", "c"),
            ],
            NOW,
        );
        let first = relay.next_job().unwrap();
        let other = relay.next_job().unwrap();
        assert_eq!((first.id, other.id), (MessageId(1), MessageId(3)));
        assert!(relay.next_job().is_none());
        relay.finish(&first, Ok(String::new()));
        assert_eq!(relay.next_job().unwrap().id, MessageId(2));
    }

    #[test]
    fn a_full_chat_queue_refuses_without_marking_seen() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 100, "", "x")], NOW);
        let _running = relay.next_job().unwrap();
        for id in 0..20 {
            assert_eq!(
                relay.on_frame(&[record("c1", id, "", "x")], NOW + id * 7),
                [Outcome::Accepted]
            );
        }
        assert_eq!(
            relay.on_frame(&[record("c1", 20, "", "x")], NOW + 200),
            [Outcome::Refused]
        );
    }

    #[test]
    fn a_full_body_refuses_a_message_without_marking_it_seen() {
        let mut relay = relay();
        for id in 0..30 {
            relay.on_frame(&[record(&format!("c{id}"), id, "", "x")], NOW + id * 7);
        }
        run_all(&mut relay);
        assert_eq!(
            relay.on_frame(&[record("late", 99, "", "x")], NOW + 300),
            [Outcome::Refused]
        );

        let read: Vec<String> = (0..30).map(|id| id.to_string()).collect();
        let report = format!("next=2;read={}", read.join(","));
        assert_eq!(
            relay.on_frame(&[record("late", 99, &report, "x")], NOW + 301),
            [Outcome::Accepted]
        );
    }

    #[test]
    fn the_rate_limit_refuses_without_marking_seen() {
        let mut relay = relay();
        for id in 0..10 {
            relay.on_frame(&[record(&format!("c{id}"), id, "", "x")], NOW);
        }
        assert_eq!(
            relay.on_frame(&[record("c10", 10, "", "x")], NOW),
            [Outcome::Refused]
        );
        assert_eq!(
            relay.on_frame(&[record("c10", 10, "", "x")], NOW + 60),
            [Outcome::Accepted]
        );
    }

    #[test]
    fn a_duplicate_uses_no_rate() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "x")], NOW);
        for _ in 0..20 {
            relay.on_frame(&[record("c1", 1, "", "x")], NOW);
        }
        assert_eq!(
            relay.on_frame(&[record("c1", 2, "", "x")], NOW),
            [Outcome::Accepted]
        );
    }

    #[test]
    fn a_read_final_reply_leaves_the_body_but_a_working_one_stays() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c2", 2, "", "b")], NOW);
        let job = relay.next_job().unwrap();
        relay.finish(&job, Ok("done".into()));
        relay.on_frame(&[record("relay", 0, "h;read=1,2", "")], NOW);
        assert!(!body(&relay).contains("id = 1,"));
        assert!(body(&relay).contains("id = 2,"));
    }

    #[test]
    fn a_read_list_from_another_token_removes_nothing() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        run_all(&mut relay);
        let mut other = record("relay", 0, "h;read=1", "");
        other.token = b"other".to_vec();
        relay.on_frame(&[other], NOW);
        assert!(body(&relay).contains("id = 1,"));
    }

    #[test]
    fn the_next_flag_moves_the_slot_window() {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 0, "h;next=57", "")], NOW);
        assert_eq!(relay.next_slot(), 57);
    }

    #[test]
    fn a_reload_starts_the_slot_window_at_one() {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 0, "h;next=57", "")], NOW);
        relay.reset_window(Some("tok"));
        assert_eq!(relay.next_slot(), 1);
    }

    #[test]
    fn a_second_token_with_the_same_chat_and_id_waits_for_the_first() {
        let other = || Record {
            token: b"other".to_vec(),
            ..record("c1", 1, "", "from a new token")
        };
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "first")], NOW);
        assert_eq!(relay.on_frame(&[other()], NOW), [Outcome::Refused]);
        relay.on_frame(&[record("c1", 0, "stop", "")], NOW);

        assert_eq!(relay.on_frame(&[other()], NOW), [Outcome::Accepted]);
        assert_eq!(run_all(&mut relay).len(), 1);
        assert!(!body(&relay).contains("status = \"working\""));
    }

    fn from_token(token: &str, r: Record) -> Record {
        Record {
            token: token.as_bytes().to_vec(),
            ..r
        }
    }

    fn restore_text(relay: &Relay) -> String {
        String::from_utf8(relay.restore_file()).unwrap()
    }

    /// One chat with a finished message from `tok`, then a hello from `new`.
    fn wiped() -> Relay {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 0, "h", "")], NOW);
        relay.on_frame(&[record("c1", 1, "", "before the wipe")], NOW);
        run_all(&mut relay);
        relay.on_frame(&[from_token("new", record("relay", 0, "h", ""))], NOW);
        relay
    }

    #[test]
    fn the_first_token_gets_no_restore() {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 0, "h", "")], NOW);
        assert!(restore_text(&relay).contains("token = \"\""));
    }

    #[test]
    fn a_hello_from_a_new_token_gets_the_chats_back() {
        let relay = wiped();
        let text = restore_text(&relay);
        assert!(text.contains("token = \"new\""));
        assert!(text.contains("text = \"before the wipe\""));
        assert!(text.contains("text = \"\\027M1\\010p\\031echo: before the wipe\\010\""));
    }

    #[test]
    fn a_done_reply_and_its_history_hold_blocks_but_an_error_stays_plain() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        relay.on_frame(&[record("c2", 2, "", "b")], NOW);
        let first = relay.next_job().unwrap();
        let second = relay.next_job().unwrap();
        relay.finish(&first, Ok("**done**".into()));
        relay.finish(&second, Err("no **luck**".into()));

        let state = relay.to_state();
        let texts: Vec<&str> = state.lane.records.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["\x1bM1\np\x1f|cffffd100done|r\n", "no **luck**"]);
        let replies: Vec<Vec<u8>> = relay
            .history
            .to_restore()
            .into_iter()
            .map(|c| c.history[1].text.clone())
            .collect();
        assert_eq!(replies, [texts[0].as_bytes(), texts[1].as_bytes()]);
    }

    #[test]
    fn the_restored_flag_ends_the_restore_and_retires_no_token() {
        let mut relay = wiped();

        relay.on_frame(
            &[from_token("new", record("relay", 0, "h;restored", ""))],
            NOW,
        );

        assert!(restore_text(&relay).contains("token = \"\""));
        assert!(body(&relay).contains("echo: before the wipe"));
    }

    #[test]
    fn a_new_token_in_the_file_of_the_same_account_retires_the_old_token() {
        let mut relay = wiped();
        relay.saw_saved_token("ACCOUNT1", "tok");
        relay.on_frame(&[record("c1", 2, "", "still running")], NOW);
        let job = relay.next_job().unwrap();

        let retired = relay.saw_saved_token("ACCOUNT1", "new");
        relay.finish(&job, Ok("late".into()));

        assert_eq!(retired.as_deref(), Some("tok"));
        assert!(!body(&relay).contains("echo: before the wipe"));
        assert!(!body(&relay).contains("late"));
    }

    #[test]
    fn a_token_in_the_file_of_another_account_retires_nothing() {
        let mut relay = wiped();
        relay.saw_saved_token("ACCOUNT1", "tok");

        let retired = relay.saw_saved_token("ACCOUNT2", "new");

        assert_eq!(retired, None);
        assert!(body(&relay).contains("echo: before the wipe"));
    }

    #[test]
    fn the_same_token_in_a_file_again_retires_nothing() {
        let mut relay = wiped();
        relay.saw_saved_token("ACCOUNT1", "tok");

        let retired = relay.saw_saved_token("ACCOUNT1", "tok");

        assert_eq!(retired, None);
        assert!(body(&relay).contains("echo: before the wipe"));
    }

    #[test]
    fn two_accounts_that_play_at_once_both_keep_their_replies() {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 0, "h;next=40", "")], NOW);
        relay.on_frame(
            &[from_token("two", record("relay", 0, "h;next=5", ""))],
            NOW,
        );
        relay.saw_saved_token("ACCOUNT1", "tok");
        relay.saw_saved_token("ACCOUNT2", "two");
        relay.on_frame(
            &[from_token("two", record("relay", 0, "h;restored", ""))],
            NOW,
        );

        relay.on_frame(&[record("c1", 1, "", "from one")], NOW);
        relay.on_frame(&[from_token("two", record("c2", 2, "", "from two"))], NOW);
        run_all(&mut relay);

        assert!(body(&relay).contains("echo: from one"));
        assert!(body(&relay).contains("echo: from two"));
        assert_eq!(relay.next_slots(), [5, 40]);
    }

    #[test]
    fn the_account_of_each_token_survives_a_restart() {
        let mut relay = wiped();
        relay.saw_saved_token("ACCOUNT1", "tok");

        let mut restarted = restart(&relay);
        let retired = restarted.saw_saved_token("ACCOUNT1", "new");

        assert_eq!(retired.as_deref(), Some("tok"));
    }

    #[test]
    fn a_restore_goes_on_after_a_bridge_restart() {
        let relay = wiped();
        assert!(restore_text(&restart(&relay)).contains("token = \"new\""));
    }

    #[test]
    fn a_message_runs_at_the_lower_of_the_config_and_the_game_level() {
        let mut relay = relay();
        relay.on_frame(
            &[
                record("c1", 1, "level=full-auto", "raise"),
                record("c2", 2, "level=ask", "lower"),
                record("c3", 3, "agent=codex;level=auto-edit", "raise codex"),
            ],
            NOW,
        );
        let levels: Vec<Permission> = run_all(&mut relay).iter().map(|j| j.permission).collect();
        assert_eq!(
            levels,
            [Permission::AutoEdit, Permission::Ask, Permission::Ask]
        );
    }

    #[test]
    fn a_lowered_run_shows_its_level_first_in_the_live_file_and_in_its_reply() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "agent=codex;level=auto-edit", "hi")], NOW);
        let job = relay.next_job().unwrap();
        relay.begin(&job);
        let live = String::from_utf8(relay.live_file(&no_notices())).unwrap();
        assert!(
            live.contains(r#"lines = {"Level: ask (config)", }"#),
            "{live}"
        );
        relay.finish(&job, Ok("done".into()));
        assert!(body(&relay).contains("(Ran at ask, the most that config.toml allows.)"));
    }

    #[test]
    fn a_run_at_the_level_that_it_asked_for_has_no_note() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "level=auto-edit", "hi")], NOW);
        let job = relay.next_job().unwrap();
        relay.begin(&job);
        let live = String::from_utf8(relay.live_file(&no_notices())).unwrap();
        assert!(live.contains(r#"lines = {"Level: auto-edit", }"#), "{live}");
        relay.finish(&job, Ok("done".into()));
        assert!(!body(&relay).contains("Ran at"));
    }

    #[test]
    fn an_agent_that_is_not_in_the_config_never_runs() {
        let mut relay = relay();
        let outcomes = relay.on_frame(&[record("c1", 1, "agent=gemini", "hi")], NOW);
        assert_eq!(outcomes, [Outcome::BadAgent]);
        assert!(run_all(&mut relay).is_empty());
        assert!(body(&relay).contains(BAD_AGENT));
    }

    #[test]
    fn each_error_text_of_the_relay_says_what_to_do_next() {
        assert!(BAD_FOLDER.contains("Pick another one"), "{BAD_FOLDER}");
        assert!(
            BAD_AGENT.contains("Pick another one in Settings"),
            "{BAD_AGENT}"
        );
        // The Resend link next to the error is the next step of RESTARTED.
        assert_eq!(RESTARTED, "Stopped: the desktop app restarted.");
    }

    #[test]
    fn the_build_counts_as_good_only_when_both_channels_work() {
        let mut relay = relay();
        relay.on_frame(
            &[record("relay", 0, "h;build=70009;out=shot;in=missing", "")],
            NOW,
        );
        assert_eq!(relay.client_build(), None);
        relay.on_frame(
            &[record("relay", 0, "h;build=70009;out=shot;in=slots", "")],
            NOW,
        );
        assert_eq!(relay.client_build(), Some("70009"));
        relay.on_frame(
            &[record("relay", 0, "h;build=70100;out=fail;in=slots", "")],
            NOW,
        );
        assert_eq!(restart(&relay).client_build(), Some("70009"));
    }

    fn first_run(relay: &mut Relay) {
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        relay.keep_session(&job, Some("s9".into()));
        relay.finish(&job, Ok(String::new()));
    }

    #[test]
    fn the_next_message_of_a_chat_resumes_its_agent_session() {
        let mut relay = relay();
        first_run(&mut relay);
        relay.on_frame(&[record("c1", 2, "", "b")], NOW);
        assert_eq!(relay.next_job().unwrap().resume_id(), Some("s9"));
    }

    #[test]
    fn a_new_session_flag_another_agent_or_a_restart_keeps_the_rules() {
        let mut relay = relay();
        first_run(&mut relay);
        relay.on_frame(&[record("c1", 2, "n", "fresh")], NOW);
        assert_eq!(run_all(&mut relay)[0].resume, None);
        relay.on_frame(&[record("c1", 3, "agent=codex", "other agent")], NOW);
        assert_eq!(run_all(&mut relay)[0].resume, None);

        let mut restarted = restart(&relay);
        restarted.on_frame(&[record("c1", 4, "", "after restart")], NOW);
        assert_eq!(restarted.next_job().unwrap().resume_id(), Some("s9"));
    }

    #[test]
    fn a_rule_removal_from_the_settings_tab_is_taken_once_and_starts_no_run() {
        let mut relay = relay();
        relay.on_frame(&[record("settings", 1, "rule=remove:a1b2", "")], NOW);
        assert_eq!(relay.take_rule_removals(), ["a1b2".to_owned()]);
        assert!(relay.take_rule_removals().is_empty());
        assert!(relay.next_job().is_none());
    }

    #[test]
    fn stop_signals_the_run_in_progress_once() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "long task")], NOW);
        relay.next_job().unwrap();
        relay.on_frame(&[record("c1", 0, "stop", "")], NOW);
        assert_eq!(relay.take_cancels(), [ChatId::new("c1")]);
        assert!(relay.take_cancels().is_empty());
    }

    /// A control record has id 0, so the replay store of messages cannot tell two stops
    /// apart. The tag of the frame can.
    #[test]
    fn a_replayed_frame_does_not_stop_a_later_run() {
        let mut relay = relay();
        let stop = [record("c1", 0, "stop", "")];
        relay.on_frame(&[record("c1", 1, "", "first")], NOW);
        let first = relay.next_job().unwrap();
        relay.on_tagged_frame([7; 8], &stop, NOW);
        relay.take_cancels();
        relay.finish(&first, Ok("done".into()));
        relay.on_frame(&[record("c1", 2, "", "second")], NOW + 5);
        relay.next_job().unwrap();

        let replayed = relay.on_tagged_frame([7; 8], &stop, NOW + 10);

        assert_eq!(replayed, [Outcome::Duplicate]);
        assert!(relay.take_cancels().is_empty());
    }

    #[test]
    fn a_replayed_frame_does_not_delete_a_chat_again_or_answer_again() {
        let mut relay = relay();
        let frame = [
            record("c1", 0, "delete", ""),
            record("c2", 0, "perm=p1:o1:0123456789abcdef", ""),
            record("s", 0, "rule=remove:r1", ""),
        ];
        relay.on_tagged_frame([7; 8], &frame, NOW);
        relay.take_rule_removals();

        let replayed = relay.on_tagged_frame([7; 8], &frame, NOW + 1);

        assert_eq!(
            replayed,
            [Outcome::Duplicate, Outcome::Duplicate, Outcome::Duplicate]
        );
        assert!(relay.take_rule_removals().is_empty());
    }

    #[test]
    fn a_replayed_frame_gives_a_refused_message_its_next_chance() {
        let mut relay = relay();
        for id in 1..=10 {
            relay.on_frame(&[record("c1", id, "", "x")], NOW);
        }
        let frame = [record("c1", 11, "", "late")];
        assert_eq!(
            relay.on_tagged_frame([7; 8], &frame, NOW),
            [Outcome::Refused]
        );

        let replayed = relay.on_tagged_frame([7; 8], &frame, NOW + 61);

        assert_eq!(replayed, [Outcome::Accepted]);
    }

    #[test]
    fn a_frame_tag_is_forgotten_when_the_frame_is_too_old_to_pass() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "long task")], NOW);
        relay.next_job().unwrap();
        let stop = [record("c1", 0, "stop", "")];
        relay.on_tagged_frame([7; 8], &stop, NOW);
        relay.take_cancels();

        relay.on_tagged_frame([7; 8], &stop, NOW + 361);

        assert_eq!(relay.take_cancels(), [ChatId::new("c1")]);
    }

    fn open_notice() -> Notice {
        Notice {
            id: "a1b2c3d4e5f6".into(),
            prompted: crate::desktop::Prompted::Dialog,
            waiting: crate::desktop::Waiting::Open,
            topic: crate::desktop::Topic::Action,
        }
    }

    #[test]
    fn a_new_message_while_a_run_waits_for_the_desktop_interrupts_the_run() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "read my key")], NOW);
        let job = relay.next_job().unwrap();
        relay.desktop(&job.chat, job.id, open_notice());

        assert_eq!(
            relay.on_frame(&[record("c1", 2, "", "no, do this")], NOW),
            [Outcome::Accepted]
        );

        assert_eq!(relay.take_interrupts(), [ChatId::new("c1")]);
        assert!(relay.take_cancels().is_empty(), "no Stop");
    }

    #[test]
    fn a_new_message_while_a_run_waits_for_the_game_interrupts_the_run() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "clean up")], NOW);
        let job = relay.next_job().unwrap();
        relay.ask(&job.chat, job.id, b"rm -rf build".to_vec(), Vec::new(), NOW);

        relay.on_frame(&[record("c1", 2, "", "no, do this")], NOW);

        assert_eq!(relay.take_interrupts(), [ChatId::new("c1")]);
    }

    #[test]
    fn a_new_message_while_a_run_only_works_waits_in_the_queue() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "long task")], NOW);
        let job = relay.next_job().unwrap();
        relay.step(&job.chat, job.id, "edit a.rs".into());
        let mut ended = open_notice();
        ended.waiting = crate::desktop::Waiting::Approved;
        relay.desktop(&job.chat, job.id, ended);

        relay.on_frame(&[record("c1", 2, "", "and then this")], NOW);

        assert!(relay.take_interrupts().is_empty());
        assert!(relay.next_job().is_none(), "the run goes on");
    }

    #[test]
    fn a_duplicate_a_refused_message_or_another_chat_never_interrupts() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "read my key")], NOW);
        let job = relay.next_job().unwrap();
        relay.desktop(&job.chat, job.id, open_notice());

        relay.on_frame(&[record("c1", 1, "", "read my key")], NOW);
        relay.on_frame(&[record("c2", 3, "", "other chat")], NOW);
        relay.on_frame(&[record("c1", 0, "list", "")], NOW);
        assert!(relay.take_interrupts().is_empty());

        for id in 100..130 {
            let at = NOW + (id - 100) * 7;
            relay.on_frame(&[record(&format!("x{id}"), id, "", "x")], at);
        }
        run_all(&mut relay);
        let refused = relay.on_frame(&[record("c1", 99, "", "late")], NOW + 400);
        assert_eq!(refused, [Outcome::Refused]);
        assert!(relay.take_interrupts().is_empty());
    }

    #[test]
    fn after_an_interrupt_the_new_message_runs_next_and_resumes_the_session() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "n", "read my key")], NOW);
        let job = relay.next_job().unwrap();
        relay.desktop(&job.chat, job.id, open_notice());
        relay.on_frame(&[record("c1", 2, "", "no, do this")], NOW);
        relay.take_interrupts();

        relay.keep_session(&job, Some("s1".into()));
        relay.finish(&job, Err("Stopped.".into()));

        let next = relay.next_job().unwrap();
        assert_eq!(next.id, MessageId(2));
        assert_eq!(next.resume_id(), Some("s1"));
    }

    #[test]
    fn stop_with_no_run_in_progress_signals_nothing() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 0, "stop", "")], NOW);
        assert!(relay.take_cancels().is_empty());
    }

    #[test]
    fn the_report_gives_the_addon_version() {
        let mut relay = relay();
        assert_eq!(relay.addon_version(), None);
        relay.on_frame(&[record("relay", 0, "h;ver=1", "")], NOW);
        assert_eq!(relay.addon_version(), Some(1));
        relay.on_frame(&[record("relay", 0, "h", "")], NOW);
        assert_eq!(
            relay.addon_version(),
            Some(1),
            "a report with no version keeps the last one"
        );
    }

    #[test]
    fn a_relay_addon_newer_than_the_bridge_gets_the_update_text_and_never_runs() {
        let mut relay = relay();
        let frame = [record("relay", 0, "h;ver=2", ""), record("c1", 1, "", "hi")];
        let outcomes = relay.on_frame(&frame, NOW);
        assert_eq!(outcomes, [Outcome::Control, Outcome::WrongVersion]);
        assert!(relay.next_job().is_none());
        let body = String::from_utf8(relay.body(NOW)).unwrap();
        assert!(body.contains(crate::story::UPDATE_BRIDGE), "{body}");
    }

    #[test]
    fn a_relay_addon_older_than_the_bridge_is_updated_in_the_curseforge_app() {
        let mut relay = relay();
        let frame = [record("relay", 0, "h;ver=0", ""), record("c1", 1, "", "hi")];
        assert_eq!(relay.on_frame(&frame, NOW)[1], Outcome::WrongVersion);
        let body = String::from_utf8(relay.body(NOW)).unwrap();
        assert!(body.contains(crate::relay_addon::UPDATE_ADDON), "{body}");
    }

    #[test]
    fn a_relay_addon_with_a_supported_or_no_version_runs() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "no version yet")], NOW);
        relay.on_frame(
            &[record("relay", 0, "h;ver=1", ""), record("c2", 2, "", "hi")],
            NOW,
        );
        assert!(relay.next_job().is_some());
        assert!(relay.next_job().is_some());
    }

    fn restart(relay: &Relay) -> Relay {
        Relay::from_state(policy(), relay.to_state())
    }

    #[test]
    fn a_frame_seen_before_a_restart_does_not_stop_a_later_run() {
        let mut relay = relay();
        let stop = [record("c1", 0, "stop", "")];
        relay.on_frame(&[record("c1", 1, "", "first")], NOW);
        relay.next_job().unwrap();
        relay.on_tagged_frame([7; 8], &stop, NOW);
        let saved = serde_json::to_string(&relay.to_state()).unwrap();
        let mut relay = Relay::from_state(policy(), serde_json::from_str(&saved).unwrap());
        relay.on_frame(&[record("c1", 2, "", "second")], NOW + 5);
        relay.next_job().unwrap();

        let replayed = relay.on_tagged_frame([7; 8], &stop, NOW + 10);

        assert_eq!(replayed, [Outcome::Duplicate]);
        assert!(relay.take_cancels().is_empty());
    }

    #[test]
    fn a_message_that_ran_before_a_restart_never_runs_again() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "once")], NOW);
        run_all(&mut relay);

        let mut restarted = restart(&relay);
        assert_eq!(
            restarted.on_frame(&[record("c1", 1, "", "once")], NOW),
            [Outcome::Duplicate]
        );
        assert!(body(&restarted).contains("echo: once"));
        assert!(run_all(&mut restarted).is_empty());
    }

    #[test]
    fn a_restart_ends_the_run_in_progress_as_an_error_and_keeps_the_queue() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c1", 2, "", "b")], NOW);
        relay.next_job().unwrap();
        relay.on_frame(&[record("relay", 0, "h;next=57", "")], NOW);

        let mut restarted = restart(&relay);
        assert!(body(&restarted).contains(RESTARTED));
        assert_eq!(restarted.next_slot(), 57);
        let jobs = run_all(&mut restarted);
        assert_eq!(
            jobs.iter().map(|j| j.id).collect::<Vec<_>>(),
            [MessageId(2)]
        );
    }

    #[test]
    fn stop_ends_the_waiting_messages_of_a_chat_as_errors() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c1", 2, "", "b")], NOW);
        let _running = relay.next_job().unwrap();
        relay.on_frame(&[record("c1", 0, "stop", "")], NOW);
        assert!(relay.next_job().is_none());
        assert!(body(&relay).contains(r#"id = 2, status = "error", text = "Stopped.""#));
    }

    #[test]
    fn delete_drops_the_replies_the_session_and_the_history_of_the_chat() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c2", 2, "", "b")], NOW);
        let jobs = run_all(&mut relay);
        relay.keep_session(&jobs[0], Some("s1".into()));

        relay.on_frame(&[record("c1", 0, "d", "")], NOW);

        assert!(
            !body(&relay).contains("echo: a"),
            "no reply of c1 is left to block the body"
        );
        assert!(body(&relay).contains("echo: b"));
        assert_eq!(relay.unread(), 1);
        let state = relay.to_state();
        assert!(state.sessions.is_empty());
        assert!(state.history.to_restore().iter().all(|c| c.id != b"c1"));
    }

    #[test]
    fn a_run_of_a_deleted_chat_ends_with_no_reply_and_no_session() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c1", 2, "", "b")], NOW);
        let running = relay.next_job().unwrap();

        relay.on_frame(&[record("c1", 0, "d", "")], NOW);
        assert_eq!(relay.take_cancels(), [ChatId::new("c1")]);
        relay.keep_session(&running, Some("s1".into()));
        relay.finish(&running, Err("Stopped.".into()));

        assert_eq!(relay.unread(), 0);
        assert!(relay.to_state().sessions.is_empty());
        assert!(relay.next_job().is_none(), "the waiting message never runs");
    }

    fn info(id: &str, cwd: &str, title: &str, updated: u32) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            cwd: cwd.into(),
            title: title.into(),
            updated,
        }
    }

    /// Runs a list request of chat `relay` with what the agents found.
    fn list(relay: &mut Relay, id: u32, found: Vec<(String, SessionInfo)>) -> String {
        relay.on_frame(&[record("relay", id, "list", "")], NOW);
        let job = relay.next_job().unwrap();
        assert_eq!(job.work, Work::ListSessions);
        relay.finish_list(&job, Ok(found), NOW);
        body(relay)
    }

    #[test]
    fn a_list_shows_the_sessions_in_the_roots_newest_first() {
        let mut relay = relay();
        list(
            &mut relay,
            1,
            vec![
                (
                    "claude".into(),
                    info("old", "/home/x/Code/app", "Old work", NOW - 7200),
                ),
                (
                    "claude".into(),
                    info("new", "/home/x/Code/app", "New work", NOW - 60),
                ),
                ("claude".into(), info("ssh", "/home/x/.ssh", "Keys", NOW)),
                ("codex".into(), info("bad;id", "/home/x/Code", "Bad", NOW)),
            ],
        );
        assert_eq!(
            relay.list_text(NOW),
            "claude\tnew\t60\t1\t\tapp\tapp\tNew work\n\
             claude\told\t7200\t0\t\tapp\tapp\tOld work",
            "a session outside the roots or with a bad id never shows"
        );
    }

    #[test]
    fn a_title_with_line_breaks_stays_on_one_line_and_is_cut() {
        let mut relay = relay();
        let title = format!("a\tb\nc{}", "é".repeat(80));
        list(
            &mut relay,
            1,
            vec![("claude".into(), info("s1", "/home/x/Code", &title, NOW))],
        );
        let text = relay.list_text(NOW);
        assert_eq!(text.lines().count(), 1);
        let shown = text.rsplit('\t').next().unwrap();
        assert!(shown.starts_with("a b c"));
        assert!(shown.len() <= MAX_TITLE);
    }

    #[test]
    fn a_failed_list_publishes_the_error() {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 1, "list", "")], NOW);
        let job = relay.next_job().unwrap();
        relay.finish_list(&job, Err("agent crashed".into()), NOW);
        assert!(body(&relay).contains(r#"status = "error", text = "agent crashed""#));
    }

    #[test]
    fn an_attach_continues_a_listed_session_and_later_messages_resume_it() {
        let mut relay = relay();
        list(
            &mut relay,
            1,
            vec![(
                "codex".into(),
                info("s1", "/home/x/Code/app", "Work", NOW - 3600),
            )],
        );

        relay.on_frame(&[record("c9", 2, "attach=s1", "")], NOW);
        let attach = relay.next_job().unwrap();
        assert_eq!(attach.agent, "codex");
        assert_eq!(attach.cwd, "/home/x/Code/app");
        assert_eq!(
            attach.work,
            Work::Attach {
                session: "s1".into(),
                open: Open::Same
            }
        );
        relay.keep_session(&attach, Some("s1".into()));
        relay.finish(&attach, Ok("last exchange".into()));

        relay.on_frame(&[record_in("app", "c9", 3, "agent=codex", "go on")], NOW);
        let next = relay.next_job().unwrap();
        assert_eq!(next.resume_id(), Some("s1"));
    }

    #[test]
    fn an_attach_to_an_active_session_forks_it() {
        let mut relay = relay();
        list(
            &mut relay,
            1,
            vec![(
                "claude".into(),
                info("s1", "/home/x/Code", "Work", NOW - 10),
            )],
        );
        relay.on_frame(&[record("c9", 2, "attach=s1", "")], NOW);
        let job = relay.next_job().unwrap();
        assert_eq!(
            job.work,
            Work::Attach {
                session: "s1".into(),
                open: Open::Fork
            }
        );
    }

    #[test]
    fn an_attach_keeps_the_fork_flag_of_older_state_files() {
        let work = Work::Attach {
            session: "s1".into(),
            open: Open::Fork,
        };
        let json = serde_json::to_string(&work).unwrap();
        assert_eq!(json, r#"{"Attach":{"session":"s1","fork":true}}"#);
        let old = r#"{"Attach":{"session":"s1","fork":false}}"#;
        let loaded: Work = serde_json::from_str(old).unwrap();
        assert_eq!(
            loaded,
            Work::Attach {
                session: "s1".into(),
                open: Open::Same
            }
        );
    }

    #[test]
    fn an_attach_to_a_session_that_the_list_did_not_show_gets_an_error() {
        let mut relay = relay();
        let outcomes = relay.on_frame(&[record("c9", 2, "attach=s1", "")], NOW);
        assert_eq!(outcomes, [Outcome::BadSession]);
        assert!(relay.next_job().is_none());
        assert!(body(&relay).contains(NO_SESSION));
    }

    #[test]
    fn a_listed_session_of_a_chat_names_that_chat() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        relay.keep_session(&job, Some("s1".into()));
        relay.finish(&job, Ok("done".into()));

        list(
            &mut relay,
            2,
            vec![(
                "claude".into(),
                info("s1", "/home/x/Code", "Work", NOW - 3600),
            )],
        );
        assert!(relay.list_text(NOW).contains("\t0\tc1\t"));
    }

    fn snapshot(paths: &[(&str, Option<usize>)]) -> Snapshot {
        let folders = paths
            .iter()
            .map(|(path, parent)| crate::folder_walk::Folder {
                path: (*path).into(),
                parent: *parent,
                repo: false,
            })
            .collect();
        Snapshot {
            folders,
            complete: true,
            home: None,
        }
    }

    /// Runs a folder list request of chat `folders` with what the walk found.
    fn list_folders(relay: &mut Relay, id: u32, found: &Snapshot) -> String {
        relay.on_frame(&[record("folders", id, "list=folders", "")], NOW);
        let job = relay.next_job().unwrap();
        assert_eq!(job.work, Work::ListFolders);
        relay.finish_folders(&job, found);
        let state = relay.to_state();
        let reply = state.lane.records.iter().find(|r| r.id == MessageId(id));
        reply.unwrap().text.clone()
    }

    #[test]
    fn a_folder_list_answers_with_the_folder_tree() {
        let mut relay = relay();
        let found = snapshot(&[("/home/x/Code", None), ("/home/x/Code/app", Some(0))]);
        let text = list_folders(&mut relay, 1, &found);
        assert_eq!(text, "/home/x/Code\n0\t/home/x/Code\t\n1\tapp\t");
    }

    #[test]
    fn a_folder_of_the_tree_comes_back_as_the_folder_of_a_new_chat() {
        let mut relay = relay();
        let found = snapshot(&[("/home/x/Code", None), ("/home/x/Code/app", Some(0))]);
        list_folders(&mut relay, 1, &found);
        relay.on_frame(&[record_in("app", "c1", 2, "n", "hi")], NOW);
        assert_eq!(relay.next_job().unwrap().cwd, "/home/x/Code/app");
    }

    #[test]
    fn a_settings_list_answers_with_the_values_of_the_bridge() {
        let mut relay = relay();
        relay.on_frame(&[record("settings", 5, "list=settings", "")], NOW);
        let job = relay.next_job().unwrap();
        assert_eq!(job.work, Work::ListSettings);

        relay.finish_settings(&job, &BridgeSettings::default(), &[], &[]);

        let body = body(&relay);
        assert!(
            body.contains("chat = \"settings\", id = 5, status = \"done\""),
            "{body}"
        );
        assert!(body.contains(r"default_agent\009claude"), "{body}");
        assert!(relay.next_job().is_none());
    }

    #[test]
    fn a_folder_list_leaves_the_session_list_for_attach() {
        let mut relay = relay();
        list(
            &mut relay,
            1,
            vec![("codex".into(), info("s1", "/home/x/Code", "Work", NOW))],
        );
        list_folders(&mut relay, 2, &snapshot(&[]));
        relay.on_frame(&[record("c9", 3, "attach=s1", "")], NOW);
        assert_eq!(relay.next_job().unwrap().agent, "codex");
    }

    #[test]
    fn the_first_message_with_mkdir_asks_the_run_for_a_new_folder() {
        let mut relay = relay();
        relay.on_frame(&[record_in("work/new", "c1", 1, "n;mkdir=1", "hi")], NOW);
        let job = relay.next_job().unwrap();
        assert!(job.new_folder);
        assert_eq!(job.cwd, "/home/x/Code/work/new");
    }

    fn with_home() -> Relay {
        let mut relay = relay();
        relay.take_new_folders(std::path::Path::new("/home/x"));
        relay
    }

    #[test]
    fn a_folder_in_the_home_folder_under_no_root_runs_after_the_desktop_check() {
        let mut relay = with_home();

        relay.on_frame(&[record_in("../lighthouse", "c1", 1, "n", "hi")], NOW);

        assert_eq!(relay.next_job().unwrap().cwd, "/home/x/lighthouse");
    }

    #[test]
    fn with_no_home_folder_a_folder_under_no_root_never_runs() {
        let mut relay = relay();

        relay.on_frame(&[record_in("../lighthouse", "c1", 1, "n", "hi")], NOW);

        assert!(relay.next_job().is_none());
        assert!(body(&relay).contains("That folder isn't allowed."));
    }

    #[test]
    fn the_home_folder_a_folder_above_it_and_a_hidden_folder_end_with_a_reply_and_no_run() {
        let cases = [
            ("..", "your whole home folder"),
            ("../..", "outside your home folder"),
            ("/etc", "outside your home folder"),
            ("../.ssh", "hidden or system folders"),
            ("../app/node_modules", "hidden or system folders"),
        ];
        for (id, (cwd, reason)) in (1..).zip(cases) {
            let mut relay = with_home();

            relay.on_frame(&[record_in(cwd, "c1", id, "n", "hi")], NOW);

            assert!(relay.next_job().is_none(), "{cwd}");
            assert!(body(&relay).contains(reason), "{cwd}: {}", body(&relay));
        }
    }

    #[test]
    fn a_root_that_the_desktop_added_takes_git_actions_and_attaches() {
        let mut relay = with_home();

        relay.add_root(std::path::Path::new("/home/x/lighthouse"));

        assert!(relay.game_folder(b"../lighthouse/src").is_some());
    }

    #[test]
    fn mkdir_on_a_later_message_is_ignored() {
        let mut relay = relay();
        relay.on_frame(&[record_in("work/new", "c1", 1, "mkdir=1", "hi")], NOW);
        assert!(!relay.next_job().unwrap().new_folder);
    }

    #[test]
    fn a_new_folder_with_a_bad_last_part_or_an_absolute_path_never_runs() {
        for cwd in [
            "a/b/..",
            "a/..",
            ".",
            "a/.",
            "/home/x/Code/new",
            "a/b\u{7}",
            "",
        ] {
            let mut relay = relay();
            let outcomes = relay.on_frame(&[record_in(cwd, "c1", 1, "n;mkdir=1", "hi")], NOW);
            assert_eq!(outcomes, [Outcome::BadFolder], "{cwd:?}");
            assert!(relay.next_job().is_none());
            assert!(
                body(&relay).contains("Couldn't create the folder: the name can't contain"),
                "{cwd:?}"
            );
        }
    }

    #[test]
    fn a_new_folder_outside_every_root_never_runs() {
        let mut relay = relay();
        let outcomes = relay.on_frame(&[record_in("../../new", "c1", 1, "n;mkdir=1", "hi")], NOW);
        assert_eq!(outcomes, [Outcome::BadFolder]);
        assert!(body(&relay).contains("That folder isn't allowed"));
    }

    #[test]
    fn a_done_reply_with_a_report_shows_its_usage_line() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        let usage = Usage {
            input: 1234,
            cached: 0,
            output: 350,
            cost_usd: Some(0.04),
        };

        relay.finish_run(&job, Ok("Done.".into()), RunBlocks::default(), Some(&usage));

        assert!(
            body(&relay).contains(r#"text = "\027M1\010u\0311.2k in \194\183 350 out \194\183 $0.04\010p\031Done.\010""#),
            "{}",
            body(&relay)
        );
    }

    #[test]
    fn an_error_reply_shows_no_usage_line() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();

        relay.finish_run(
            &job,
            Err("Stopped.".into()),
            RunBlocks::default(),
            Some(&Usage::default()),
        );

        assert!(body(&relay).contains(r#"status = "error", text = "Stopped.""#));
    }

    #[test]
    fn a_failed_run_publishes_an_error() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        relay.finish(&job, Err("agent crashed".into()));
        assert!(body(&relay).contains(r#"status = "error", text = "agent crashed""#));
    }

    #[test]
    fn a_folder_resolves_against_the_base_and_stays_in_a_root() {
        let mut relay = relay();
        relay.on_frame(
            &[
                record_in("app/../lib", "c1", 1, "", "a"),
                record_in("", "c2", 2, "", "b"),
            ],
            NOW,
        );
        let jobs = run_all(&mut relay);
        assert_eq!(jobs[0].cwd, "/home/x/Code/lib");
        assert_eq!(jobs[1].cwd, "/home/x/Code");
    }

    #[test]
    fn a_folder_outside_every_root_never_runs_and_gets_an_error() {
        let mut relay = relay();
        let outcomes = relay.on_frame(&[record_in("../../.ssh", "c1", 1, "", "a")], NOW);
        assert_eq!(outcomes, [Outcome::BadFolder]);
        assert!(relay.next_job().is_none());
        assert!(
            body(&relay).contains(r#"id = 1, status = "error", text = "That folder isn't allowed"#)
        );
        assert_eq!(
            relay.on_frame(&[record_in("../../.ssh", "c1", 1, "", "a")], NOW),
            [Outcome::Duplicate]
        );
    }

    fn worktree(chat: &str) -> ChatWorktree {
        ChatWorktree {
            chat: ChatId::new(chat),
            repo: "/home/x/Code/app".into(),
            worktree: "/home/x/Code/.gnomish-worktrees/app/fix".into(),
            folder: "/home/x/Code/.gnomish-worktrees/app/fix".into(),
            branch: "gnomish/fix".into(),
            start_branch: Some("main".into()),
            start_commit: "abc".into(),
        }
    }

    fn changes(chat: &str, id: u32) -> RunChanges {
        use crate::run_changes::{ChangeKind, FileChange, Snapshot};
        RunChanges {
            chat: ChatId::new(chat),
            id: MessageId(id),
            top: "/home/x/Code/app".into(),
            start: Snapshot {
                tree: "t1".into(),
                head: None,
            },
            end: Snapshot {
                tree: "t2".into(),
                head: None,
            },
            files: vec![FileChange {
                path: "a.rs".into(),
                lines: Some((3, 1)),
                kind: ChangeKind::Modified,
            }],
            odd_names: false,
            shared: false,
            outcome: ChangeOutcome::Open,
        }
    }

    #[test]
    fn a_git_action_waits_behind_the_run_of_its_chat() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "work")], NOW);
        let running = relay.next_job().unwrap();

        let outcome = relay.on_frame(&[record("c1", 2, "git=commit:1", "fix it")], NOW);

        assert_eq!(outcome, [Outcome::Accepted]);
        assert!(relay.next_job().is_none());
        relay.finish(&running, Ok("done".into()));
        let job = relay.next_job().unwrap();
        assert_eq!(job.work, Work::Git(GitAction::Commit(MessageId(1))));
        assert_eq!(job.text, "fix it");
    }

    #[test]
    fn an_unknown_git_action_is_an_error_and_never_runs() {
        let mut relay = relay();

        let outcome = relay.on_frame(&[record("c1", 1, "git=push", "")], NOW);

        assert_eq!(outcome, [Outcome::BadAction]);
        assert!(relay.next_job().is_none());
        assert!(body(&relay).contains("doesn't know that action"));
    }

    #[test]
    fn a_git_action_never_ends_a_wait_for_an_answer() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "work")], NOW);
        let job = relay.next_job().unwrap();
        relay.ask(&job.chat, job.id, b"rm x".to_vec(), Vec::new(), NOW);

        relay.on_frame(&[record("c1", 2, "git=merge", "")], NOW);

        assert!(relay.take_interrupts().is_empty());
    }

    #[test]
    fn an_own_branch_chat_plans_a_new_branch_then_uses_it() {
        let mut relay = relay();
        let mut first = record("c1", 1, "n;branch=1", "go");
        first.name = b"Fix tests".to_vec();
        relay.on_frame(&[first], NOW);
        let job = relay.next_job().unwrap();

        let plan = relay.branch_plan(&job.chat);
        relay.set_worktree(&job.chat, Some(worktree("c1")));

        assert_eq!(
            plan,
            BranchPlan::Make {
                name: "Fix tests".into()
            }
        );
        assert_eq!(
            relay.branch_plan(&job.chat),
            BranchPlan::Use(worktree("c1"))
        );
        assert_eq!(relay.branch_plan(&ChatId::new("c2")), BranchPlan::Plain);
    }

    #[test]
    fn an_own_branch_resumes_the_session_of_its_worktree() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "branch=1", "a")], NOW);
        let mut job = relay.next_job().unwrap();
        relay.set_worktree(&job.chat, Some(worktree("c1")));
        job.cwd = worktree("c1").folder;
        relay.keep_session(&job, Some(SessionId::from("s1")));
        relay.finish(&job, Ok(String::new()));

        relay.on_frame(&[record("c1", 2, "branch=1", "b")], NOW);

        assert_eq!(relay.next_job().unwrap().resume_id(), Some("s1"));
    }

    #[test]
    fn a_deleted_chat_hands_its_worktree_to_the_cleanup() {
        let mut relay = relay();
        relay.set_worktree(&ChatId::new("c1"), Some(worktree("c1")));

        relay.on_frame(&[record("c1", 0, "d", "")], NOW);

        assert_eq!(relay.take_cleanups(), [worktree("c1")]);
        assert_eq!(relay.branch_plan(&ChatId::new("c1")), BranchPlan::Plain);
    }

    #[test]
    fn a_deleted_chat_keeps_its_worktree_until_its_run_ends() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "branch=1", "a")], NOW);
        let job = relay.next_job().unwrap();
        relay.set_worktree(&job.chat, Some(worktree("c1")));

        relay.on_frame(&[record("c1", 0, "d", "")], NOW);
        let during = relay.take_cleanups();
        relay.finish(&job, Err("Stopped.".into()));

        assert!(during.is_empty());
        assert_eq!(relay.take_cleanups(), [worktree("c1")]);
    }

    #[test]
    fn a_run_reply_carries_the_blocks_right_after_the_marker() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        let run = RunBlocks {
            changes: Some(changes("c1", 1)),
            ..RunBlocks::default()
        };

        relay.finish_run(&job, Ok("Done.".into()), run, None);

        let body = body(&relay);
        assert!(
            body.contains(
                r"\027M1\010G\0311\0313\0311\010F\031a.rs\0313\0311\031M\010p\031Done.\010"
            ),
            "{body}"
        );
        assert!(relay.changes_of(&job.chat, job.id).is_some());
    }

    #[test]
    fn an_error_with_changes_is_rendered_with_its_blocks() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        let run = RunBlocks {
            changes: Some(changes("c1", 1)),
            ..RunBlocks::default()
        };

        relay.finish_run(&job, Err("Stopped.".into()), run, None);

        assert!(body(&relay).contains(r#"status = "error", text = "\027M1\010G"#));
    }

    #[test]
    fn an_error_of_the_agent_never_starts_with_the_marker() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();

        relay.finish(&job, Err("\x1bM1\nG\x1f9\x1f9\x1f9".into()));

        assert!(body(&relay).contains(r#"status = "error", text = "M1\010G"#));
    }

    #[test]
    fn the_restore_history_keeps_a_reply_without_its_blocks() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        let run = RunBlocks {
            changes: Some(changes("c1", 1)),
            ..RunBlocks::default()
        };

        relay.finish_run(&job, Ok("Done.".into()), run, None);

        let state = relay.to_state();
        let restored =
            String::from_utf8(state.history.to_restore()[0].history[1].text.clone()).unwrap();
        assert_eq!(restored, "\x1bM1\np\x1fDone.\n");
    }

    /// The addon shows a restored error as plain text, so the marker and the paragraph
    /// bytes of the renderer must not reach it (SPEC.md 7.3.1).
    #[test]
    fn the_restore_history_keeps_an_error_with_blocks_as_plain_text() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        let run = RunBlocks {
            changes: Some(changes("c1", 1)),
            ..RunBlocks::default()
        };

        relay.finish_run(&job, Err("Stopped.".into()), run, None);

        let state = relay.to_state();
        let restored =
            String::from_utf8(state.history.to_restore()[0].history[1].text.clone()).unwrap();
        assert_eq!(restored, "Stopped.");
    }

    #[test]
    fn a_commit_marks_its_summary_and_a_discard_forgets_the_branch() {
        let mut relay = relay();
        relay.set_worktree(&ChatId::new("c1"), Some(worktree("c1")));
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        let run = RunBlocks {
            changes: Some(changes("c1", 1)),
            ..RunBlocks::default()
        };
        relay.finish_run(&job, Ok("Done.".into()), run, None);
        relay.on_frame(&[record("c1", 2, "git=commit:1", "msg")], NOW);
        let commit = relay.next_job().unwrap();

        relay.finish_git(
            &commit,
            Ok("Committed 1 file.".into()),
            &Effect::Committed(MessageId(1)),
        );
        relay.on_frame(&[record("c1", 3, "git=discard", "")], NOW);
        let discard = relay.next_job().unwrap();
        relay.finish_git(&discard, Ok("Discarded.".into()), &Effect::Discarded);

        let outcome = relay
            .changes_of(&ChatId::new("c1"), MessageId(1))
            .unwrap()
            .outcome;
        assert_eq!(outcome, ChangeOutcome::Committed);
        assert!(relay.worktree_of(&ChatId::new("c1")).is_none());
    }

    #[test]
    fn a_failed_action_changes_no_record() {
        let mut relay = relay();
        relay.set_worktree(&ChatId::new("c1"), Some(worktree("c1")));
        relay.on_frame(&[record("c1", 3, "git=discard", "")], NOW);
        let discard = relay.next_job().unwrap();

        relay.finish_git(&discard, Err("Couldn't discard.".into()), &Effect::Nothing);

        assert!(relay.worktree_of(&ChatId::new("c1")).is_some());
    }

    #[test]
    fn the_bridge_keeps_only_the_last_summaries() {
        let mut relay = relay();
        for id in 0..40 {
            relay.keep_changes(changes("c1", id));
        }

        assert!(relay.changes_of(&ChatId::new("c1"), MessageId(0)).is_none());
        assert!(
            relay
                .changes_of(&ChatId::new("c1"), MessageId(39))
                .is_some()
        );
    }

    #[test]
    fn worktrees_and_summaries_come_back_after_a_restart() {
        let mut relay = relay();
        let mut first = record("c1", 1, "branch=1", "a");
        first.name = b"x".to_vec();
        relay.on_frame(&[first], NOW);
        relay.set_worktree(&ChatId::new("c1"), Some(worktree("c1")));
        relay.keep_changes(changes("c1", 1));

        let again = Relay::from_state(policy(), relay.to_state());

        assert_eq!(again.worktree_of(&ChatId::new("c1")), Some(&worktree("c1")));
        assert!(again.changes_of(&ChatId::new("c1"), MessageId(1)).is_some());
        assert_eq!(
            again.own_branch.get(&ChatId::new("c1")).map(String::as_str),
            Some("x")
        );
    }
}
