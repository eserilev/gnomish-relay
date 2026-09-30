//! The `command` backend with the real sandbox of the OS (SPEC.md 9.2, 6.6.4): the whole
//! fake harness runs in `bwrap` on Linux and in `sandbox-exec` on macOS. With no working
//! tool the tests skip, unless `GNOMISH_REQUIRE_BWRAP` or `GNOMISH_REQUIRE_SANDBOX_EXEC`
//! is set, as in CI.
#![cfg(any(target_os = "linux", target_os = "macos"))]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use bridge::agent::{Agent, Control, Event, EventSender, Events, StopSignal};
use bridge::agent_wall::{AgentDirs, AgentWall};
use bridge::allow_hosts::HostList;
use bridge::command_sandbox::{CommandSandbox, detect_overlay};
use bridge::config::{AgentSpec, Kind, Permission};
use bridge::gate::Gate;
use bridge::harness::{CommandAgent, RAN_BEFORE, free_commands_note};
use bridge::harness_output::LONG_OUTPUT;
use bridge::harness_process::TOO_MUCH;
use bridge::harness_sandbox::NO_SANDBOX;
use bridge::proxy::{Limits, Net, ProxySettings};
use bridge::relay::{ChatId, Job, MessageId, Session, Work};
use bridge::story_sandbox::{self, Sandbox};

const REQUIRE: &str = if cfg!(target_os = "macos") {
    "GNOMISH_REQUIRE_SANDBOX_EXEC"
} else {
    "GNOMISH_REQUIRE_BWRAP"
};

/// `None` skips the test on a computer with no working sandbox.
fn tool() -> Option<Sandbox> {
    let tool = story_sandbox::detect();
    if tool != Sandbox::None {
        return Some(tool);
    }
    assert!(
        std::env::var_os(REQUIRE).is_none(),
        "the sandbox tool is missing or does not work, and {REQUIRE} is set"
    );
    eprintln!("skipped: the sandbox tool is missing or does not work");
    None
}

/// A public address that stands for the fake server behind the proxy.
const PUBLIC: [u8; 4] = [93, 184, 216, 34];

fn server() -> SocketAddr {
    static SERVER: OnceLock<SocketAddr> = OnceLock::new();
    *SERVER.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = io::BufReader::new(&stream).read_line(&mut line);
                let _ = write!(stream, "HTTP/1.1 200 OK\r\n\r\nhello {}", line.trim_end());
            }
        });
        addr
    })
}

fn fake_resolve(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let ip: IpAddr = match host {
        "allowed.test" | "other.test" => PUBLIC.into(),
        "local.test" => [127, 0, 0, 1].into(),
        _ => return Err(io::ErrorKind::NotFound.into()),
    };
    Ok(vec![SocketAddr::new(ip, port)])
}

fn fake_connect(addr: &SocketAddr, limit: Duration) -> io::Result<TcpStream> {
    if addr.ip() == IpAddr::from(PUBLIC) {
        return TcpStream::connect_timeout(&server(), limit);
    }
    TcpStream::connect_timeout(addr, limit)
}

fn open_network() -> ProxySettings {
    ProxySettings {
        net: Net {
            resolve: fake_resolve,
            connect: fake_connect,
        },
        limits: Limits::default(),
        ..ProxySettings::public()
    }
}

fn strict_network() -> ProxySettings {
    ProxySettings {
        hosts: Arc::new(HostList::default()),
        mode: protocol::connect::Mode::Listed,
        ..open_network()
    }
}

/// The target folder, unless a part of its path is hidden, as in a worktree under
/// `.claude/`. The home cache folder then takes its place.
fn test_root() -> PathBuf {
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    if !target.components().any(|c| c.as_os_str() == ".claude") {
        return target;
    }
    let cache = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache");
    fs::create_dir_all(&cache).unwrap();
    cache
}

/// A home with an ssh key, the folders of the bridge, a chat folder, and a second
/// project. The programs lie in a folder of their own, which the sandbox does not hide.
struct Machine {
    _root: tempfile::TempDir,
    home: PathBuf,
    data: PathBuf,
    config: PathBuf,
    chat: PathBuf,
    other: PathBuf,
    bin: PathBuf,
}

fn copy_program(from: &str, bin: &Path) -> PathBuf {
    let to = bin.join(Path::new(from).file_name().unwrap());
    fs::copy(from, &to).unwrap();
    to
}

fn machine() -> Machine {
    let root = tempfile::tempdir_in(test_root()).unwrap();
    let base = root.path().canonicalize().unwrap();
    let home = base.join("home");
    let data = home.join(".local/share/gnomish-relay");
    let config = home.join(".config/gnomish-relay");
    let chat = home.join("Code/app");
    let other = home.join("Code/other");
    let bin = base.join("bin");
    for dir in [
        &data,
        &config,
        &chat.join(".git"),
        &other,
        &home.join(".ssh"),
        &bin,
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(home.join(".ssh/id_ed25519"), "secret-ssh-key").unwrap();
    fs::write(data.join("state.json"), "secret-state").unwrap();
    fs::write(chat.join(".env"), "secret-env").unwrap();
    fs::write(chat.join("notes.txt"), "plain-notes").unwrap();
    copy_program(env!("CARGO_BIN_EXE_fake-cli-agent"), &bin);
    copy_program(env!("CARGO_BIN_EXE_gnomish-relay"), &bin);
    Machine {
        _root: root,
        home,
        data,
        config,
        chat,
        other,
        bin,
    }
}

fn gate(m: &Machine, tool: Sandbox, network: ProxySettings) -> Gate {
    let wrapper = m.bin.join("gnomish-relay");
    let mut sandbox = CommandSandbox::new(tool.clone(), wrapper.clone(), Some(m.home.clone()));
    sandbox.overlay = detect_overlay(&tool);
    Gate {
        sandbox,
        wall: AgentWall::new(
            tool,
            wrapper,
            Some(m.home.clone()),
            AgentDirs::of(&m.home, &|_| None),
            m.data.clone(),
            network,
        ),
        home: m.home.clone(),
        ..Gate::bare(vec![m.home.join("Code")], m.config.clone(), m.data.clone())
    }
}

fn spec(m: &Machine, template: &[&str]) -> AgentSpec {
    let mut command = vec![m.bin.join("fake-cli-agent").to_string_lossy().into_owned()];
    command.extend(template.iter().map(|w| (*w).to_owned()));
    AgentSpec {
        kind: Kind::Command,
        command,
        env: vec!["CARGO_PKG_NAME".into()],
        modes: BTreeMap::new(),
        agent_hosts: Vec::new(),
        resume: vec!["--continue".into()],
        ask_args: Vec::new(),
    }
}

fn agent_with(m: &Machine, template: &[&str], gate: &Gate) -> CommandAgent {
    CommandAgent::new("fake", &spec(m, template), Duration::from_secs(30), gate)
}

fn agent(m: &Machine, tool: Sandbox, template: &[&str]) -> CommandAgent {
    agent_with(m, template, &gate(m, tool, open_network()))
}

fn blank_job() -> Job {
    Job {
        token: "tok".into(),
        chat: ChatId::new("c1"),
        id: MessageId(1),
        agent: "fake".into(),
        permission: Permission::AutoEdit,
        asked: Permission::AutoEdit,
        cwd: String::new(),
        session: Session::New,
        resume: None,
        text: String::new(),
        work: Work::Prompt,
        new_folder: false,
    }
}

fn job(m: &Machine, level: Permission, text: &str) -> Job {
    Job {
        permission: level,
        asked: level,
        cwd: m.chat.to_string_lossy().into_owned(),
        text: text.into(),
        ..blank_job()
    }
}

/// The reply with the note of the first run of this agent taken off.
fn reply(agent: &CommandAgent, job: &Job) -> Result<String, String> {
    let note = format!("{}\n\n", free_commands_note("fake"));
    let reply = agent.run(job, &Control::default()).reply?;
    Ok(reply.strip_prefix(&note).unwrap_or(&reply).to_owned())
}

fn probe(m: &Machine, agent: &CommandAgent, level: Permission, probes: &str) -> String {
    reply(agent, &job(m, level, &format!("net {probes}"))).unwrap()
}

fn listening() -> (Control, Receiver<(ChatId, MessageId, Event)>) {
    let (tx, rx): (EventSender, _) = channel();
    let job = blank_job();
    let control = Control {
        stop: StopSignal::default(),
        events: Events::to_bridge(tx, &job),
    };
    (control, rx)
}

#[test]
fn the_message_reaches_the_harness_as_an_argument_a_file_or_stdin() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let job = job(&m, Permission::AutoEdit, "fix the bug; rm -rf / \"$(x)\"");

    for template in [
        &["echo", "{prompt}"][..],
        &["echo", "--file", "{prompt_file}"],
        &["echo"],
    ] {
        let agent = agent(&m, tool.clone(), template);

        assert_eq!(
            reply(&agent, &job).unwrap(),
            "echo: fix the bug; rm -rf / \"$(x)\"",
            "{template:?}"
        );
    }
}

#[test]
fn the_first_reply_says_once_that_the_harness_runs_commands_with_no_question() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["echo", "{prompt}"]);
    let job = job(&m, Permission::AutoEdit, "hi");

    let first = agent.run(&job, &Control::default()).reply.unwrap();
    let second = agent.run(&job, &Control::default()).reply.unwrap();

    assert_eq!(first, format!("{}\n\necho: hi", free_commands_note("fake")));
    assert_eq!(second, "echo: hi");
}

#[test]
fn each_line_becomes_a_clean_progress_line_and_the_output_is_the_reply() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["stream", "{prompt}"]);
    let (control, events) = listening();

    let run = agent.run(&job(&m, Permission::AutoEdit, "ok"), &control);

    let steps: Vec<String> = events
        .try_iter()
        .filter_map(|(_, _, e)| match e {
            Event::Progress(line) => Some(line),
            _ => None,
        })
        .collect();
    assert_eq!(
        steps,
        ["reading main.rs", "100%", "editing main.rs", "done: ok"]
    );
    let reply = run.reply.unwrap();
    assert!(
        reply.ends_with("reading main.rs\n100%\nediting main.rs\ndone: ok"),
        "{reply}"
    );
    assert_eq!(run.session, Some(RAN_BEFORE.into()));
}

#[test]
fn a_harness_that_fails_gives_its_exit_status_and_last_error_line() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["crash"]);

    let error = reply(&agent, &job(&m, Permission::AutoEdit, "hi")).unwrap_err();

    assert_eq!(
        error,
        "The agent failed (exit status 3): Error: no API key. Set FAKE_API_KEY."
    );
}

#[test]
fn a_long_output_keeps_its_end_and_a_huge_output_stops_the_run() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["huge", "{prompt}"]);

    let long = reply(&agent, &job(&m, Permission::AutoEdit, "1")).unwrap();
    let huge = reply(&agent, &job(&m, Permission::AutoEdit, "20")).unwrap_err();

    assert!(long.starts_with(LONG_OUTPUT), "{}", &long[..80]);
    assert!(long.ends_with("the end"));
    assert!(long.len() <= 256 * 1024);
    assert_eq!(huge, TOO_MUCH);
}

#[test]
fn stop_ends_the_harness_and_every_program_that_it_started() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["sleeper", "{prompt}"]);
    let marker = m.chat.join("late.txt");
    let control = Control::default();
    let stop = control.stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        stop.request();
    });
    let started = Instant::now();

    let run = agent.run(
        &job(&m, Permission::AutoEdit, &marker.to_string_lossy()),
        &control,
    );

    assert_eq!(run.reply.unwrap_err(), "Stopped.");
    assert!(started.elapsed() < Duration::from_secs(5));
    std::thread::sleep(Duration::from_secs(4));
    assert!(!marker.exists(), "a program of the harness outlived Stop");
}

#[test]
fn a_harness_that_runs_too_long_times_out() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let gate = gate(&m, tool, open_network());
    let agent = CommandAgent::new("fake", &spec(&m, &["slow"]), Duration::from_secs(1), &gate);

    let error = reply(&agent, &job(&m, Permission::AutoEdit, "hi")).unwrap_err();

    assert_eq!(error, "Timed out.");
}

#[test]
fn only_the_writes_of_the_harness_to_the_chat_folder_stay_and_it_never_sees_the_secrets() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["probe", "{prompt}"]);
    let probes = format!(
        "write={} write={} read={} read={} read={} read={}",
        m.chat.join("new.txt").display(),
        m.other.join("new.txt").display(),
        m.home.join(".ssh/id_ed25519").display(),
        m.data.join("state.json").display(),
        m.chat.join(".env").display(),
        m.chat.join("notes.txt").display(),
    );

    let reply = probe(&m, &agent, Permission::AutoEdit, &probes);

    let words: Vec<&str> = reply.split_whitespace().collect();
    assert_eq!(words[0], "write=ok");
    // With the view of the home folder the write works, but only inside the view.
    for secret in &words[2..5] {
        assert!(!secret.contains("secret"), "{reply}");
    }
    assert_eq!(words[5], "read=plain-notes");
    assert!(m.chat.join("new.txt").exists());
    assert!(!m.other.join("new.txt").exists());
}

#[test]
fn a_harness_that_makes_a_git_folder_gets_a_notice_after_the_reply() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let module = m.chat.join(".git/modules/lib");
    fs::create_dir_all(&module).unwrap();
    let agent = agent(&m, tool, &["probe", "{prompt}"]);
    let probes = format!(
        "write={} write={}",
        module.join("HEAD").display(),
        module.join("config").display(),
    );

    let reply = probe(&m, &agent, Permission::AutoEdit, &probes);

    assert!(reply.starts_with("write=ok write=ok\n\n"), "{reply}");
    assert!(
        reply.contains(&module.join("config").display().to_string()),
        "{reply}"
    );
}

#[test]
fn at_ask_the_harness_cannot_change_the_chat_folder() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["probe", "{prompt}"]);
    let probes = format!(
        "write={} read={}",
        m.chat.join("new.txt").display(),
        m.chat.join("notes.txt").display()
    );

    let reply = probe(&m, &agent, Permission::Ask, &probes);

    assert_eq!(reply, "write=fail read=plain-notes");
    assert!(!m.chat.join("new.txt").exists());
}

#[test]
fn the_writes_of_the_harness_to_the_home_folder_never_reach_the_real_home() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["probe", "{prompt}"]);
    let rc = m.home.join(".bashrc");

    let reply = probe(
        &m,
        &agent,
        Permission::FullAuto,
        &format!("write={}", rc.display()),
    );

    assert!(!rc.exists(), "{reply}");
}

#[test]
fn the_harness_reaches_public_hosts_only_through_the_proxy() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["probe", "{prompt}"]);
    let outside = server();

    let reply = probe(
        &m,
        &agent,
        Permission::AutoEdit,
        &format!("direct={outside} proxy=allowed.test:443 proxy=local.test:443"),
    );

    assert_eq!(
        reply,
        "direct=fail proxy=200:hello_GET_/agent_HTTP/1.1 proxy=403"
    );
}

#[test]
fn in_strict_mode_the_harness_reaches_only_its_hosts() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let gate = gate(&m, tool, strict_network());
    let mut spec = spec(&m, &["probe", "{prompt}"]);
    spec.agent_hosts = vec!["allowed.test".into()];
    let agent = CommandAgent::new("fake", &spec, Duration::from_secs(30), &gate);

    let reply = probe(
        &m,
        &agent,
        Permission::AutoEdit,
        "proxy=allowed.test:443 proxy=other.test:443",
    );

    assert_eq!(reply, "proxy=200:hello_GET_/agent_HTTP/1.1 proxy=403");
}

#[cfg(target_os = "linux")]
#[test]
fn a_socket_in_the_home_folder_is_out_of_reach_also_with_no_view_of_the_home_folder() {
    use std::os::fd::AsRawFd;
    let Some(tool) = tool() else { return };
    let m = machine();
    fs::create_dir_all(m.home.join(".docker")).unwrap();
    let folder = fs::File::open(m.home.join(".docker")).unwrap();
    let short = format!("/proc/self/fd/{}/docker.sock", folder.as_raw_fd());
    let _listener = std::os::unix::net::UnixListener::bind(short).unwrap();
    // The view of the home folder hides sockets too, so this test goes without it.
    let mut gate = gate(&m, tool, open_network());
    gate.sandbox.overlay = bridge::command_sandbox::Overlay::Missing;
    let agent = agent_with(&m, &["probe", "{prompt}"], &gate);
    let socket = m.home.join(".docker/docker.sock");

    let reply = probe(
        &m,
        &agent,
        Permission::AutoEdit,
        &format!("sock={}", socket.display()),
    );

    assert_eq!(reply, "sock=fail");
}

#[test]
fn the_harness_gets_the_variables_of_its_entry_and_no_other_variable() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["env", "{prompt}"]);
    // Cargo gives both to the test process. The entry names only the first.
    let text = "CARGO_PKG_NAME CARGO_MANIFEST_DIR NO_COLOR";

    let reply = reply(&agent, &job(&m, Permission::AutoEdit, text)).unwrap();

    assert_eq!(
        reply,
        "CARGO_PKG_NAME=bridge CARGO_MANIFEST_DIR=- NO_COLOR=1"
    );
}

#[test]
fn a_chat_that_goes_on_passes_the_resume_arguments() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["history", "{prompt}"]);
    let mut job = job(&m, Permission::AutoEdit, "first");

    let first = reply(&agent, &job).unwrap();
    job.resume = Some(RAN_BEFORE.into());
    job.text = "second".into();
    let second = reply(&agent, &job).unwrap();

    assert_eq!(first, "run 1, continue false");
    assert_eq!(second, "run 2, continue true");
}

#[test]
fn check_agent_starts_the_harness_inside_the_sandbox() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let agent = agent(&m, tool, &["echo", "{prompt}"]);

    let report = agent.check(&m.chat.to_string_lossy()).unwrap();

    assert_eq!(report.version, "fake-cli-agent 1.2.3");
    assert!(report.load_session);
    assert!(report.details.iter().any(|d| d.starts_with("sandbox: ")));
    assert!(
        report
            .details
            .iter()
            .any(|d| d == "input: the message is one argument")
    );
}

#[test]
fn with_no_sandbox_the_bridge_refuses_to_start_the_harness() {
    let m = machine();
    let agent = agent(&m, Sandbox::None, &["echo", "{prompt}"]);

    let error = reply(&agent, &job(&m, Permission::AutoEdit, "hi")).unwrap_err();
    let check = agent.check(&m.chat.to_string_lossy()).unwrap_err();

    assert_eq!(error, NO_SANDBOX);
    assert_eq!(check, NO_SANDBOX);
}

/// Each preset whose tool is on `PATH`, with the real home folder for its login, the
/// real network through the proxy, and a real model call. Run it by hand:
/// `cargo test -p bridge --test harness -- --ignored`.
#[test]
#[ignore = "calls a real model with the login of this computer"]
fn each_installed_preset_answers_inside_the_sandbox() {
    let Some(tool) = tool() else { return };
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let root = tempfile::tempdir_in(test_root()).unwrap();
    let base = root.path().canonicalize().unwrap();
    let chat = base.join("chat");
    fs::create_dir_all(&chat).unwrap();
    let path = std::env::var_os("PATH").unwrap_or_default();
    for preset in &bridge::harness_presets::PRESETS {
        if bridge::program::find_program(preset.program, &path, false).is_none() {
            eprintln!("skipped {}: not on PATH", preset.name);
            continue;
        }
        let text = format!(
            "allowed_roots = [\"{}\"]\ndefault_agent = \"x\"\n[wow]\npath = \"/wow\"\n\
             [agents.x]\nkind = \"command\"\npreset = \"{}\"\npermission = \"auto-edit\"\n\
             env = [\"OPENAI_API_KEY\", \"ANTHROPIC_API_KEY\", \"GEMINI_API_KEY\"]\n",
            base.display(),
            preset.name
        );
        let relay = bridge::config::parse(&text, &home).unwrap().relay.unwrap();
        let mut sandbox = CommandSandbox::new(
            tool.clone(),
            PathBuf::from(env!("CARGO_BIN_EXE_gnomish-relay")),
            Some(home.clone()),
        );
        sandbox.overlay = detect_overlay(&tool);
        let data = base.join("data");
        let gate = Gate {
            sandbox,
            wall: AgentWall::new(
                tool.clone(),
                PathBuf::new(),
                None,
                AgentDirs::default(),
                data.clone(),
                ProxySettings::public(),
            ),
            home: home.clone(),
            ..Gate::bare(vec![base.clone()], base.join("config"), data)
        };
        let agent = CommandAgent::new("x", &relay.agents["x"], Duration::from_mins(3), &gate);
        let job = Job {
            cwd: chat.to_string_lossy().into_owned(),
            text: "Reply with only the word hi.".into(),
            ..blank_job()
        };

        let reply = agent.run(&job, &Control::default()).reply;

        let reply = reply.unwrap_or_else(|e| panic!("{}: {e}", preset.name));
        assert!(
            reply.to_lowercase().contains("hi"),
            "{}: {reply}",
            preset.name
        );
    }
}
