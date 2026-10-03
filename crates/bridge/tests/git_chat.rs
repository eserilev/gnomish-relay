//! Git in a chat (SPEC.md 9.11), through the whole bridge: a message from the game, a
//! test agent that edits files, and the real `git` in temp folders.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bridge::agent::{Agent, Control, Event, Run};
use bridge::ci_checks::CiChecks;
use bridge::config::{Permission, Policy};
use bridge::desktop::{Approvals, Prompt, Verdict};
use bridge::folder_path::{path_bytes, real_path};
use bridge::game_folders::GameFolders;
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

/// An agent that writes one file into the folder of its run, and tells what it saw. It
/// can also report the output of a command. While `hold` is set, a run waits before it
/// writes.
struct Editor {
    folders: Arc<Mutex<Vec<String>>>,
    output: Option<String>,
    hold: Arc<AtomicBool>,
}

impl Agent for Editor {
    fn run(&self, job: &Job, control: &Control) -> Run {
        self.folders.lock().unwrap().push(job.cwd.clone());
        while self.hold.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(
            Path::new(&job.cwd).join("agent.txt"),
            format!("{}\n", job.text),
        )
        .unwrap();
        if let Some(output) = &self.output {
            control.events.send(Event::CommandOutput(output.clone()));
        }
        Run {
            reply: Ok("Done.".into()),
            session: None,
            usage: None,
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
    hold: Arc<AtomicBool>,
    frames: Vec<Vec<u8>>,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let host = GitHost::with_config(UserConfig::Skip).unwrap();
    String::from_utf8(host.bytes(dir, args).unwrap()).unwrap()
}

/// `code/app`, a repository with one commit on `main`, and `code` as the only root.
fn world() -> World {
    let root = tempfile::tempdir().unwrap();
    let base = real_path(root.path()).unwrap();
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
        hold: Arc::default(),
        frames: Vec::new(),
    }
}

fn bridge(w: &World) -> Bridge {
    bridge_with(w, None, CiChecks::Off)
}

fn bridge_with(w: &World, output: Option<&str>, ci: CiChecks) -> Bridge {
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
        games: vec![GameFolders {
            name: "game".into(),
            addons: w.addons.clone(),
            screenshots: w.addons.join("../../Screenshots"),
            accounts: w.accounts.clone(),
        }],
        state: w.data.clone(),
        config: w.data.join("config"),
    };
    let editor = Editor {
        folders: Arc::clone(&w.folders),
        output: output.map(str::to_owned),
        hold: Arc::clone(&w.hold),
    };
    let agents = [("claude".to_owned(), Arc::new(editor) as Arc<dyn Agent>)].into();
    let keys = KeySet::new(StripKey::from_hex(&hex(KEY)).unwrap(), None).unwrap();
    Bridge::new(paths, policy, keys, agents)
        .unwrap()
        .with_git(GitHost::with_config(UserConfig::Skip).unwrap(), ci)
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
    send_in(w, "c1", id, flags, name, text);
}

fn send_in(w: &mut World, chat: &str, id: u32, flags: &str, name: &str, text: &str) {
    let payload = format!("tok\x1f{chat}\x1f{id}\x1fapp\x1f{flags}\x1f{name}\x1f{text}");
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
    let folders: Vec<PathBuf> = w
        .folders
        .lock()
        .unwrap()
        .iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(folders, [copy.as_path()]);
    assert!(copy.join("agent.txt").is_file());
    assert!(!w.repo.join("agent.txt").exists());
    assert!(
        line.contains(r"\010B\031gnomish/fix-it\0311\031main"),
        "{line}"
    );
}

fn worktrees(w: &World) -> usize {
    fs::read_dir(w.code.join(".gnomish-worktrees/app")).map_or(0, Iterator::count)
}

#[test]
fn a_message_that_waits_for_the_limit_makes_no_worktree_until_it_starts() {
    let mut w = world();
    let mut bridge = bridge(&w).with_max_runs(1);
    w.hold.store(true, Ordering::SeqCst);
    send_in(&mut w, "c1", 1, "n;branch=1", "One", "one");
    send_in(&mut w, "c2", 2, "n;branch=1", "Two", "two");

    let folders = Arc::clone(&w.folders);
    let started = common::step_until_within(&mut bridge, Duration::from_secs(30), || {
        !folders.lock().unwrap().is_empty()
    });
    for _ in 0..20 {
        bridge.step();
    }
    let while_waiting = worktrees(&w);
    w.hold.store(false, Ordering::SeqCst);
    reply(&mut bridge, &w, 1);
    reply(&mut bridge, &w, 2);

    assert!(started);
    assert_eq!(while_waiting, 1);
    assert_eq!(worktrees(&w), 2);
}

#[test]
fn commit_refuses_a_summary_of_two_chats_that_ran_at_once_in_one_folder() {
    let mut w = world();
    let mut bridge = bridge(&w);
    w.hold.store(true, Ordering::SeqCst);
    send_in(&mut w, "c1", 1, "n", "One", "one");
    send_in(&mut w, "c2", 2, "n", "Two", "two");
    let folders = Arc::clone(&w.folders);
    let both = common::step_until_within(&mut bridge, Duration::from_secs(30), || {
        folders.lock().unwrap().len() == 2
    });
    w.hold.store(false, Ordering::SeqCst);
    reply(&mut bridge, &w, 1);
    reply(&mut bridge, &w, 2);

    send_in(&mut w, "c1", 3, "git=commit:1", "One", "one");
    let line = reply(&mut bridge, &w, 3);

    assert!(both);
    assert!(
        line.contains("Another chat worked in this folder"),
        "{line}"
    );
    assert_eq!(git(&w.repo, &["rev-list", "--count", "HEAD"]).trim(), "1");
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

/// Another chat points the copy of `feature` at a git folder with a clean filter
/// that leaves a mark. Returns the mark.
fn move_the_commondir(w: &World) -> PathBuf {
    let evil = w.code.join("evil");
    fs::create_dir_all(&evil).unwrap();
    git(&evil, &["init", "-q"]);
    let mark = w.code.join("filter-ran");
    let filter = format!("sh -c 'touch {}; cat'", mark.display());
    git(&evil, &["config", "filter.x.clean", &filter]);
    let admin = w.repo.join(".git/worktrees/feature");
    fs::write(
        admin.join("commondir"),
        format!("{}/.git\n", evil.display()),
    )
    .unwrap();
    let copy = w.code.join(".gnomish-worktrees/app/feature");
    fs::write(copy.join(".gitattributes"), "* filter=x\n").unwrap();
    mark
}

#[test]
fn git_actions_on_a_copy_whose_commondir_moved_run_no_git_there() {
    let mut w = world();
    let mut bridge = bridge(&w);
    send(&mut w, 1, "n;branch=1", "Feature", "write it");
    reply(&mut bridge, &w, 1);
    let mark = move_the_commondir(&w);

    send(&mut w, 2, "git=commit:1", "app", "agent work");
    let commit = reply(&mut bridge, &w, 2);
    send(&mut w, 3, "git=discard", "app", "");
    let discard = reply(&mut bridge, &w, 3);

    assert!(commit.contains("won't run git there"), "{commit}");
    assert!(discard.contains("won't run git there"), "{discard}");
    assert!(w.code.join(".gnomish-worktrees/app/feature").is_dir());
    assert!(!mark.exists());
}

#[test]
fn a_run_in_a_copy_whose_commondir_moved_does_not_start() {
    let mut w = world();
    let mut bridge = bridge(&w);
    send(&mut w, 1, "n;branch=1", "Feature", "write it");
    reply(&mut bridge, &w, 1);
    let mark = move_the_commondir(&w);

    send(&mut w, 2, "branch=1", "Feature", "again");
    let line = reply(&mut bridge, &w, 2);

    assert!(line.contains("won't run git there"), "{line}");
    assert_eq!(w.folders.lock().unwrap().len(), 1);
    assert!(!mark.exists());
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

#[test]
fn the_last_test_output_of_a_run_becomes_its_test_line() {
    let mut w = world();
    let output = "test result: FAILED. 7 passed; 2 failed; 1 ignored; 0 measured";
    let mut bridge = bridge_with(&w, Some(output), CiChecks::Off);

    send(&mut w, 1, "n", "app", "test it");
    let line = reply(&mut bridge, &w, 1);

    assert!(line.contains(r"\010T\0317\0312\0311"), "{line}");
}

#[test]
fn output_with_no_test_summary_gives_no_test_line() {
    let mut w = world();
    let mut bridge = bridge_with(&w, Some("Compiling app"), CiChecks::Off);

    send(&mut w, 1, "n", "app", "build it");
    let line = reply(&mut bridge, &w, 1);

    assert!(!line.contains(r"\010T\031"), "{line}");
}

#[test]
fn checks_that_are_off_say_how_to_turn_them_on() {
    let mut w = world();
    let mut bridge = bridge(&w);

    send(&mut w, 1, "git=checks", "app", "");
    let line = reply(&mut bridge, &w, 1);

    assert!(line.contains("Checks are off."), "{line}");
}

#[cfg(unix)]
fn fake_gh(w: &World, answer: &str) -> PathBuf {
    let gh = w.data.join("gh");
    bridge::fake_program::write(&gh, &format!("#!/bin/sh\n{answer}\n")).unwrap();
    gh
}

#[cfg(unix)]
#[test]
fn the_ci_checks_of_the_branch_come_under_the_reply_and_on_request() {
    let mut w = world();
    let rollup = r#"{"statusCheckRollup":[{"name":"build","status":"COMPLETED","conclusion":"SUCCESS"},{"name":"lint","status":"COMPLETED","conclusion":"FAILURE"}]}"#;
    let gh = fake_gh(&w, &format!("echo '{rollup}'"));
    let mut bridge = bridge_with(&w, None, CiChecks::On { program: gh });

    send(&mut w, 1, "n", "app", "write it");
    let line = reply(&mut bridge, &w, 1);
    send(&mut w, 2, "git=checks", "app", "");
    let checks = reply(&mut bridge, &w, 2);

    assert!(line.contains(r"\010C\0311\0311\0310\031lint"), "{line}");
    assert!(
        checks.contains(r#"status = "done", text = "\027M1\010C\0311\0311\0310\031lint\010""#),
        "{checks}"
    );
}

#[cfg(unix)]
#[test]
fn checks_of_a_branch_with_no_pull_request_say_so() {
    let mut w = world();
    let gh = fake_gh(
        &w,
        "echo 'no pull requests found for branch \"main\"' >&2\nexit 1",
    );
    let mut bridge = bridge_with(&w, None, CiChecks::On { program: gh });

    send(&mut w, 1, "git=checks", "app", "");
    let line = reply(&mut bridge, &w, 1);

    assert!(line.contains("No pull request for main yet."), "{line}");
}
