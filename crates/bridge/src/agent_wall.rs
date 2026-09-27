//! The wall of the agent process (SPEC.md 6.6.4, "The agent process behind the proxy"):
//! `bwrap` on Linux with a network of its own, a new `/proc`, private `/run` and `/tmp`,
//! and read-only startup files. Its only way out is a proxy of the run. The agent keeps
//! its other file writes, so it still keeps its sessions and its login.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use crate::allow_hosts::{Defaults, HostList};
use crate::config::Kind;
use crate::forward::{FORWARD_FLAG, INNER_PORT, ports_arg};
use crate::proxy::{Proxy, ProxySettings};
use crate::story_sandbox::{self, Sandbox};

/// The flag of the forwarder that starts the agent with no shell.
pub const EXEC_FLAG: &str = "--exec";
/// Where the agent finds the socket of its proxy inside the wall. `/run` is private there.
pub const INNER_SOCKET: &str = "/run/gnomish-relay/agent.sock";
pub const NO_WALL: &str =
    "(No network wall for the agent on this computer: it has the full network.)";
/// These hold the sockets of the desktop, the ssh agent, and Docker.
const PRIVATE_FOLDERS: [&str; 4] = ["/run", "/tmp", "/var/tmp", "/dev/shm"];
/// The folders under the home folder that the scan for sockets looks into.
const SOCKET_DEPTH: usize = 3;
/// A home folder with more entries in its top levels gets no more of the scan.
const MAX_SCAN: usize = 200_000;

/// The model hosts of Claude (measured on Claude Code 2.1.283 on 2026-09-27). The second
/// one refreshes a login.
const CLAUDE_HOSTS: [&str; 2] = ["api.anthropic.com", "platform.claude.com"];
/// The model hosts of Codex (measured on codex-cli 0.157.0 on 2026-09-27).
const CODEX_HOSTS: [&str; 3] = ["api.openai.com", "chatgpt.com", "auth.openai.com"];

/// Code that runs later, outside the wall, with the full network (SPEC.md 6.6.4).
pub const STARTUP_FILES: [&str; 43] = [
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_logout",
    ".profile",
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".zlogin",
    ".zlogout",
    ".config/zsh",
    ".oh-my-zsh/custom",
    ".config/fish",
    ".config/systemd/user",
    ".local/share/systemd/user",
    ".config/autostart",
    ".config/environment.d",
    ".pam_environment",
    ".xprofile",
    ".xinitrc",
    ".config/plasma-workspace/env",
    ".local/bin",
    ".cargo/bin",
    ".ssh",
    ".gitconfig",
    ".config/git",
    ".cargo/config.toml",
    ".npmrc",
    ".config/pip",
    ".pip",
    ".pypirc",
    ".vimrc",
    ".config/nvim",
    ".config/direnv",
    ".gnupg",
    ".config/Code/User",
    ".claude/settings.json",
    ".claude/settings.local.json",
    ".claude/CLAUDE.md",
    ".claude/hooks",
    ".claude/commands",
    ".claude/agents",
    ".claude/skills",
];

/// Two more folders of the config of Claude and Codex: they start programs too.
const AGENT_CONFIG: [&str; 2] = [".claude/plugins", ".codex/config.toml"];

/// Which hosts the agent reaches through its proxy (`[sandbox] agent_network`).
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentNetwork {
    /// Any public host.
    #[default]
    Open,
    /// The model hosts of the backend and `agent_hosts`.
    Strict,
}

pub fn model_hosts(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Claude => &CLAUDE_HOSTS,
        Kind::Codex => &CODEX_HOSTS,
        Kind::Acp | Kind::Echo => &[],
    }
}

/// Every startup file and agent config, under `home`.
pub fn startup_paths(home: &Path) -> Vec<PathBuf> {
    STARTUP_FILES
        .iter()
        .chain(AGENT_CONFIG.iter())
        .map(|name| home.join(name))
        .collect()
}

/// The wall of this computer, for the agent processes of the bridge.
#[derive(Clone, Debug)]
pub struct AgentWall {
    /// Only `bwrap`. Seatbelt cannot nest, and Windows has none (SPEC.md 6.6.4).
    pub tool: Sandbox,
    /// This program: the forwarder inside the wall.
    pub wrapper: PathBuf,
    pub home: Option<PathBuf>,
    /// The socket of the proxy lies here, so S31 hides it from every command.
    pub data_dir: PathBuf,
    pub proxy: ProxySettings,
    /// The notice of no wall shows once for each start of the bridge.
    told: Arc<AtomicBool>,
}

impl AgentWall {
    pub fn new(
        tool: Sandbox,
        wrapper: PathBuf,
        home: Option<PathBuf>,
        data_dir: PathBuf,
        proxy: ProxySettings,
    ) -> AgentWall {
        AgentWall {
            tool,
            wrapper,
            home,
            data_dir,
            proxy,
            told: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn detect(data_dir: &Path, network: AgentNetwork, local_ports: &[u16]) -> AgentWall {
        let tool = match story_sandbox::detect() {
            Sandbox::Bwrap(bwrap) => Sandbox::Bwrap(bwrap),
            Sandbox::Seatbelt | Sandbox::None => Sandbox::None,
        };
        let proxy = match network {
            AgentNetwork::Open => ProxySettings::public(),
            AgentNetwork::Strict => ProxySettings::new(HostList::default()),
        };
        AgentWall::new(
            tool,
            std::env::current_exe().unwrap_or_default(),
            std::env::var_os("HOME").map(PathBuf::from),
            data_dir.to_owned(),
            proxy.with_local_ports(local_ports),
        )
    }

    /// No wall and no notice, as for a check with no model call.
    pub fn none() -> AgentWall {
        let wall = AgentWall::new(
            Sandbox::None,
            PathBuf::new(),
            None,
            PathBuf::new(),
            ProxySettings::public(),
        );
        wall.told.store(true, Ordering::Relaxed);
        wall
    }

    /// The wall of one backend: in `strict` mode its list is the model hosts of `kind`
    /// and the `agent_hosts` of its entry, which config load checked.
    #[must_use]
    pub fn for_agent(&self, kind: Kind, agent_hosts: &[String]) -> AgentWall {
        let mut more: Vec<String> = model_hosts(kind).iter().map(|h| (*h).to_owned()).collect();
        more.extend(agent_hosts.iter().cloned());
        let hosts = HostList::new(Defaults::Off, &more).unwrap_or_default();
        let mut wall = self.clone();
        wall.proxy.hosts = Arc::new(hosts);
        wall
    }

    pub fn is_on(&self) -> bool {
        matches!(self.tool, Sandbox::Bwrap(_))
    }

    /// One line for the log at start.
    pub fn summary(&self) -> String {
        if !self.is_on() {
            return "no wall: the agents have the full network".into();
        }
        let hosts = match self.proxy.mode {
            protocol::connect::Mode::Public => "any public host",
            protocol::connect::Mode::Listed => "only their model hosts and agent_hosts",
        };
        format!(
            "a bwrap wall, with {hosts} through the proxy and the local ports {:?}",
            self.proxy.local_ports
        )
    }

    /// `Some` once for each start of the bridge, on a computer with no wall.
    pub fn notice(&self) -> Option<&'static str> {
        if self.is_on() {
            return None;
        }
        let first = !self.told.swap(true, Ordering::Relaxed);
        first.then_some(NO_WALL)
    }

    /// The proxy and the walls of one agent process. `binds` are the folders that the
    /// agent writes, such as the chat folder and the temp folder of the run. `None` on a
    /// computer with no wall.
    pub fn prepare(&self, binds: &[&Path], tag: &str) -> Result<Option<RunWall>, String> {
        let Sandbox::Bwrap(bwrap) = &self.tool else {
            return Ok(None);
        };
        let place = self.data_dir.join("sandbox");
        std::fs::create_dir_all(&place)
            .map_err(|e| format!("No folder for the proxy of the agent: {e}"))?;
        let (listening, socket) = start_proxy(&place, self.proxy.clone(), tag)?;
        let startup = self.home.as_deref().map(startup_paths).unwrap_or_default();
        let (read_only, missing) = startup.into_iter().partition(|p| p.exists());
        let mut all_binds: Vec<PathBuf> = binds.iter().map(|b| b.to_path_buf()).collect();
        all_binds.push(self.data_dir.clone());
        let spec = WallSpec {
            bwrap: bwrap.clone(),
            forwarder: self.wrapper.clone(),
            socket: socket.clone(),
            binds: all_binds,
            read_only,
            sockets: self.home.as_deref().map(scan_sockets).unwrap_or_default(),
            local_ports: self.proxy.local_ports.to_vec(),
        };
        let (proxy, folder) = listening;
        Ok(Some(RunWall {
            spec,
            missing,
            socket,
            proxy: Some(proxy),
            _folder: folder,
        }))
    }
}

/// The proxy of the agent, and the open folder of its socket.
type Listening = (Proxy, std::fs::File);

/// The proxy of the agent on a Unix socket with a random name in `place`.
fn start_proxy(
    place: &Path,
    settings: ProxySettings,
    tag: &str,
) -> Result<(Listening, PathBuf), String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| format!("No random bytes from the OS: {e}"))?;
    let name = bytes.iter().fold(String::from("agent-"), |mut name, b| {
        let _ = write!(name, "{b:02x}");
        name
    }) + ".sock";
    let listening = listen(place, &name, settings, tag)
        .map_err(|e| format!("The proxy of the agent did not start: {e}"))?;
    Ok((listening, place.join(name)))
}

/// A socket path holds at most 108 bytes, and a data folder can be longer. So the bind
/// goes through the open folder, `/proc/self/fd/<folder>/<name>`, which is short.
#[cfg(target_os = "linux")]
fn listen(
    place: &Path,
    name: &str,
    settings: ProxySettings,
    tag: &str,
) -> std::io::Result<Listening> {
    use std::os::fd::AsRawFd;
    let folder = std::fs::File::open(place)?;
    let short = PathBuf::from(format!("/proc/self/fd/{}/{name}", folder.as_raw_fd()));
    let proxy = crate::proxy::listen_unix(&short, settings, tag.to_owned())?;
    Ok((proxy, folder))
}

/// The wall is `bwrap`, which only Linux has.
#[cfg(not(target_os = "linux"))]
fn listen(
    _place: &Path,
    _name: &str,
    _settings: ProxySettings,
    _tag: &str,
) -> std::io::Result<Listening> {
    Err(std::io::ErrorKind::Unsupported.into())
}

/// Each socket file in the top levels of the home folder. The walk does not follow links.
pub fn scan_sockets(home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut seen = 0;
    let mut folders = vec![(home.to_path_buf(), 0)];
    while let Some((folder, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_SCAN {
                return found;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if is_socket(kind) {
                found.push(entry.path());
            } else if kind.is_dir() && depth + 1 < SOCKET_DEPTH {
                folders.push((entry.path(), depth + 1));
            }
        }
    }
    found.sort();
    found
}

#[cfg(unix)]
fn is_socket(kind: std::fs::FileType) -> bool {
    use std::os::unix::fs::FileTypeExt;
    kind.is_socket()
}

#[cfg(not(unix))]
fn is_socket(_kind: std::fs::FileType) -> bool {
    false
}

/// What the `bwrap` arguments of an agent are made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallSpec {
    pub bwrap: PathBuf,
    pub forwarder: PathBuf,
    /// The socket of the proxy, outside the wall.
    pub socket: PathBuf,
    /// Folders that the agent writes, bound back when a private folder covers them.
    pub binds: Vec<PathBuf>,
    /// Startup files that exist.
    pub read_only: Vec<PathBuf>,
    /// Socket files of the home folder, each covered with `/dev/null`.
    pub sockets: Vec<PathBuf>,
    pub local_ports: Vec<u16>,
}

fn os(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

/// Strictly inside: a bind of all of `/tmp` would bring back every socket in it.
fn in_private_folder(path: &Path) -> bool {
    PRIVATE_FOLDERS
        .iter()
        .any(|f| path.starts_with(f) && path != Path::new(f))
}

/// The whole disk as it is, then the private folders, then the binds back, the socket of
/// the proxy, the read-only startup files, and the covered sockets. The agent and its
/// arguments come last, each one as it is: no shell.
pub fn wall_args(spec: &WallSpec, program: &Path, args: &[String]) -> Vec<OsString> {
    let mut out = os(&["--dev-bind", "/", "/"]);
    for folder in PRIVATE_FOLDERS {
        out.extend(os(&["--tmpfs", folder]));
    }
    for path in spec.binds.iter().filter(|p| in_private_folder(p)) {
        out.extend(["--bind".into(), path.into(), path.into()]);
    }
    out.extend([
        "--bind".into(),
        spec.socket.clone().into(),
        INNER_SOCKET.into(),
    ]);
    for path in &spec.read_only {
        out.extend(["--ro-bind".into(), path.into(), path.into()]);
    }
    for path in &spec.sockets {
        out.extend(["--ro-bind".into(), "/dev/null".into(), path.into()]);
    }
    out.extend(os(&[
        "--unshare-net",
        "--unshare-pid",
        "--proc",
        "/proc",
        "--die-with-parent",
        "--new-session",
        "--",
    ]));
    out.extend([
        spec.forwarder.clone().into(),
        FORWARD_FLAG.into(),
        INNER_SOCKET.into(),
        ports_arg(&spec.local_ports).into(),
        EXEC_FLAG.into(),
        program.into(),
    ]);
    out.extend(args.iter().map(OsString::from));
    out
}

/// The wall of one agent process. Its proxy stops, and its socket goes away, with it.
pub struct RunWall {
    pub spec: WallSpec,
    /// The startup files that were missing at the start.
    missing: Vec<PathBuf>,
    socket: PathBuf,
    /// `None` only in `drop`.
    proxy: Option<Proxy>,
    /// The short path of the socket goes through this open folder.
    _folder: std::fs::File,
}

impl Drop for RunWall {
    /// The proxy stops first: it wakes its accept loop through the socket.
    fn drop(&mut self) {
        drop(self.proxy.take());
        let _ = std::fs::remove_file(&self.socket);
    }
}

impl RunWall {
    /// `bwrap` and its arguments, which start `program` with `args` inside the wall.
    pub fn launch(&self, program: &Path, args: &[String]) -> (PathBuf, Vec<OsString>) {
        (
            self.spec.bwrap.clone(),
            wall_args(&self.spec, program, args),
        )
    }

    /// The startup files that the agent made during the run (SPEC.md 6.6.4).
    pub fn made_startup_files(&self) -> Vec<PathBuf> {
        self.missing
            .iter()
            .filter(|p| p.exists())
            .cloned()
            .collect()
    }
}

/// The variables of an agent: the proxy inside a wall, where only the loopback of the
/// wall skips it, and no telemetry for Claude, with a wall or not.
pub fn agent_env(kind: Kind, walled: Walled) -> Vec<(String, OsString)> {
    let mut env = Vec::new();
    if walled == Walled::Yes {
        let url = OsString::from(format!("http://127.0.0.1:{INNER_PORT}"));
        for name in crate::command_sandbox::PROXY_VARS {
            env.push((name.to_owned(), url.clone()));
        }
        let loopback = crate::command_sandbox::NO_PROXY;
        env.push(("NO_PROXY".into(), loopback.into()));
        env.push(("no_proxy".into(), loopback.into()));
    }
    if kind == Kind::Claude {
        // No telemetry and no update check. The update would write `~/.local/bin`,
        // which is read-only in the wall.
        env.push((
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".into(),
            "1".into(),
        ));
    }
    env
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walled {
    Yes,
    No,
}

impl Walled {
    pub fn of(wall: Option<&RunWall>) -> Walled {
        if wall.is_some() {
            Walled::Yes
        } else {
            Walled::No
        }
    }
}

/// The reply with the notes of the bridge before it, and the note of the made startup
/// files after it.
pub fn with_notes(reply: String, before: &[&str], after: Option<String>) -> String {
    let mut parts: Vec<String> = before.iter().map(|n| (*n).to_owned()).collect();
    parts.push(reply);
    parts.extend(after);
    parts.join("\n\n")
}

/// The notice for each startup file that the agent made, for the end of the reply.
pub fn made_notice(made: &[PathBuf], home: Option<&Path>) -> Option<String> {
    if made.is_empty() {
        return None;
    }
    let shown: Vec<String> = made
        .iter()
        .map(|p| match home.and_then(|h| p.strip_prefix(h).ok()) {
            Some(rest) => format!("~/{}", rest.display()),
            None => p.display().to_string(),
        })
        .collect();
    Some(format!(
        "The agent made {} during the run. Check it before you open a new terminal.",
        shown.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> WallSpec {
        WallSpec {
            bwrap: PathBuf::from("/usr/bin/bwrap"),
            forwarder: PathBuf::from("/usr/bin/gnomish-relay"),
            socket: PathBuf::from("/home/x/.local/share/gnomish-relay/sandbox/agent-1.sock"),
            binds: vec![
                PathBuf::from("/home/x/Code/app"),
                PathBuf::from("/tmp/gnomish-relay-run-1"),
            ],
            read_only: vec![PathBuf::from("/home/x/.bashrc")],
            sockets: vec![PathBuf::from("/home/x/.docker/desktop/docker.sock")],
            local_ports: vec![5432],
        }
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn position(args: &[String], word: &str) -> usize {
        args.iter().position(|a| a == word).unwrap()
    }

    #[test]
    fn the_wall_has_its_own_network_processes_and_proc() {
        let args = strings(&wall_args(&spec(), Path::new("/usr/bin/claude"), &[]));

        for flag in [
            "--unshare-net",
            "--unshare-pid",
            "--die-with-parent",
            "--new-session",
        ] {
            assert!(args.contains(&flag.to_owned()), "{flag}");
        }
        let proc_at = position(&args, "--proc");
        assert_eq!(args[proc_at + 1], "/proc");
        assert!(!args.contains(&"--share-net".to_owned()));
    }

    #[test]
    fn each_private_folder_comes_before_its_binds_back() {
        let args = strings(&wall_args(&spec(), Path::new("/usr/bin/claude"), &[]));

        let tmp = args
            .windows(2)
            .position(|w| w == ["--tmpfs", "/tmp"])
            .unwrap();
        let back = args
            .windows(3)
            .position(|w| w[0] == "--bind" && w[1] == "/tmp/gnomish-relay-run-1")
            .unwrap();
        assert!(tmp < back);
        assert!(!args.contains(&"/home/x/Code/app".to_owned()));
        let mut whole = spec();
        whole.binds = vec![PathBuf::from("/tmp"), PathBuf::from("/run/")];
        let args = strings(&wall_args(&whole, Path::new("/usr/bin/claude"), &[]));
        assert!(
            !args
                .windows(2)
                .any(|w| w[0] == "--bind" && w[1].starts_with("/tmp"))
        );
        assert!(
            !args
                .windows(2)
                .any(|w| w[0] == "--bind" && w[1].starts_with("/run"))
        );
        let socket = args.windows(3).position(|w| w[2] == INNER_SOCKET).unwrap();
        assert_eq!(args[socket], "--bind");
        assert!(position(&args, "--tmpfs") < socket);
    }

    #[test]
    fn the_startup_files_are_read_only_and_the_sockets_are_covered() {
        let args = strings(&wall_args(&spec(), Path::new("/usr/bin/claude"), &[]));

        assert!(
            args.windows(3)
                .any(|w| w == ["--ro-bind", "/home/x/.bashrc", "/home/x/.bashrc"])
        );
        assert!(args.windows(3).any(|w| {
            w == [
                "--ro-bind",
                "/dev/null",
                "/home/x/.docker/desktop/docker.sock",
            ]
        }));
    }

    #[test]
    fn the_forwarder_starts_the_agent_last_with_each_argument_as_it_is() {
        let args = strings(&wall_args(
            &spec(),
            Path::new("/usr/bin/claude"),
            &["-p".into(), "a b; rm -rf ~".into()],
        ));

        let end = position(&args, "--");
        assert_eq!(
            args[end + 1..],
            [
                "/usr/bin/gnomish-relay",
                FORWARD_FLAG,
                INNER_SOCKET,
                "5432",
                EXEC_FLAG,
                "/usr/bin/claude",
                "-p",
                "a b; rm -rf ~",
            ][..]
        );
    }

    #[test]
    fn claude_and_codex_have_their_model_hosts_and_other_agents_have_none() {
        assert!(model_hosts(Kind::Claude).contains(&"api.anthropic.com"));
        assert!(model_hosts(Kind::Codex).contains(&"chatgpt.com"));
        assert!(model_hosts(Kind::Acp).is_empty());
        for host in model_hosts(Kind::Claude)
            .iter()
            .chain(model_hosts(Kind::Codex))
        {
            assert!(crate::allow_hosts::check_host_name(host).is_ok(), "{host}");
        }
    }

    #[test]
    fn the_strict_list_of_an_agent_is_its_model_hosts_and_its_agent_hosts() {
        let base = AgentWall::none();

        let wall = base.for_agent(Kind::Claude, &["bedrock.example.com".into()]);

        assert!(wall.proxy.hosts.allows("api.anthropic.com"));
        assert!(wall.proxy.hosts.allows("bedrock.example.com"));
        assert!(!wall.proxy.hosts.allows("chatgpt.com"));
        assert!(base.proxy.hosts.is_empty());
    }

    #[test]
    fn the_agent_gets_the_proxy_in_every_name_and_claude_sends_no_telemetry() {
        let env = agent_env(Kind::Claude, Walled::Yes);
        let value = |name: &str| env.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());

        assert_eq!(value("HTTPS_PROXY"), Some("http://127.0.0.1:3128".into()));
        assert_eq!(value("NO_PROXY"), Some("localhost,127.0.0.1,::1".into()));
        assert_eq!(
            value("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"),
            Some("1".into())
        );
        let codex = agent_env(Kind::Codex, Walled::Yes);
        assert!(!codex.iter().any(|(n, _)| n.starts_with("CLAUDE")));
    }

    #[test]
    fn with_no_wall_the_agent_gets_no_proxy_and_claude_still_no_telemetry() {
        let env = agent_env(Kind::Claude, Walled::No);

        assert_eq!(
            env,
            vec![(
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_owned(),
                OsString::from("1")
            )]
        );
        assert!(agent_env(Kind::Acp, Walled::No).is_empty());
        assert_eq!(Walled::of(None), Walled::No);
    }

    #[test]
    fn the_notes_of_the_bridge_go_around_the_reply() {
        assert_eq!(
            with_notes("hi".into(), &["(a)", "(b)"], Some("made".into())),
            "(a)\n\n(b)\n\nhi\n\nmade"
        );
        assert_eq!(with_notes("hi".into(), &[], None), "hi");
    }

    #[test]
    fn with_no_wall_the_notice_shows_once() {
        let wall = AgentWall::new(
            Sandbox::None,
            PathBuf::new(),
            None,
            PathBuf::new(),
            ProxySettings::public(),
        );

        assert_eq!(wall.notice(), Some(NO_WALL));
        assert_eq!(wall.notice(), None);
        assert!(wall.prepare(&[], "chat test").unwrap().is_none());
        assert_eq!(AgentWall::none().notice(), None);
        assert!(wall.summary().contains("full network"));
    }

    #[test]
    fn the_summary_of_a_wall_names_its_mode_and_its_local_ports() {
        let open = AgentWall::new(
            Sandbox::Bwrap(PathBuf::from("/usr/bin/bwrap")),
            PathBuf::new(),
            None,
            PathBuf::new(),
            ProxySettings::public().with_local_ports(&[5432]),
        );
        let mut strict = open.clone();
        strict.proxy.mode = protocol::connect::Mode::Listed;

        assert!(
            open.summary().contains("any public host"),
            "{}",
            open.summary()
        );
        assert!(open.summary().contains("5432"), "{}", open.summary());
        assert!(
            strict.summary().contains("model hosts"),
            "{}",
            strict.summary()
        );
    }

    #[test]
    fn the_notice_of_a_made_startup_file_names_it_from_the_home_folder() {
        let home = Path::new("/home/x");
        let made = [home.join(".zshrc"), PathBuf::from("/etc/other")];

        let notice = made_notice(&made, Some(home)).unwrap();

        assert!(notice.contains("~/.zshrc, /etc/other"), "{notice}");
        assert!(notice.contains("Check it before you open a new terminal."));
        assert_eq!(made_notice(&[], Some(home)), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_scan_finds_the_sockets_in_the_top_levels_of_the_home_folder() {
        let home = tempfile::tempdir().unwrap();
        let near = home.path().join(".docker/desktop");
        let deep = home.path().join("a/b/c");
        std::fs::create_dir_all(&near).unwrap();
        std::fs::create_dir_all(&deep).unwrap();
        let _a = std::os::unix::net::UnixListener::bind(near.join("docker.sock")).unwrap();
        let _b = std::os::unix::net::UnixListener::bind(deep.join("far.sock")).unwrap();
        std::fs::write(near.join("plain"), "x").unwrap();

        let found = scan_sockets(home.path());

        assert_eq!(found, vec![near.join("docker.sock")]);
    }

    #[test]
    fn the_startup_paths_hold_the_shells_the_desktop_and_the_agent_config() {
        let paths = startup_paths(Path::new("/home/x"));

        for name in [
            ".bashrc",
            ".config/systemd/user",
            ".ssh",
            ".codex/config.toml",
        ] {
            assert!(paths.contains(&Path::new("/home/x").join(name)), "{name}");
        }
        assert!(!paths.contains(&PathBuf::from("/home/x/.claude.json")));
        assert!(!paths.contains(&PathBuf::from("/home/x/.claude/projects")));
    }
}
