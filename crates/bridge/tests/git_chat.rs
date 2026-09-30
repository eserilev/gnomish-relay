//! Git in a chat (SPEC.md 9.10), through the whole bridge: a message from the game, a
//! test agent that edits files, and the real `git` in temp folders.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bridge::agent::{Agent, Control, Run};
use bridge::config::{Permission, Policy};
use bridge::desktop::{Approvals, Prompt, Verdict};
use bridge::folder_path::path_bytes;
use bridge::git_host::{GitHost, UserConfig};
use bridge::ids::hex;
use bridge::raise::Raiser;
use bridge::receive::{KeySet, StripKey};
use bridge::relay::{Folders, Job};
use bridge::run::{Bridge, Paths, now};
use bridge::slots::{BODY_FILE, slot_name};
use common::{install_window, signed_frame};
use protocol::apps::App;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

/// An agent that writes one file into the folder of its run, and tells what it saw.
struct Editor {
    folders: Arc<Mutex<Vec<String>>>,
}

impl Agent for Editor {
    fn run(&self, job: &Job, _control: &Control) -> Run {
        self.folders.lock().unwrap().push(job.cwd.clone());
        fs::write(
            Path::new(&job.cwd).join("agent.txt"),
            format!("{}\n", job.text),
        )
        .unwrap();
        Run {
            reply: Ok("Done.".into()),
            session: None,
        }
    }
}

struct World {
    _root: tempfile::TempDir,
    code: PathBuf,
    repo: PathBuf,
    addons: PathBuf,
    accounts: PathBuf,
    data: PathBuf,
    folders: Arc<Mutex<Vec<String>>>,
    frames: Vec<Vec<u8>>,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let host = GitHost::with_config(UserConfig::Skip).unwrap();
    String::from_utf8(host.bytes(dir, args).unwrap()).unwrap()
}

/// `code/app`, a repository with one commit on `main`, and `code` as the only root.
fn world() -> World {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().canonicalize().unwrap();
    let code = base.join("code");
    let repo = code.join("app");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    fs::write(repo.join("readme.txt"), "hi\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "one"]);
    let addons = base.join("Interface/AddOns");
    let accounts = base.join("WTF/Account");
    let data = base.join("data");
    for dir in [&addons, &accounts, &data, &base.join("Screenshots")] {
        fs::create_dir_all(dir).unwrap();
    }
    install_window(&addons, App::Relay);
    World {
        _root: root,
        code,
        repo,
        addons,
        accounts,
        data,
        folders: Arc::default(),
        frames: Vec::new(),
    }
}

fn bridge(w: &World) -> Bridge {
    let code = path_bytes(&w.code);
    let policy = Policy {
        folders: Folders {
            roots: vec![code.clone()],
            base: code,
        },
        agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
        default_agent: "claude".into(),
    };
    let paths = Paths {
        addons: w.addons.clone(),
        screenshots: w.addons.join("../../Screenshots"),
        accounts: w.accounts.clone(),
        state: w.data.clone(),
        config: w.data.join("config"),
    };
    let editor = Editor {
        folders: Arc::clone(&w.folders),
    };
    let agents = [("claude".to_owned(), Arc::new(editor) as Arc<dyn Agent>)].into();
    let keys = KeySet::new(StripKey::from_hex(&hex(KEY)).unwrap(), None).unwrap();
    Bridge::new(paths, policy, keys, agents)
        .unwrap()
        .with_git(GitHost::with_config(UserConfig::Skip).unwrap())
}

fn with_desktop(bridge: Bridge, w: &World) -> (Bridge, Approvals) {
    let approvals = Approvals::new(&w.data, Prompt::Off);
    let raiser = Raiser {
        approvals: approvals.clone(),
        config_dir: w.data.join("config"),
        home: w.code.clone(),
        permission_timeout: Duration::from_secs(20),
        free_commands: Vec::new(),
    };
    (bridge.with_raises(raiser), approvals)
}

/// Answers the first desktop request as `gnomish-relay approve` or `deny` does, and
/// returns its text.
fn answer_on_the_desktop(
    approvals: Approvals,
    verdict: Verdict,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        for _ in 0..1000 {
            if let Some(open) = approvals.list().first() {
                approvals.answer(&open.id, verdict).unwrap();
                return open.text.clone();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        String::new()
    })
}

/// Sends one message of the chat `c1` from the folder `app`, as a reload outbox does.
fn send(w: &mut World, id: u32, flags: &str, name: &str, text: &str) {
    let payload = format!("tok\x1fc1\x1f{id}\x1fapp\x1f{flags}\x1f{name}\x1f{text}");
    w.frames.push(signed_frame(now(), payload.as_bytes(), KEY));
    let dir = w.accounts.join("ACCOUNT1/SavedVariables");
    fs::create_dir_all(&dir).unwrap();
    let entries: Vec<String> = w
        .frames
        .iter()
        .map(|frame| format!("\t\t{{ [\"frame\"] = \"{}\" }},\n", hex(frame)))
        .collect();
    let text = format!(
        "GnomishRelayDB = {{\n\t[\"outbox\"] = {{\n{}\t}},\n}}\n",
        entries.concat()
    );
    fs::write(dir.join("GnomishRelay.lua"), text).unwrap();
}

fn body(w: &World) -> String {
    fs::read_to_string(w.addons.join(slot_name(App::Relay, 1)).join(BODY_FILE)).unwrap()
}

/// The final record of message `id`, as the slot body holds it.
fn reply(bridge: &mut Bridge, w: &World, id: u32) -> String {
    let done = |text: &str| {
        text.contains(&format!("id = {id}, status = \"done\""))
            || text.contains(&format!("id = {id}, status = \"error\""))
    };
    let found = common::step_until_within(bridge, Duration::from_secs(30), || done(&body(w)));
    assert!(found, "no reply to {id}: {}", body(w));
    let text = body(w);
    let start = text.find(&format!("id = {id}, status")).unwrap();
    text[start..].lines().next().unwrap().to_owned()
}

/// A chat on its own branch, whose first run made one commit there.
fn chat_with_a_commit(w: &mut World, bridge: &mut Bridge) {
    send(w, 1, "n;branch=1", "Feature", "write it");
    reply(bridge, w, 1);
    let copy = w.code.join(".gnomish-worktrees/app/feature");
    git(&copy, &["add", "-A"]);
    git(&copy, &["commit", "-q", "-m", "agent work"]);
}

#[test]
fn a_run_in_a_repository_ends_with_a_change_summary_and_its_branch() {
    let mut w = world();
    let mut bridge = bridge(&w);

    send(&mut w, 1, "n", "app", "write it");
    let line = reply(&mut bridge, &w, 1);

    assert!(
        line.contains(r"\027M1\010B\031main\0310\031\010G\0311\0311\0310\010F\031agent.txt\0311\0310\031A\010p\031Done."),
        "{line}"
    );
}

#[test]
fn a_run_that_changes_nothing_has_no_change_summary() {
    let mut w = world();
    fs::write(w.repo.join("agent.txt"), "same\n").unwrap();
    git(&w.repo, &["add", "-A"]);
    git(&w.repo, &["commit", "-q", "-m", "two"]);
    let mut bridge = bridge(&w);

    send(&mut w, 1, "n", "app", "same");
    let line = reply(&mut bridge, &w, 1);

    assert!(!line.contains(r"\010G\031"), "{line}");
}

#[test]
fn commit_from_the_game_commits_the_files_of_the_summary() {
    let mut w = world();
    let mut bridge = bridge(&w);
    send(&mut w, 1, "n", "app", "write it");
    reply(&mut bridge, &w, 1);
    fs::write(w.repo.join("mine.txt"), "the player's own file\n").unwrap();

    send(&mut w, 2, "git=commit:1", "app", "add the agent file");
    let line = reply(&mut bridge, &w, 2);

    assert!(line.contains("Committed 1 file as "), "{line}");
    assert_eq!(
        git(&w.repo, &["log", "-1", "--format=%s"]).trim(),
        "add the agent file"
    );
    assert_eq!(git(&w.repo, &["status", "--porcelain"]), "?? mine.txt\n");
}

#[test]
fn revert_from_the_game_takes_back_only_the_run() {
    let mut w = world();
    fs::write(w.repo.join("readme.txt"), "the player's edit\n").unwrap();
    let mut bridge = bridge(&w);
    send(&mut w, 1, "n", "app", "write it");
    reply(&mut bridge, &w, 1);

    send(&mut w, 2, "git=revert:1", "app", "");
    let line = reply(&mut bridge, &w, 2);

    assert!(line.contains("Reverted 1 file."), "{line}");
    assert!(!w.repo.join("agent.txt").exists());
    assert_eq!(
        fs::read_to_string(w.repo.join("readme.txt")).unwrap(),
        "the player's edit\n"
    );
}

#[test]
fn a_second_revert_of_one_summary_changes_nothing() {
    let mut w = world();
    let mut bridge = bridge(&w);
    send(&mut w, 1, "n", "app", "write it");
    reply(&mut bridge, &w, 1);
    send(&mut w, 2, "git=revert:1", "app", "");
    reply(&mut bridge, &w, 2);

    send(&mut w, 3, "git=revert:1", "app", "");
    let line = reply(&mut bridge, &w, 3);

    assert!(line.contains("already reverted"), "{line}");
}

#[test]
fn a_commit_on_an_own_branch_goes_to_that_branch() {
    let mut w = world();
    let mut bridge = bridge(&w);
    send(&mut w, 1, "n;branch=1", "Feature", "write it");
    reply(&mut bridge, &w, 1);

    send(&mut w, 2, "git=commit:1", "app", "agent work");
    let line = reply(&mut bridge, &w, 2);

    assert!(line.contains(" on gnomish/feature."), "{line}");
    let log = git(&w.repo, &["log", "-1", "--format=%s", "gnomish/feature"]);
    assert_eq!(log.trim(), "agent work");
    assert_eq!(
        git(&w.repo, &["log", "-1", "--format=%s", "main"]).trim(),
        "one"
    );
}

#[test]
fn a_run_outside_a_repository_has_no_blocks() {
    let mut w = world();
    fs::remove_dir_all(w.repo.join(".git")).unwrap();
    let mut bridge = bridge(&w);

    send(&mut w, 1, "n", "app", "write it");
    let line = reply(&mut bridge, &w, 1);

    assert!(
        line.contains(r#"text = "\027M1\010p\031Done.\010""#),
        "{line}"
    );
}

#[test]
fn an_own_branch_works_in_a_worktree_next_to_the_repository() {
    let mut w = world();
    let mut bridge = bridge(&w);

    send(&mut w, 1, "n;branch=1", "Fix it", "write it");
    let line = reply(&mut bridge, &w, 1);

    let copy = w.code.join(".gnomish-worktrees/app/fix-it");
    assert_eq!(
        *w.folders.lock().unwrap(),
        [copy.to_string_lossy().into_owned()]
    );
    assert!(copy.join("agent.txt").is_file());
    assert!(!w.repo.join("agent.txt").exists());
    assert!(
        line.contains(r"\010B\031gnomish/fix-it\0311\031main"),
        "{line}"
    );
}

#[test]
fn a_second_run_of_an_own_branch_uses_the_same_worktree() {
    let mut w = world();
    let mut bridge = bridge(&w);
    send(&mut w, 1, "n;branch=1", "Fix it", "one");
    reply(&mut bridge, &w, 1);

    send(&mut w, 2, "branch=1", "Fix it", "two");
    reply(&mut bridge, &w, 2);

    let folders = w.folders.lock().unwrap().clone();
    assert_eq!(folders[0], folders[1]);
}

#[test]
fn merge_asks_on_the_desktop_and_then_merges_into_the_start_branch() {
    let mut w = world();
    let (mut bridge, approvals) = with_desktop(bridge(&w), &w);
    chat_with_a_commit(&mut w, &mut bridge);
    let answering = answer_on_the_desktop(approvals, Verdict::Approve);

    send(&mut w, 2, "git=merge", "app", "");
    let line = reply(&mut bridge, &w, 2);

    assert!(
        answering
            .join()
            .unwrap()
            .contains("merge gnomish/feature into main")
    );
    assert!(line.contains("Merged gnomish/feature into main."), "{line}");
    assert!(w.repo.join("agent.txt").is_file());
}

#[test]
fn merge_with_a_deny_on_the_desktop_changes_nothing() {
    let mut w = world();
    let (mut bridge, approvals) = with_desktop(bridge(&w), &w);
    chat_with_a_commit(&mut w, &mut bridge);
    let answering = answer_on_the_desktop(approvals, Verdict::Deny);

    send(&mut w, 2, "git=merge", "app", "");
    let line = reply(&mut bridge, &w, 2);
    answering.join().unwrap();

    assert!(line.contains("Not merged."), "{line}");
    assert!(!w.repo.join("agent.txt").exists());
}

#[test]
fn discard_from_the_game_removes_the_worktree_and_the_branch() {
    let mut w = world();
    let mut bridge = bridge(&w);
    send(&mut w, 1, "n;branch=1", "Gone", "write it");
    reply(&mut bridge, &w, 1);

    send(&mut w, 2, "git=discard", "app", "");
    let line = reply(&mut bridge, &w, 2);

    assert!(line.contains("Discarded gnomish/gone."), "{line}");
    assert!(!w.code.join(".gnomish-worktrees").exists());
    assert_eq!(git(&w.repo, &["branch", "--list", "gnomish/*"]), "");
}

#[test]
fn a_deleted_chat_keeps_a_worktree_with_changes() {
    let mut w = world();
    let mut bridge = bridge(&w);
    send(&mut w, 1, "n;branch=1", "Busy", "write it");
    reply(&mut bridge, &w, 1);

    send(&mut w, 0, "d", "app", "");
    common::step_until_within(&mut bridge, Duration::from_secs(2), || false);

    assert!(
        w.code
            .join(".gnomish-worktrees/app/busy/agent.txt")
            .is_file()
    );
}

#[test]
fn a_deleted_chat_removes_a_clean_worktree_and_its_merged_branch() {
    let mut w = world();
    let mut bridge = bridge(&w);
    send(&mut w, 1, "n;branch=1", "Tidy", "write it");
    reply(&mut bridge, &w, 1);
    let copy = w.code.join(".gnomish-worktrees/app/tidy");
    fs::remove_file(copy.join("agent.txt")).unwrap();

    send(&mut w, 0, "d", "app", "");
    let branches = || git(&w.repo, &["branch", "--list", "gnomish/*"]);
    let gone = common::step_until_within(&mut bridge, Duration::from_secs(10), || {
        !copy.exists() && branches().is_empty()
    });

    assert!(gone, "{}", branches());
}
