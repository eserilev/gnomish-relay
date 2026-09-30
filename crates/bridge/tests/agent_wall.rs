//! The wall of the agent process with the real `bwrap` (SPEC.md 6.6.4, "The agent
//! process behind the proxy"). Each fake agent runs the probes of a `net ...` prompt
//! inside its wall. With no working `bwrap` the tests skip, unless
//! `GNOMISH_REQUIRE_BWRAP` is set, as in CI.
#![cfg(target_os = "linux")]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bridge::acp::AcpAgent;
use bridge::agent::{Agent, Control};
use bridge::agent_wall::{AgentWall, NO_WALL};
use bridge::allow_hosts::HostList;
use bridge::claude::ClaudeAgent;
use bridge::codex::CodexAgent;
use bridge::config::{Kind, Permission};
use bridge::gate::Gate;
use bridge::process::AgentProcess;
use bridge::proxy::{Limits, Net, ProxySettings};
use bridge::relay::{ChatId, Job, MessageId, Session, Work};
use bridge::story_sandbox::{self, Sandbox};

/// `None` skips the test on a computer with no working `bwrap`.
fn bwrap() -> Option<Sandbox> {
    let tool = story_sandbox::detect();
    if matches!(tool, Sandbox::Bwrap(_)) {
        return Some(tool);
    }
    assert!(
        std::env::var_os("GNOMISH_REQUIRE_BWRAP").is_none(),
        "bwrap is missing or does not work, and GNOMISH_REQUIRE_BWRAP is set"
    );
    eprintln!("skipped: bwrap is missing or does not work");
    None
}

/// A public address that stands for the fake server behind the proxy.
const PUBLIC: [u8; 4] = [93, 184, 216, 34];

/// A web server on this computer that answers with the first line of each request.
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

/// A server on the loopback of this computer, for `local_ports`.
fn local_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut line = String::new();
            let _ = io::BufReader::new(&stream).read_line(&mut line);
            let _ = writeln!(stream, "local {}", line.trim_end());
        }
    });
    port
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
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

/// A home with a startup file and a socket of the desktop, a data folder, and a chat
/// folder. The root is not under `/tmp`, because the wall covers `/tmp`.
struct Machine {
    _root: tempfile::TempDir,
    home: PathBuf,
    data: PathBuf,
    chat: tempfile::TempDir,
    _docker: (std::os::unix::net::UnixListener, fs::File),
}

/// A socket path holds at most 108 bytes, so the bind goes through the open folder.
fn bind_socket(path: &std::path::Path) -> (std::os::unix::net::UnixListener, fs::File) {
    use std::os::fd::AsRawFd;
    let folder = fs::File::open(path.parent().unwrap()).unwrap();
    let name = path.file_name().unwrap().to_string_lossy();
    let short = format!("/proc/self/fd/{}/{name}", folder.as_raw_fd());
    (
        std::os::unix::net::UnixListener::bind(short).unwrap(),
        folder,
    )
}

fn test_root() -> PathBuf {
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(&target).unwrap();
    target
}

fn machine() -> Machine {
    let root = tempfile::tempdir_in(test_root()).unwrap();
    let home = root.path().canonicalize().unwrap().join("home");
    let data = home.join(".local/share/gnomish-relay");
    fs::create_dir_all(home.join(".docker/desktop")).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(home.join(".bashrc"), "old rc").unwrap();
    let docker = bind_socket(&home.join(".docker/desktop/docker.sock"));
    Machine {
        _root: root,
        home,
        data,
        chat: tempfile::tempdir().unwrap(),
        _docker: docker,
    }
}

fn settings(local_ports: &[u16]) -> ProxySettings {
    let settings = ProxySettings {
        net: Net {
            resolve: fake_resolve,
            connect: fake_connect,
        },
        limits: Limits::default(),
        ..ProxySettings::public()
    };
    settings.with_local_ports(local_ports)
}

fn wall(m: &Machine, tool: Sandbox, settings: ProxySettings) -> AgentWall {
    AgentWall::new(
        tool,
        PathBuf::from(env!("CARGO_BIN_EXE_gnomish-relay")),
        Some(m.home.clone()),
        m.data.clone(),
        settings,
    )
}

fn gate(m: &Machine) -> Gate {
    Gate {
        // It marks the commands as sandboxed, so no notice of the command sandbox shows.
        // These probes run no command.
        sandbox: bridge::command_sandbox::CommandSandbox::new(
            Sandbox::Seatbelt,
            PathBuf::from(env!("CARGO_BIN_EXE_gnomish-relay")),
            None,
        ),
        ..Gate::bare(
            vec![m.chat.path().canonicalize().unwrap()],
            m.home.join(".config/gnomish-relay"),
            m.data.clone(),
        )
    }
}

fn job(m: &Machine, text: &str) -> Job {
    Job {
        token: "tok".into(),
        chat: ChatId::new("c1"),
        id: MessageId(1),
        agent: "fake".into(),
        permission: Permission::AutoEdit,
        asked: Permission::AutoEdit,
        cwd: m.chat.path().to_string_lossy().into_owned(),
        session: Session::New,
        resume: None,
        text: text.into(),
        work: Work::Prompt,
        new_folder: false,
    }
}

fn claude(m: &Machine, wall: AgentWall) -> Box<dyn Agent> {
    Box::new(ClaudeAgent {
        command: vec![env!("CARGO_BIN_EXE_fake-claude").into(), "net".into()],
        env: Vec::new(),
        modes: BTreeMap::new(),
        timeout: Duration::from_secs(30),
        permission_timeout: Duration::from_secs(30),
        projects: m.home.join(".claude/projects"),
        gate: gate(m),
        wall,
    })
}

fn codex(m: &Machine, wall: AgentWall) -> Box<dyn Agent> {
    Box::new(CodexAgent {
        command: vec![env!("CARGO_BIN_EXE_fake-codex").into(), "net".into()],
        env: Vec::new(),
        timeout: Duration::from_secs(30),
        permission_timeout: Duration::from_secs(30),
        gate: gate(m),
        wall,
    })
}

fn acp(m: &Machine, wall: AgentWall) -> Box<dyn Agent> {
    Box::new(AcpAgent {
        command: vec![env!("CARGO_BIN_EXE_fake-acp-agent").into(), "net".into()],
        env: Vec::new(),
        modes: BTreeMap::new(),
        timeout: Duration::from_secs(30),
        permission_timeout: Duration::from_secs(30),
        gate: gate(m),
        wall,
    })
}

type Backend = fn(&Machine, AgentWall) -> Box<dyn Agent>;

const BACKENDS: [(&str, Backend); 3] = [("claude", claude), ("codex", codex), ("acp", acp)];

fn probe(m: &Machine, agent: &dyn Agent, probes: &str) -> String {
    agent
        .run(&job(m, &format!("net {probes}")), &Control::default())
        .reply
        .unwrap()
}

#[test]
fn a_direct_connection_fails_and_a_public_host_works_through_the_proxy_for_each_agent() {
    let Some(tool) = bwrap() else { return };
    let m = machine();
    let outside = server();

    for (name, backend) in BACKENDS {
        let agent = backend(&m, wall(&m, tool.clone(), settings(&[])));

        let reply = probe(
            &m,
            &*agent,
            &format!("direct={outside} proxy=allowed.test:443"),
        );

        assert_eq!(
            reply, "direct=fail proxy=200:hello_GET_/agent_HTTP/1.1",
            "{name}"
        );
    }
}

#[test]
fn a_name_that_leads_to_this_computer_is_refused_in_the_open_mode() {
    let Some(tool) = bwrap() else { return };
    let m = machine();
    let agent = claude(&m, wall(&m, tool, settings(&[])));

    let reply = probe(&m, &*agent, "proxy=local.test:443 proxy=other.test:22");

    assert_eq!(reply, "proxy=403 proxy=403");
}

#[test]
fn the_strict_mode_reaches_only_the_model_hosts_and_the_agent_hosts() {
    let Some(tool) = bwrap() else { return };
    let m = machine();
    let strict = ProxySettings {
        hosts: Arc::new(HostList::default()),
        mode: protocol::connect::Mode::Listed,
        ..settings(&[])
    };
    let base = wall(&m, tool, strict);

    for (name, backend) in BACKENDS {
        let agent = backend(&m, base.for_agent(Kind::Acp, &["allowed.test".into()]));

        let reply = probe(&m, &*agent, "proxy=allowed.test:443 proxy=other.test:443");

        assert_eq!(
            reply, "proxy=200:hello_GET_/agent_HTTP/1.1 proxy=403",
            "{name}"
        );
    }
}

#[test]
fn a_listed_local_port_works_and_another_port_of_this_computer_stays_closed() {
    let Some(tool) = bwrap() else { return };
    let m = machine();
    let listed = local_server();
    let other = local_server();
    let agent = codex(&m, wall(&m, tool, settings(&[listed])));

    let reply = probe(
        &m,
        &*agent,
        &format!("local={listed} direct=127.0.0.1:{other}"),
    );

    assert_eq!(reply, "local=local_ping direct=fail");
}

#[test]
fn a_server_of_the_agent_works_on_the_loopback_of_its_wall() {
    let Some(tool) = bwrap() else { return };
    let m = machine();
    let agent = acp(&m, wall(&m, tool, settings(&[])));

    let reply = probe(&m, &*agent, &format!("serve={}", free_port()));

    assert_eq!(reply, "serve=served");
}

#[test]
fn the_agent_sees_no_process_and_no_socket_of_this_computer() {
    let Some(tool) = bwrap() else { return };
    let m = machine();
    let tmp_socket_dir = tempfile::tempdir().unwrap();
    let tmp_socket = tmp_socket_dir.path().join("desktop.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&tmp_socket).unwrap();
    let docker = m.home.join(".docker/desktop/docker.sock");
    let agent = claude(&m, wall(&m, tool, settings(&[])));

    let reply = probe(
        &m,
        &*agent,
        &format!(
            "proc={} sock={} sock={}",
            std::process::id(),
            docker.display(),
            tmp_socket.display()
        ),
    );

    assert_eq!(reply, "proc=no sock=fail sock=fail");
}

#[test]
fn a_startup_file_cannot_change_and_a_new_one_gets_a_notice() {
    let Some(tool) = bwrap() else { return };
    let m = machine();
    let bashrc = m.home.join(".bashrc");
    let zshrc = m.home.join(".zshrc");
    let agent = claude(&m, wall(&m, tool, settings(&[])));

    let reply = probe(
        &m,
        &*agent,
        &format!("write={} write={}", bashrc.display(), zshrc.display()),
    );

    assert!(reply.starts_with("write=fail write=ok"), "{reply}");
    assert!(
        reply.ends_with(
            "The agent made ~/.zshrc during the run. Check it before you open a new terminal."
        ),
        "{reply}"
    );
    assert_eq!(fs::read_to_string(bashrc).unwrap(), "old rc");
}

#[test]
fn with_no_wall_the_first_reply_carries_the_notice() {
    let m = machine();
    let no_wall = wall(&m, Sandbox::None, settings(&[]));
    let agent = acp(&m, no_wall);

    let first = probe(&m, &*agent, "proc=1");
    let second = probe(&m, &*agent, "proc=1");

    assert!(first.starts_with(NO_WALL), "{first}");
    assert!(!second.contains(NO_WALL), "{second}");
}

/// A child of a child that writes a file every 100 ms. After the agent process ends, the
/// file stops growing: the whole tree ended with the wall.
#[test]
fn a_grandchild_of_the_agent_ends_with_the_wall() {
    let Some(tool) = bwrap() else { return };
    let m = machine();
    let beat = m.data.join("beat");
    let run = wall(&m, tool, settings(&[]))
        .prepare(&[m.chat.path()], "test")
        .unwrap()
        .unwrap();
    let script = format!(
        "(while true; do echo x >> '{}'; sleep 0.1; done) & sleep 300",
        beat.display()
    );
    let command = ["/bin/sh".to_owned(), "-c".to_owned(), script];
    let process = AgentProcess::start_in(&command, &[], &[], &[], "/", Some(&run)).unwrap();
    wait_for(|| beat.exists());

    drop(process);
    std::thread::sleep(Duration::from_millis(500));
    let before = fs::read_to_string(&beat).unwrap().len();
    std::thread::sleep(Duration::from_millis(600));
    let after = fs::read_to_string(&beat).unwrap().len();

    assert_eq!(before, after, "the grandchild still runs");
}

#[test]
fn the_socket_of_the_agent_proxy_lies_in_the_data_folder_and_goes_away_with_the_run() {
    let Some(tool) = bwrap() else { return };
    let m = machine();

    let run = wall(&m, tool, settings(&[]))
        .prepare(&[m.chat.path()], "test")
        .unwrap()
        .unwrap();
    let socket = run.spec.socket.clone();

    assert!(
        socket.starts_with(m.data.join("sandbox")),
        "{}",
        socket.display()
    );
    assert!(socket.exists());
    drop(run);
    assert!(!socket.exists());
}

/// Claude in its wall runs a command through the real command sandbox, which nests in
/// the wall. The command reaches the proxy of the commands, with its short list, and
/// never the proxy of the agent, which takes any public host.
#[test]
fn a_command_of_claude_runs_in_the_command_sandbox_inside_the_wall() {
    let Some(tool) = bwrap() else { return };
    let m = machine();
    let names = ["allowed.test".to_owned()];
    let command_proxy = ProxySettings {
        net: Net {
            resolve: fake_resolve,
            connect: fake_connect,
        },
        ..ProxySettings::new(HostList::new(bridge::allow_hosts::Defaults::Off, &names).unwrap())
    };
    let mut gate = gate(&m);
    gate.sandbox = bridge::command_sandbox::CommandSandbox::new(
        tool.clone(),
        PathBuf::from(env!("CARGO_BIN_EXE_gnomish-relay")),
        None,
    )
    .with_proxy(command_proxy);
    // In a file, so the classifier sees one `bash` (ask), which full-auto runs.
    let script = "curl -sS --max-time 10 --proxytunnel http://allowed.test/cmd > allowed.txt\n\
        curl -sS --max-time 10 https://other.test/ 2> other.txt\n\
        ls /run/gnomish-relay 2> sock.txt\n";
    fs::write(m.chat.path().join("probe.sh"), script).unwrap();
    let input = serde_json::json!({ "command": "bash probe.sh" }).to_string();
    let agent = ClaudeAgent {
        command: vec![
            env!("CARGO_BIN_EXE_fake-claude").into(),
            "tool".into(),
            "Bash".into(),
            input,
        ],
        env: Vec::new(),
        modes: BTreeMap::new(),
        timeout: Duration::from_secs(30),
        permission_timeout: Duration::from_secs(30),
        projects: m.home.join(".claude/projects"),
        gate,
        wall: wall(&m, tool, settings(&[])),
    };
    let mut full_auto = job(&m, "run it");
    full_auto.permission = Permission::FullAuto;
    full_auto.asked = Permission::FullAuto;

    let reply = agent.run(&full_auto, &Control::default()).reply.unwrap();

    let read = |name: &str| fs::read_to_string(m.chat.path().join(name)).unwrap_or_default();
    assert_eq!(reply, "allow: Allowed by Gnomish Relay.");
    assert_eq!(read("allowed.txt"), "hello GET /cmd HTTP/1.1");
    assert!(read("other.txt").contains("403"), "{}", read("other.txt"));
    assert!(
        read("sock.txt").contains("No such file"),
        "{}",
        read("sock.txt")
    );
}

/// The real `claude` in its wall, in both modes of the network, runs a Bash command in
/// the real command sandbox: `cargo test --test agent_wall live -- --ignored`.
#[test]
#[ignore = "live: needs claude, a Claude login, and bwrap"]
fn live_claude_answers_in_its_wall_and_runs_a_command_in_the_command_sandbox() {
    let Some(tool) = bwrap() else { return };
    for strict in [false, true] {
        let m = machine();
        let mut gate = gate(&m);
        gate.sandbox = bridge::command_sandbox::CommandSandbox::new(
            tool.clone(),
            PathBuf::from(env!("CARGO_BIN_EXE_gnomish-relay")),
            None,
        );
        let mut base = AgentWall::detect(&m.data, bridge::agent_wall::AgentNetwork::Open, &[]);
        if strict {
            base = AgentWall::detect(&m.data, bridge::agent_wall::AgentNetwork::Strict, &[]);
        }
        base.wrapper = PathBuf::from(env!("CARGO_BIN_EXE_gnomish-relay"));
        let agent = ClaudeAgent {
            command: vec!["claude".into(), "--model".into(), "haiku".into()],
            env: Vec::new(),
            modes: BTreeMap::new(),
            timeout: Duration::from_mins(3),
            permission_timeout: Duration::from_secs(5),
            projects: PathBuf::from(std::env::var("HOME").unwrap()).join(".claude/projects"),
            gate,
            wall: base.for_agent(Kind::Claude, &[]),
        };
        let mut full_auto = job(
            &m,
            "Run exactly this Bash command: bash -c 'echo made > made.txt'. Then reply DONE.",
        );
        full_auto.permission = Permission::FullAuto;
        full_auto.asked = Permission::FullAuto;

        let reply = agent.run(&full_auto, &Control::default()).reply;

        let made = fs::read_to_string(m.chat.path().join("made.txt")).unwrap_or_default();
        assert!(reply.is_ok(), "strict={strict}: {reply:?}");
        assert_eq!(made, "made\n", "strict={strict}: {reply:?}");
    }
}

fn wait_for(done: impl Fn() -> bool) {
    for _ in 0..100 {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("it never happened");
}
