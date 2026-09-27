//! The sandbox of the commands of a run from the game (SPEC.md 6.6.4). `protocol` builds
//! the policy (S31), and this module makes it real: bubblewrap on Linux and Seatbelt on
//! macOS. Claude Code runs each command through this program (`CLAUDE_CODE_SHELL_PREFIX`),
//! and this program starts the command inside the walls of its run.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use protocol::sandbox::{SandboxPolicy, is_hidden, sandbox_policy};
use protocol::sbpl::sbpl_string;
use serde::{Deserialize, Serialize};

use crate::action_input::{DESKTOP_PATHS, DESKTOP_WRITES, resolve, resolved_bytes};
use crate::allow_hosts::HostList;
use crate::forward::INNER_PORT;
use crate::holder::{Holder, check_holder, holder_file, join_args, start_holder};
use crate::process::BASE_ENV;
use crate::proxy::{self, Proxy, ProxySettings};
use crate::story_sandbox::{self, Sandbox};

/// The flag of this program that runs one command inside the walls of a run.
pub const RUN_FLAG: &str = "--sandbox-run";
/// Names the walls file of the run, for the wrapper.
pub const WALLS_VAR: &str = "GNOMISH_RELAY_SANDBOX";
/// The wrapper writes it into the temp folder, so the bridge sees that the wrapper ran.
const MARKER: &str = ".gnomish-relay-sandbox";
/// A folder with more is too large to check at the start of each run.
const MAX_WALK: usize = 1_000_000;
/// Git outside the sandbox runs what these name, so a git folder in the chat folder hides
/// them. `commondir` points a linked worktree at its repository.
const GIT_FOLDER_GUARDED: [&str; 4] = ["config", "hooks", "commondir", "config.worktree"];
pub const NO_SANDBOX: &str = "(No sandbox on this computer: every command asks in the game.)";
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
/// The Unix socket of the proxy, in the temp folder of the run.
const PROXY_SOCKET: &str = ".gnomish-relay-proxy";
/// The tools that keep their downloads in the home folder: cargo and rustup.
const TOOL_HOMES: [&str; 2] = [".cargo", ".rustup"];
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether `bwrap` can lay a copy-on-write view over a folder (bwrap 0.9 and Linux 5.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    Works,
    Missing,
}

/// Runs `bwrap --version` inside a sandbox with a copy-on-write view of `/etc`.
pub fn detect_overlay(tool: &Sandbox) -> Overlay {
    let Sandbox::Bwrap(bwrap) = tool else {
        return Overlay::Missing;
    };
    let bwrap = bwrap.to_string_lossy().into_owned();
    let args: Vec<String> = [
        "--ro-bind",
        "/",
        "/",
        "--overlay-src",
        "/etc",
        "--tmp-overlay",
        "/etc",
        "--unshare-all",
        "--die-with-parent",
        "--",
        &bwrap,
        "--version",
    ]
    .map(str::to_owned)
    .into();
    let out = crate::process::output(std::slice::from_ref(&bwrap), &args, &[], "/", PROBE_TIMEOUT);
    if out.is_ok_and(|out| out.success) {
        Overlay::Works
    } else {
        Overlay::Missing
    }
}

/// The sandbox of this computer, for the commands of runs from the game.
#[derive(Clone, Debug)]
pub struct CommandSandbox {
    pub tool: Sandbox,
    /// This program. Claude Code runs it before each command.
    pub wrapper: PathBuf,
    pub home: Option<PathBuf>,
    /// With no proxy, commands have no network at all.
    pub proxy: Option<ProxySettings>,
    /// With no overlay, a download of cargo or rustup fails on the read-only home folder.
    pub overlay: Overlay,
    /// The notice of no sandbox shows once for each start of the bridge.
    told: Arc<AtomicBool>,
}

impl CommandSandbox {
    pub fn new(tool: Sandbox, wrapper: PathBuf, home: Option<PathBuf>) -> CommandSandbox {
        CommandSandbox {
            tool,
            wrapper,
            home,
            proxy: None,
            overlay: Overlay::Missing,
            told: Arc::new(AtomicBool::new(false)),
        }
    }

    #[must_use]
    pub fn with_proxy(mut self, settings: ProxySettings) -> CommandSandbox {
        self.proxy = Some(settings);
        self
    }

    /// Windows has none: Git Bash cannot start in an `AppContainer` (SPEC.md 6.6.4).
    pub fn detect(hosts: HostList, local_ports: &[u16]) -> CommandSandbox {
        let wrapper = std::env::current_exe().unwrap_or_default();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut sandbox = CommandSandbox::new(with_nsenter(story_sandbox::detect()), wrapper, home);
        sandbox.overlay = detect_overlay(&sandbox.tool);
        if hosts.is_empty() && local_ports.is_empty() {
            return sandbox;
        }
        sandbox.with_proxy(ProxySettings::new(hosts).with_local_ports(local_ports))
    }

    pub fn none() -> CommandSandbox {
        CommandSandbox::new(Sandbox::None, PathBuf::new(), None)
    }

    /// One line for the log at start.
    pub fn summary(&self) -> String {
        let proxy = if self.proxy.is_some() {
            "the allowed hosts through the proxy"
        } else {
            "no network"
        };
        let downloads = match (&self.tool, self.overlay) {
            (Sandbox::Bwrap(_), Overlay::Works) => {
                ", and downloads of cargo and rustup go to the temp folder of the run"
            }
            (Sandbox::None, _) => "",
            _ => ", and a new download of cargo or rustup fails",
        };
        format!("{} with {proxy}{downloads}", self.tool.name())
    }

    pub fn is_on(&self) -> bool {
        self.tool != Sandbox::None
    }

    /// The notice for the first reply of a run with no sandbox.
    pub fn notice(&self) -> Option<&'static str> {
        let first = !self.told.swap(true, Ordering::Relaxed);
        first.then_some(NO_SANDBOX)
    }
}

/// Each command joins the sandbox of its run with `nsenter` (`holder.rs`), so `bwrap`
/// with no `nsenter` counts as no sandbox.
fn with_nsenter(tool: Sandbox) -> Sandbox {
    let path = std::env::var_os("PATH").unwrap_or_default();
    match tool {
        Sandbox::Bwrap(_) if crate::program::find_program("nsenter", &path, false).is_none() => {
            Sandbox::None
        }
        other => other,
    }
}

/// What the wrapper needs for each command of one run.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Walls {
    pub tool: Sandbox,
    /// The chat folder and the temp folder.
    pub writable: Vec<PathBuf>,
    pub temp: PathBuf,
    /// Each one exists, and none of them holds a writable path.
    pub hidden: Vec<PathBuf>,
    /// Each `.git` in the chat folder. A command cannot move, remove, or replace one, so
    /// git outside the sandbox never finds a new one there. A `.git` file is read-only.
    pub pinned: Vec<PathBuf>,
    /// An empty file that shows in place of a hidden file.
    pub empty: PathBuf,
    /// The way to the proxy of the run. With none, commands have no network.
    pub proxy: Option<ProxyEnd>,
    /// The ports of this computer that `localhost:<port>` reaches through the proxy.
    pub local_ports: Vec<u16>,
    /// Copy-on-write views of the homes of cargo and rustup, only with `bwrap`.
    pub overlays: Vec<OverlayMount>,
}

/// A folder that commands see as writable. The writes land in the temp folder of the
/// run, and the real folder never changes.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct OverlayMount {
    pub folder: PathBuf,
    /// Both in the temp folder.
    pub upper: PathBuf,
    pub work: PathBuf,
}

/// Where a command inside the sandbox finds the proxy of its run.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum ProxyEnd {
    /// `bwrap`: the forwarder, this program inside the sandbox, relays a port there to
    /// the socket.
    Socket { socket: PathBuf, forwarder: PathBuf },
    /// Seatbelt: a loopback port, the only address that the profile allows.
    Port(u16),
}

/// The walls of one run. The holder, the proxy, the temp folder, and the walls file go
/// away with it.
pub struct RunWalls {
    /// The holder that the bridge started itself, when the agent has no wall of its own.
    holder: Option<Holder>,
    pub walls: Walls,
    file: PathBuf,
    /// Before the temp folder, so the proxy stops before its socket goes away.
    _proxy: Option<Proxy>,
    _temp: tempfile::TempDir,
}

impl Drop for RunWalls {
    fn drop(&mut self) {
        drop(self.holder.take());
        let _ = std::fs::remove_file(holder_file(&self.file));
        let _ = std::fs::remove_file(&self.file);
    }
}

/// The folders of the bridge that every run hides.
pub struct Guarded<'a> {
    pub config_dir: &'a Path,
    pub data_dir: &'a Path,
}

impl RunWalls {
    /// The variables of the agent process: the wrapper, the walls, and the temp folder.
    pub fn claude_vars(&self, wrapper: &Path) -> Vec<(String, OsString)> {
        vec![
            (
                "CLAUDE_CODE_SHELL_PREFIX".into(),
                shell_prefix(wrapper).into(),
            ),
            (WALLS_VAR.into(), self.file.clone().into()),
            ("TMPDIR".into(), self.walls.temp.clone().into()),
        ]
    }

    /// Starts the one sandbox of the run from the bridge, for an agent with no wall of
    /// its own. With a wall, the forwarder in the wall starts it (`holder.rs`). Seatbelt
    /// starts a sandbox for each command, so there is nothing to start.
    pub fn hold(&mut self) -> Result<(), String> {
        if matches!(self.walls.tool, Sandbox::Bwrap(_)) {
            self.holder = Some(start_holder(&self.walls, &self.file)?);
        }
        Ok(())
    }

    /// The wrapper ran at least once in this run.
    pub fn wrapper_ran(&self) -> bool {
        self.walls.temp.join(MARKER).exists()
    }
}

/// Claude Code quotes the part before the last " -" as the program, so a flag keeps a
/// path with spaces or a " -" in it whole.
pub fn shell_prefix(wrapper: &Path) -> String {
    format!("{} {RUN_FLAG}", wrapper.display())
}

/// Makes the temp folder, the proxy, and the walls file of one run in `chat`. `tag`
/// names the chat in each log line of the proxy.
pub fn prepare(
    sandbox: &CommandSandbox,
    guarded: &Guarded,
    chat: &Path,
    tag: &str,
) -> Result<RunWalls, String> {
    let chat = resolve(chat).ok_or("The chat folder is missing.")?;
    let (temp, temp_path) = make_temp()?;
    let deny: Vec<PathBuf> = [guarded.config_dir, guarded.data_dir]
        .iter()
        .map(|d| resolve(d).unwrap_or_else(|| d.to_path_buf()))
        .collect();
    let policy = policy_of(&chat, &temp_path, &deny);
    check_writable(&policy, &chat, &temp_path)?;
    let writable = vec![chat.clone(), temp_path.clone()];
    check_wrapper(&sandbox.wrapper, &writable)?;
    let scan = scan_chat(&policy, &chat)?;
    let mut hidden = hidden_paths(&policy, &deny, sandbox.home.as_deref(), scan.hidden);
    hidden.retain(|h| !writable.iter().any(|w| w.starts_with(h)));
    let place = guarded.data_dir.join("sandbox");
    let (proxy, end) = start_proxy(sandbox, &temp_path, &hidden, tag)?.unzip();
    let overlays = tool_overlays(sandbox, &temp_path, &writable, &hidden)?;
    let walls = Walls {
        tool: sandbox.tool.clone(),
        writable,
        temp: temp_path,
        hidden,
        pinned: scan.pinned,
        empty: empty_file(&place)?,
        local_ports: match (&proxy, &sandbox.proxy) {
            (Some(_), Some(settings)) => settings.local_ports.to_vec(),
            _ => Vec::new(),
        },
        proxy: end,
        overlays,
    };
    let file = write_walls(&place, &walls)?;
    Ok(RunWalls {
        holder: None,
        walls,
        file,
        _proxy: proxy,
        _temp: temp,
    })
}

/// cargo and rustup write each download into their home. A folder that is writable or
/// hidden already, or that holds a writable path, keeps what it has. A hidden file in
/// the folder stays hidden, because `bwrap` covers it after the overlay.
fn tool_overlays(
    sandbox: &CommandSandbox,
    temp: &Path,
    writable: &[PathBuf],
    hidden: &[PathBuf],
) -> Result<Vec<OverlayMount>, String> {
    let (Overlay::Works, Some(home)) = (sandbox.overlay, &sandbox.home) else {
        return Ok(Vec::new());
    };
    let mut mounts = Vec::new();
    for name in TOOL_HOMES {
        let Ok(folder) = home.join(name).canonicalize() else {
            continue;
        };
        let meets = |other: &PathBuf| folder.starts_with(other) || other.starts_with(&folder);
        let in_hidden = hidden.iter().any(|h| folder.starts_with(h));
        if !folder.is_dir() || in_hidden || writable.iter().any(meets) {
            continue;
        }
        let upper = temp.join(format!("overlay{name}"));
        let work = temp.join(format!("overlay{name}-work"));
        for dir in [&upper, &work] {
            std::fs::create_dir(dir).map_err(|e| format!("No folder for the overlay: {e}"))?;
        }
        mounts.push(OverlayMount {
            folder,
            upper,
            work,
        });
    }
    Ok(mounts)
}

/// The proxy of the run and the way to it, or `None` with no hosts or no sandbox.
fn start_proxy(
    sandbox: &CommandSandbox,
    temp: &Path,
    hidden: &[PathBuf],
    tag: &str,
) -> Result<Option<(Proxy, ProxyEnd)>, String> {
    let Some(settings) = sandbox.proxy.clone() else {
        return Ok(None);
    };
    let failed = |e: std::io::Error| format!("The proxy of the sandbox did not start: {e}");
    match &sandbox.tool {
        Sandbox::Bwrap(_) => {
            check_forwarder(&sandbox.wrapper, hidden)?;
            let socket = temp.join(PROXY_SOCKET);
            let proxy = listen_unix(&socket, settings, tag).map_err(failed)?;
            let forwarder = sandbox.wrapper.clone();
            Ok(Some((proxy, ProxyEnd::Socket { socket, forwarder })))
        }
        Sandbox::Seatbelt => {
            let (proxy, port) = proxy::listen_tcp(settings, tag.to_owned()).map_err(failed)?;
            Ok(Some((proxy, ProxyEnd::Port(port))))
        }
        Sandbox::None => Ok(None),
    }
}

#[cfg(unix)]
fn listen_unix(socket: &Path, settings: ProxySettings, tag: &str) -> std::io::Result<Proxy> {
    proxy::listen_unix(socket, settings, tag.to_owned())
}

#[cfg(not(unix))]
fn listen_unix(_socket: &Path, _settings: ProxySettings, _tag: &str) -> std::io::Result<Proxy> {
    Err(std::io::ErrorKind::Unsupported.into())
}

/// The forwarder runs inside the sandbox, so a hidden folder must not hold it.
fn check_forwarder(wrapper: &Path, hidden: &[PathBuf]) -> Result<(), String> {
    let real = wrapper
        .canonicalize()
        .unwrap_or_else(|_| wrapper.to_owned());
    if hidden.iter().any(|h| real.starts_with(h)) {
        return Err(format!(
            "The bridge program {} is inside a folder that the sandbox hides. Install it somewhere else, for example ~/.local/bin.",
            real.display()
        ));
    }
    Ok(())
}

/// Mode 0700, with a name that no other run has.
pub fn make_temp() -> Result<(tempfile::TempDir, PathBuf), String> {
    let failed = |e: std::io::Error| format!("No temp folder for the sandbox: {e}");
    let temp = tempfile::Builder::new()
        .prefix("gnomish-relay-run-")
        .tempdir()
        .map_err(failed)?;
    let real = temp.path().canonicalize().map_err(failed)?;
    Ok((temp, real))
}

/// A command could change a wrapper in a writable folder, and the next command would
/// then run outside the sandbox.
fn check_wrapper(wrapper: &Path, writable: &[PathBuf]) -> Result<(), String> {
    let real = wrapper
        .canonicalize()
        .unwrap_or_else(|_| wrapper.to_owned());
    if writable.iter().any(|w| real.starts_with(w)) {
        return Err(format!(
            "The bridge program {} is inside the chat folder, so a command could change it. Install it somewhere else, for example ~/.local/bin.",
            real.display()
        ));
    }
    Ok(())
}
fn patterns(list: &[&str]) -> Vec<Vec<u8>> {
    list.iter().map(|p| p.as_bytes().to_vec()).collect()
}

fn policy_of(chat: &Path, temp: &Path, deny: &[PathBuf]) -> SandboxPolicy {
    let deny: Vec<Vec<u8>> = deny.iter().map(|d| resolved_bytes(d)).collect();
    sandbox_policy(
        &resolved_bytes(chat),
        &resolved_bytes(temp),
        &deny,
        &patterns(DESKTOP_PATHS),
        &patterns(DESKTOP_WRITES),
    )
}

fn check_writable(policy: &SandboxPolicy, chat: &Path, temp: &Path) -> Result<(), String> {
    if !policy.writable.contains(&resolved_bytes(chat)) {
        return Err("The chat folder is inside a folder that the sandbox hides (the config folder, the data folder, or a credential folder), so the agent cannot work there.".into());
    }
    if !policy.writable.contains(&resolved_bytes(temp)) {
        return Err("The temp folder is inside a folder that the sandbox hides.".into());
    }
    Ok(())
}

fn hides(policy: &SandboxPolicy, path: &Path) -> bool {
    is_hidden(policy, &resolved_bytes(path))
}

/// The paths that the policy hides and that exist: the `deny` folders, the `desktop`
/// paths in the home folder, and the ones that the walk of the chat folder found.
fn hidden_paths(
    policy: &SandboxPolicy,
    deny: &[PathBuf],
    home: Option<&Path>,
    in_chat: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = deny.iter().filter_map(|d| d.canonicalize().ok()).collect();
    if let Some(home) = home {
        found.extend(home_matches(policy, home));
    }
    found.extend(in_chat);
    found.sort();
    found.dedup();
    found
}

/// A pattern such as `.config/gh` names one path in the home folder. A last `*` needs
/// a look at the folder of that part.
fn home_matches(policy: &SandboxPolicy, home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for pattern in DESKTOP_PATHS.iter().chain(DESKTOP_WRITES) {
        let path = home.join(pattern);
        if !pattern.contains('*') {
            push_real(&mut found, &path);
            continue;
        }
        let Some(folder) = path.parent() else {
            continue;
        };
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            if hides(policy, &entry.path()) {
                push_real(&mut found, &entry.path());
            }
        }
    }
    found
}

/// The real path, so that a link hides its target.
fn push_real(found: &mut Vec<PathBuf>, path: &Path) {
    if let Ok(real) = path.canonicalize() {
        found.push(real);
    }
}

/// The last part of each pattern, in lower case. The folder of an entry is not hidden,
/// so a new match of the walk ends at the name of the entry.
fn last_parts() -> Vec<String> {
    DESKTOP_PATHS
        .iter()
        .chain(DESKTOP_WRITES)
        .filter_map(|p| p.rsplit('/').next())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn could_match(name: &str, lasts: &[String]) -> bool {
    let name = name.to_ascii_lowercase();
    lasts.iter().any(|last| match last.strip_suffix('*') {
        Some(start) => name.starts_with(start),
        None => name == *last,
    })
}

/// What the walk of the chat folder finds.
#[derive(Debug, Default)]
struct ChatScan {
    hidden: Vec<PathBuf>,
    pinned: Vec<PathBuf>,
}

/// macOS sees `.GIT` as `.git`.
fn is_git_name(name: &std::ffi::OsStr) -> bool {
    name.eq_ignore_ascii_case(".git")
}

/// A `.git` folder, or a folder of a submodule or a worktree inside one.
fn is_git_folder(chat: &Path, folder: &Path) -> bool {
    let inside_git = folder
        .strip_prefix(chat)
        .is_ok_and(|rest| rest.iter().any(is_git_name));
    inside_git && folder.join("HEAD").is_file()
}

/// The hidden paths and the `.git` entries in the chat folder. The walk does not follow
/// links, but a link with a hidden name hides its target. A link named `.git` stops the
/// run: a command could replace the link, and a mount cannot pin it.
fn scan_chat(policy: &SandboxPolicy, chat: &Path) -> Result<ChatScan, String> {
    let lasts = last_parts();
    let mut scan = ChatScan::default();
    let mut folders = vec![chat.to_path_buf()];
    let mut seen = 0;
    while let Some(folder) = folders.pop() {
        if is_git_folder(chat, &folder) {
            for name in GIT_FOLDER_GUARDED {
                push_real(&mut scan.hidden, &folder.join(name));
            }
        }
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            seen += 1;
            if seen > MAX_WALK {
                return Err(format!(
                    "The chat folder holds more than {MAX_WALK} files and folders, more than the sandbox checks."
                ));
            }
            let path = entry.path();
            let name = entry.file_name();
            let kind = entry
                .file_type()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if is_git_name(&name) {
                if kind.is_symlink() {
                    return Err(format!(
                        "{} is a link, and the sandbox cannot guard a .git link.",
                        path.display()
                    ));
                }
                scan.pinned.push(path.clone());
            }
            if could_match(&name.to_string_lossy(), &lasts) && hides(policy, &path) {
                push_real(&mut scan.hidden, &path);
            } else if kind.is_dir() {
                folders.push(path);
            }
        }
    }
    Ok(scan)
}

/// Runs at the same time share the file, so each one only makes it when it is missing.
fn empty_file(place: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(place).map_err(|e| format!("No folder for the sandbox: {e}"))?;
    let file = place.join("empty");
    let made = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file);
    match made {
        Ok(_) => Ok(file),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => check_empty(file),
        Err(e) => Err(format!("No empty file for the sandbox: {e}")),
    }
}

/// A link or a file with bytes would show something in place of a hidden file.
fn check_empty(file: PathBuf) -> Result<PathBuf, String> {
    let meta = std::fs::symlink_metadata(&file).map_err(|e| format!("{e}"))?;
    if !meta.is_file() || meta.len() != 0 {
        return Err(format!("{} is not an empty file.", file.display()));
    }
    Ok(file)
}

fn write_walls(place: &Path, walls: &Walls) -> Result<PathBuf, String> {
    let name = walls
        .temp
        .file_name()
        .map(|n| format!("{}.json", n.to_string_lossy()))
        .ok_or("The temp folder has no name.")?;
    let text = serde_json::to_string(walls)
        .map_err(|e| format!("The sandbox needs paths in UTF-8: {e}"))?;
    crate::fs_safe::write_private(place, &name, &text).map_err(|e| format!("{e:#}"))?;
    Ok(place.join(name))
}

/// They hold the sockets of the ssh agent and the desktop, and the temp files of other
/// programs. They are empty and read-only in the sandbox, unless they hold a writable path.
fn private_folders(walls: &Walls) -> Vec<&'static str> {
    let folders: &[&'static str] = if cfg!(target_os = "macos") {
        &["/private/tmp", "/private/var/tmp"]
    } else {
        &["/tmp", "/var/tmp", "/run"]
    };
    folders
        .iter()
        .copied()
        .filter(|f| !walls.writable.iter().any(|w| Path::new(f).starts_with(w)))
        .collect()
}

fn os(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

/// A read-only system with the writable paths bound in, then each hidden path covered
/// and made read-only. The holder of the run adds its namespaces (`holder.rs`).
pub fn mount_args(walls: &Walls) -> Vec<OsString> {
    let mut args = os(&["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]);
    let private = private_folders(walls);
    for folder in &private {
        args.extend(os(&["--tmpfs", folder]));
    }
    for path in &walls.writable {
        args.extend(["--bind".into(), path.into(), path.into()]);
    }
    // A mount point cannot be moved or removed. Before the hidden paths, which lie inside.
    for path in &walls.pinned {
        let how = if path.is_dir() { "--bind" } else { "--ro-bind" };
        args.extend([how.into(), path.into(), path.into()]);
    }
    // Before the hidden paths, so a hidden file in the folder stays hidden.
    for mount in &walls.overlays {
        args.extend([
            "--overlay-src".into(),
            mount.folder.clone().into(),
            "--overlay".into(),
            mount.upper.clone().into(),
            mount.work.clone().into(),
            mount.folder.clone().into(),
        ]);
    }
    for path in &walls.hidden {
        if path.is_dir() {
            args.extend(["--tmpfs".into(), path.into()]);
        } else {
            args.extend(["--ro-bind".into(), walls.empty.clone().into(), path.into()]);
        }
    }
    // Only after the binds: each one needs a mount point inside a tmpfs.
    for folder in &private {
        args.extend(os(&["--remount-ro", folder]));
    }
    for path in walls.hidden.iter().filter(|p| p.is_dir()) {
        args.extend(["--remount-ro".into(), path.into()]);
    }
    args
}

fn literal(path: &Path) -> Result<Vec<u8>, String> {
    sbpl_string(&raw_bytes(path)).ok_or_else(|| format!("{} has a NUL byte", path.display()))
}

/// The bytes of the path as the OS has them, also when they are not UTF-8.
#[cfg(unix)]
fn raw_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn raw_bytes(path: &Path) -> Vec<u8> {
    crate::config::path_bytes(path)
}

/// One rule with one filter, `subpath` or `literal`, for each path. No path gives no rule.
fn rule(out: &mut Vec<u8>, head: &str, filter: &str, paths: &[&Path]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    out.extend_from_slice(head.as_bytes());
    for path in paths {
        out.extend_from_slice(format!(" ({filter} ").as_bytes());
        out.extend(literal(path)?);
        out.push(b')');
    }
    out.extend_from_slice(b")\n");
    Ok(())
}

const PROFILE_START: &str = "(version 1)
(allow default)
(deny network*)
(deny file-write*)
(allow file-write* (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/tty\") (literal \"/dev/stdout\") (literal \"/dev/stderr\") (literal \"/dev/dtracehelper\") (subpath \"/dev/fd\"))
";

/// `git credential-osxkeychain` gives the GitHub token of the user with no prompt, and
/// the proxy reaches `github.com`. The cost: a tool that checks TLS with the Security
/// framework fails. `curl`, `git`, cargo, npm, and pip do not use it (SPEC.md 6.6.4).
const NO_KEYCHAIN: &str = "(deny mach-lookup (global-name \"com.apple.SecurityServer\") (global-name \"com.apple.secd\"))\n";

/// A later rule wins in Seatbelt, so the hidden paths come last. Each path is an escaped
/// string literal (S32).
pub fn seatbelt_profile(walls: &Walls) -> Result<Vec<u8>, String> {
    let mut out = PROFILE_START.as_bytes().to_vec();
    if let Some(ProxyEnd::Port(port)) = walls.proxy {
        let rule = format!("(allow network-outbound (remote ip \"localhost:{port}\"))\n");
        out.extend_from_slice(rule.as_bytes());
        out.extend_from_slice(NO_KEYCHAIN.as_bytes());
    }
    for port in &walls.local_ports {
        let rule = format!("(allow network-outbound (remote ip \"localhost:{port}\"))\n");
        out.extend_from_slice(rule.as_bytes());
    }
    let writable: Vec<&Path> = walls.writable.iter().map(PathBuf::as_path).collect();
    rule(&mut out, "(allow file-write*", "subpath", &writable)?;
    let fixed = fixed_paths(walls);
    let fixed: Vec<&Path> = fixed.iter().map(PathBuf::as_path).collect();
    rule(&mut out, "(deny file-write*", "literal", &fixed)?;
    let private = private_folders(walls).into_iter().map(Path::new);
    let hidden: Vec<&Path> = walls
        .hidden
        .iter()
        .map(PathBuf::as_path)
        .chain(private)
        .collect();
    rule(&mut out, "(deny file-read* file-write*", "subpath", &hidden)?;
    Ok(out)
}

/// Seatbelt rules name paths, so a move of a folder above a hidden or pinned path would
/// take the path out of its rule. Each such folder in a writable folder stays in place.
fn fixed_paths(walls: &Walls) -> Vec<PathBuf> {
    let mut fixed = walls.pinned.clone();
    for path in walls.hidden.iter().chain(&walls.pinned) {
        let inside = |w: &&PathBuf| path.starts_with(w) && path != *w;
        let Some(root) = walls.writable.iter().find(inside) else {
            continue;
        };
        let above = path.ancestors().skip(1).take_while(|a| a != root);
        fixed.extend(above.map(Path::to_path_buf));
    }
    fixed.sort();
    fixed.dedup();
    fixed
}

/// How the wrapper starts one command inside the walls. Each tool of the sandbox is one
/// arm of `launch`, and the callers see only this step. A tool that starts the command
/// through calls of its OS, such as an `AppContainer` on Windows, adds a variant here.
#[derive(Debug, PartialEq)]
pub enum Launch {
    /// A program that starts the command inside the walls, such as `bwrap`.
    Program {
        program: PathBuf,
        args: Vec<OsString>,
    },
}

/// The launch of `command` with `shell` inside the walls of the run in `walls_file`. With
/// `bwrap`, the command joins the one sandbox of the run.
pub fn launch(
    walls: &Walls,
    walls_file: &Path,
    cwd: &Path,
    shell: &Path,
    command: &str,
) -> Result<Launch, String> {
    match &walls.tool {
        Sandbox::Bwrap(_) => {
            let holder = check_holder(walls_file)?;
            let path = std::env::var_os("PATH").unwrap_or_default();
            let nsenter = crate::program::find_program("nsenter", &path, false)
                .ok_or("nsenter is not on PATH")?;
            Ok(Launch::Program {
                program: nsenter,
                args: join_args(&holder, cwd, shell, command),
            })
        }
        Sandbox::Seatbelt => {
            let profile = seatbelt_profile(walls)?;
            let mut args = vec![OsString::from("-p"), bytes_arg(profile)];
            args.extend(["--".into(), shell.into(), "-c".into(), command.into()]);
            Ok(Launch::Program {
                program: PathBuf::from(SANDBOX_EXEC),
                args,
            })
        }
        Sandbox::None => Err("This computer has no sandbox.".into()),
    }
}

#[cfg(unix)]
fn bytes_arg(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes)
}

/// Only macOS runs the profile.
#[cfg(not(unix))]
fn bytes_arg(bytes: Vec<u8>) -> OsString {
    match String::from_utf8(bytes) {
        Ok(text) => text.into(),
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned().into(),
    }
}

/// The allowlist of SPEC.md 6.2 rule 12, the temp folder of the run, and the proxy. The
/// agent keeps its keys, such as `ANTHROPIC_API_KEY`, and the command never sees them.
pub fn command_env(
    walls: &Walls,
    own: impl Fn(&str) -> Option<OsString>,
) -> Vec<(String, OsString)> {
    let mut env: Vec<(String, OsString)> = BASE_ENV
        .iter()
        .filter(|name| **name != "TMPDIR")
        .filter_map(|name| own(name).map(|value| ((*name).to_owned(), value)))
        .collect();
    env.push(("TMPDIR".into(), walls.temp.clone().into()));
    // The caches in the home folder are read-only, and npm fails with no cache.
    env.push(("npm_config_cache".into(), walls.temp.join("npm").into()));
    env.push(("PIP_CACHE_DIR".into(), walls.temp.join("pip").into()));
    if let Some(end) = &walls.proxy {
        env.extend(proxy_env(end));
    }
    env
}

/// Each tool reads its own names, and some of them read only the lower-case ones.
pub(crate) const PROXY_VARS: [&str; 8] = [
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "CARGO_HTTP_PROXY",
    "npm_config_https_proxy",
    "npm_config_proxy",
];

/// Only the loopback skips the proxy: the loopback of the sandbox on Linux, where the
/// forwarder relays each port of `local_ports`, and the ports that Seatbelt allows on
/// macOS. A command has no other way out.
pub const NO_PROXY: &str = "localhost,127.0.0.1,::1";

fn proxy_env(end: &ProxyEnd) -> Vec<(String, OsString)> {
    let port = match end {
        ProxyEnd::Socket { .. } => INNER_PORT,
        ProxyEnd::Port(port) => *port,
    };
    let url = OsString::from(format!("http://127.0.0.1:{port}"));
    let mut env: Vec<(String, OsString)> = PROXY_VARS
        .iter()
        .map(|name| ((*name).to_owned(), url.clone()))
        .collect();
    env.push(("NO_PROXY".into(), NO_PROXY.into()));
    env.push(("no_proxy".into(), NO_PROXY.into()));
    env
}

/// `gnomish-relay --sandbox-run <command>` runs one command of Claude Code inside the
/// walls of its run. It never runs the command outside them: a failure is an error.
pub fn run_wrapped(command: &str) -> i32 {
    match wrapped(command) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("gnomish-relay sandbox: {e}");
            126
        }
    }
}

fn wrapped(command: &str) -> Result<i32, String> {
    let file = std::env::var_os(WALLS_VAR).ok_or("no sandbox for this command")?;
    let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read the walls: {e}"))?;
    let walls: Walls = serde_json::from_str(&text).map_err(|e| format!("bad walls: {e}"))?;
    let cwd = std::env::current_dir().map_err(|e| format!("no working folder: {e}"))?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let shell = crate::program::find_program("bash", &path, false).ok_or("bash is not on PATH")?;
    let launch = launch(&walls, Path::new(&file), &cwd, &shell, command)?;
    std::fs::write(walls.temp.join(MARKER), b"").map_err(|e| format!("no marker: {e}"))?;
    let env = command_env(&walls, |name| std::env::var_os(name));
    match launch {
        Launch::Program { program, args } => {
            let mut child = std::process::Command::new(program);
            child.args(args).current_dir(&cwd).env_clear().envs(env);
            run(child)
        }
    }
}

/// The command takes the place of this process, so Claude Code sees its exit status
/// and its signals.
#[cfg(unix)]
fn run(mut child: std::process::Command) -> Result<i32, String> {
    use std::os::unix::process::CommandExt;
    Err(format!("cannot start the sandbox: {}", child.exec()))
}

#[cfg(not(unix))]
fn run(_child: std::process::Command) -> Result<i32, String> {
    Err("This computer has no sandbox.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holder::holder_args;

    /// A home with the folders of the bridge, a credential, and a chat folder with a
    /// `.env` file and git hooks.
    struct Folders {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        config: PathBuf,
        data: PathBuf,
        chat: PathBuf,
    }

    fn folders() -> Folders {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        let config = home.join(".config/gnomish-relay");
        let data = home.join(".local/share/gnomish-relay");
        let chat = home.join("Code/app");
        for dir in [&config, &data, &chat.join(".git/hooks"), &home.join(".ssh")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(chat.join(".env"), "TOKEN=1").unwrap();
        std::fs::write(chat.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(home.join(".env.local"), "TOKEN=2").unwrap();
        std::fs::write(home.join(".claude.json"), "{}").unwrap();
        Folders {
            _tmp: tmp,
            home,
            config,
            data,
            chat,
        }
    }

    fn sandbox(h: &Folders, tool: Sandbox) -> CommandSandbox {
        CommandSandbox::new(
            tool,
            PathBuf::from("/usr/bin/gnomish-relay"),
            Some(h.home.clone()),
        )
    }

    fn run_walls(h: &Folders, chat: &Path) -> Result<RunWalls, String> {
        let guarded = Guarded {
            config_dir: &h.config,
            data_dir: &h.data,
        };
        prepare(&sandbox(h, Sandbox::Seatbelt), &guarded, chat, "chat test")
    }

    fn proxied(h: &Folders, tool: Sandbox) -> Result<RunWalls, String> {
        let guarded = Guarded {
            config_dir: &h.config,
            data_dir: &h.data,
        };
        let hosts = HostList::new(crate::allow_hosts::Defaults::Keep, &[]).unwrap();
        let sandbox = sandbox(h, tool).with_proxy(ProxySettings::new(hosts));
        prepare(&sandbox, &guarded, &h.chat, "chat test")
    }

    #[test]
    fn the_walls_write_the_chat_folder_and_a_new_temp_folder() {
        let h = folders();

        let run = run_walls(&h, &h.chat).unwrap();

        assert_eq!(run.walls.writable, [h.chat.clone(), run.walls.temp.clone()]);
        assert!(run.walls.temp.is_dir());
        assert!(
            run.walls
                .temp
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("gnomish-relay-run-")
        );
        assert_eq!(std::fs::read(&run.walls.empty).unwrap(), b"");
    }

    #[test]
    fn the_walls_hide_the_bridge_folders_and_the_credentials_that_exist() {
        let h = folders();

        let run = run_walls(&h, &h.chat).unwrap();

        for path in [
            h.config.clone(),
            h.data.clone(),
            h.home.join(".ssh"),
            h.home.join(".env.local"),
            h.home.join(".claude.json"),
            h.chat.join(".env"),
            h.chat.join(".git/hooks"),
        ] {
            assert!(run.walls.hidden.contains(&path), "{}", path.display());
        }
        assert!(!run.walls.hidden.contains(&h.chat.join("main.rs")));
        assert!(!run.walls.hidden.iter().any(|p| p.ends_with(".aws")));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_with_a_hidden_name_hides_its_target() {
        let h = folders();
        let secret = h.home.join("secrets.txt");
        std::fs::write(&secret, "prod key").unwrap();
        std::os::unix::fs::symlink(&secret, h.chat.join(".netrc")).unwrap();

        let run = run_walls(&h, &h.chat).unwrap();

        assert!(run.walls.hidden.contains(&secret));
    }

    #[test]
    fn an_empty_file_with_bytes_in_it_gets_no_run() {
        let h = folders();
        std::fs::create_dir_all(h.data.join("sandbox")).unwrap();
        std::fs::write(h.data.join("sandbox/empty"), "not empty").unwrap();

        let error = run_walls(&h, &h.chat).err().unwrap();

        assert!(error.contains("is not an empty file"), "{error}");
    }

    #[test]
    fn many_runs_at_once_share_the_empty_file() {
        let h = std::sync::Arc::new(folders());
        let runs: Vec<_> = (0..16)
            .map(|_| {
                let h = std::sync::Arc::clone(&h);
                std::thread::spawn(move || {
                    run_walls(&h, &h.chat).map(|run| run.walls.empty.clone())
                })
            })
            .collect();

        for run in runs {
            let empty = run.join().unwrap().unwrap();
            assert_eq!(std::fs::read(empty).unwrap(), b"");
        }
    }

    #[test]
    fn the_walls_file_goes_away_with_the_run() {
        let h = folders();
        let run = run_walls(&h, &h.chat).unwrap();
        let file = run.file.clone();
        let temp = run.walls.temp.clone();
        let text = std::fs::read_to_string(&file).unwrap();
        let walls: Walls = serde_json::from_str(&text).unwrap();
        assert_eq!(walls, run.walls);

        drop(run);

        assert!(!file.exists());
        assert!(!temp.exists());
    }

    #[test]
    fn a_chat_folder_inside_a_hidden_folder_gets_no_run() {
        let h = folders();

        let error = run_walls(&h, &h.home.join(".ssh")).err().unwrap();

        assert!(
            error.contains("inside a folder that the sandbox hides"),
            "{error}"
        );
    }

    #[test]
    fn a_wrapper_inside_the_chat_folder_gets_no_run() {
        let h = folders();
        let guarded = Guarded {
            config_dir: &h.config,
            data_dir: &h.data,
        };
        let mut inside = sandbox(&h, Sandbox::Seatbelt);
        inside.wrapper = h.chat.join("target/debug/gnomish-relay");

        let error = prepare(&inside, &guarded, &h.chat, "chat test")
            .err()
            .unwrap();

        assert!(error.contains("inside the chat folder"), "{error}");
    }

    #[test]
    fn the_walls_pin_each_git_entry_and_hide_the_guarded_files_of_a_submodule() {
        let h = folders();
        let module = h.chat.join(".git/modules/lib");
        std::fs::create_dir_all(module.join("hooks")).unwrap();
        std::fs::write(module.join("HEAD"), "ref: refs/heads/main").unwrap();
        std::fs::write(module.join("config"), "").unwrap();
        std::fs::create_dir_all(h.chat.join("lib")).unwrap();
        std::fs::write(h.chat.join("lib/.git"), "gitdir: ../.git/modules/lib").unwrap();

        let run = run_walls(&h, &h.chat).unwrap();

        let mut pinned = run.walls.pinned.clone();
        pinned.sort();
        assert_eq!(pinned, [h.chat.join(".git"), h.chat.join("lib/.git")]);
        assert!(run.walls.hidden.contains(&module.join("config")));
        assert!(run.walls.hidden.contains(&module.join("hooks")));
        assert!(!run.walls.hidden.contains(&module.join("commondir")));
    }

    #[test]
    fn a_folder_with_no_head_inside_the_git_folder_keeps_its_config() {
        let h = folders();
        let other = h.chat.join(".git/other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("config"), "").unwrap();

        let run = run_walls(&h, &h.chat).unwrap();

        assert!(!run.walls.hidden.contains(&other.join("config")));
    }

    #[cfg(unix)]
    #[test]
    fn a_git_link_in_the_chat_folder_stops_the_run() {
        let h = folders();
        std::fs::create_dir_all(h.chat.join("lib")).unwrap();
        std::os::unix::fs::symlink(h.chat.join(".git"), h.chat.join("lib/.git")).unwrap();

        let error = run_walls(&h, &h.chat).err().unwrap();

        assert!(error.contains("cannot guard a .git link"), "{error}");
    }

    fn sample() -> Walls {
        Walls {
            tool: Sandbox::Bwrap(PathBuf::from("/usr/bin/bwrap")),
            writable: vec![
                PathBuf::from("/home/x/Code/app"),
                PathBuf::from("/tmp/run1"),
            ],
            temp: PathBuf::from("/tmp/run1"),
            hidden: vec![
                PathBuf::from("/home/x/.ssh"),
                PathBuf::from("/home/x/Code/app/.env"),
            ],
            pinned: Vec::new(),
            empty: PathBuf::from("/data/sandbox/empty"),
            proxy: None,
            local_ports: Vec::new(),
            overlays: Vec::new(),
        }
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn position(args: &[String], run: &[&str]) -> usize {
        args.windows(run.len())
            .position(|w| w == run)
            .unwrap_or_else(|| panic!("no {run:?} in {args:?}"))
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bwrap_binds_the_writable_paths_before_it_hides_and_ends_with_the_holder() {
        let args = strings(&holder_args(&sample()));

        assert_eq!(args[..3], ["--ro-bind", "/", "/"]);
        let tmp = position(&args, &["--tmpfs", "/tmp"]);
        let bind = position(&args, &["--bind", "/tmp/run1", "/tmp/run1"]);
        let hide = position(&args, &["--ro-bind", "/data/sandbox/empty", "/home/x/.ssh"]);
        let read_only = position(&args, &["--remount-ro", "/tmp"]);
        assert!(tmp < bind && bind < hide && hide < read_only);
        position(&args, &["--tmpfs", "/run"]);
        position(&args, &["--info-fd", "2"]);
        for flag in ["--unshare-all", "--die-with-parent", "--new-session"] {
            assert!(args.contains(&flag.to_owned()), "{flag}");
        }
        assert_eq!(
            args[args.len() - 4..],
            ["--", "sh", "-c", "echo ready && exec sleep infinity"]
        );
    }

    #[test]
    fn bwrap_pins_each_git_entry_after_the_writable_paths_and_before_the_hidden_paths() {
        let mut walls = sample();
        walls.pinned = vec![
            PathBuf::from("/home/x/Code/app/.git"),
            PathBuf::from("/home/x/Code/app/lib/.git"),
        ];

        let args = strings(&holder_args(&walls));

        let bind = position(&args, &["--bind", "/tmp/run1", "/tmp/run1"]);
        let pin = position(
            &args,
            &[
                "--ro-bind",
                "/home/x/Code/app/.git",
                "/home/x/Code/app/.git",
            ],
        );
        position(
            &args,
            &[
                "--ro-bind",
                "/home/x/Code/app/lib/.git",
                "/home/x/Code/app/lib/.git",
            ],
        );
        let hide = position(&args, &["--ro-bind", "/data/sandbox/empty", "/home/x/.ssh"]);
        assert!(bind < pin && pin < hide);
    }

    #[test]
    fn seatbelt_keeps_each_pinned_path_and_each_folder_above_a_guarded_path_in_place() {
        let mut walls = sample();
        walls
            .hidden
            .push(PathBuf::from("/home/x/Code/app/web/api/.env"));
        walls.pinned = vec![PathBuf::from("/home/x/Code/app/lib/.git")];

        let profile = String::from_utf8(seatbelt_profile(&walls).unwrap()).unwrap();

        let allow = profile.find("(allow file-write* (subpath").unwrap();
        let fixed = profile.find("(deny file-write* (literal").unwrap();
        assert!(allow < fixed);
        for path in [
            "/home/x/Code/app/lib/.git",
            "/home/x/Code/app/lib",
            "/home/x/Code/app/web",
            "/home/x/Code/app/web/api",
        ] {
            assert!(
                profile.contains(&format!("(literal \"{path}\")")),
                "{path}: {profile}"
            );
        }
        assert!(!profile.contains("(literal \"/home/x/Code/app\")"));
        assert!(!profile.contains("(literal \"/home/x/Code/app/.env\")"));
    }

    #[test]
    fn seatbelt_keeps_no_folder_above_a_pinned_path_that_is_a_writable_path() {
        let mut walls = sample();
        walls.pinned = vec![PathBuf::from("/tmp/run1")];

        let profile = String::from_utf8(seatbelt_profile(&walls).unwrap()).unwrap();

        assert!(!profile.contains("(literal \"/tmp\")"), "{profile}");
        assert!(!profile.contains("(literal \"\")"), "{profile}");
    }

    #[test]
    fn with_a_proxy_the_holder_is_the_forwarder_with_the_local_ports() {
        let mut walls = sample();
        walls.proxy = Some(ProxyEnd::Socket {
            socket: PathBuf::from("/tmp/run1/.gnomish-relay-proxy"),
            forwarder: PathBuf::from("/usr/bin/gnomish-relay"),
        });

        walls.local_ports = vec![5432, 3000];

        let args = strings(&holder_args(&walls));

        assert_eq!(
            args[args.len() - 6..],
            [
                "--",
                "/usr/bin/gnomish-relay",
                crate::forward::FORWARD_FLAG,
                "/tmp/run1/.gnomish-relay-proxy",
                "5432,3000",
                crate::holder::HOLD_FLAG,
            ][..]
        );
        assert!(args.contains(&"--unshare-all".to_owned()));
    }

    #[test]
    fn seatbelt_allows_each_local_port_and_no_other_port_of_this_computer() {
        let mut walls = sample();
        walls.proxy = Some(ProxyEnd::Port(41234));
        walls.local_ports = vec![5432];

        let profile = String::from_utf8(seatbelt_profile(&walls).unwrap()).unwrap();

        let deny = profile.find("(deny network*)").unwrap();
        let local = profile
            .find("(allow network-outbound (remote ip \"localhost:5432\"))")
            .unwrap();
        assert!(deny < local);
        assert_eq!(profile.matches("(allow network").count(), 2);
    }

    #[test]
    fn with_a_proxy_seatbelt_allows_only_its_port_after_the_deny_of_the_network() {
        let mut walls = sample();
        walls.proxy = Some(ProxyEnd::Port(41234));

        let profile = String::from_utf8(seatbelt_profile(&walls).unwrap()).unwrap();

        let deny = profile.find("(deny network*)").unwrap();
        let allow = profile
            .find("(allow network-outbound (remote ip \"localhost:41234\"))")
            .unwrap();
        assert!(deny < allow);
        assert_eq!(profile.matches("(allow network").count(), 1);
        let plain = String::from_utf8(seatbelt_profile(&sample()).unwrap()).unwrap();
        assert!(!plain.contains("(allow network"));
    }

    #[test]
    fn with_a_proxy_seatbelt_denies_the_services_of_the_keychain() {
        let mut walls = sample();
        walls.proxy = Some(ProxyEnd::Port(41234));

        let profile = String::from_utf8(seatbelt_profile(&walls).unwrap()).unwrap();

        assert!(profile.contains(
            "(deny mach-lookup (global-name \"com.apple.SecurityServer\") (global-name \"com.apple.secd\"))"
        ));
        let plain = String::from_utf8(seatbelt_profile(&sample()).unwrap()).unwrap();
        assert!(!plain.contains("mach-lookup"));
    }

    #[test]
    fn with_a_proxy_a_command_gets_the_proxy_in_every_name_and_only_the_loopback_skips_it() {
        let mut walls = sample();
        walls.proxy = Some(ProxyEnd::Socket {
            socket: PathBuf::from("/tmp/run1/.gnomish-relay-proxy"),
            forwarder: PathBuf::from("/usr/bin/gnomish-relay"),
        });

        let env = command_env(&walls, |_| None);

        let value = |name: &str| env.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
        for name in PROXY_VARS {
            assert_eq!(
                value(name),
                Some(OsString::from(format!("http://127.0.0.1:{INNER_PORT}"))),
                "{name}"
            );
        }
        let loopback = Some(OsString::from("localhost,127.0.0.1,::1"));
        assert_eq!(value("NO_PROXY"), loopback);
        assert_eq!(value("no_proxy"), loopback);
        walls.proxy = Some(ProxyEnd::Port(41234));
        let env = command_env(&walls, |_| None);
        assert!(env.contains(&(
            "HTTPS_PROXY".to_owned(),
            OsString::from("http://127.0.0.1:41234")
        )));
    }

    #[test]
    fn with_no_proxy_a_command_gets_no_proxy_but_still_gets_its_caches() {
        let walls = sample();

        let env = command_env(&walls, |_| None);

        assert!(!env.iter().any(|(name, _)| name.ends_with("_PROXY")));
        let npm = walls.temp.join("npm").into_os_string();
        assert!(env.contains(&("npm_config_cache".to_owned(), npm)));
        let pip = walls.temp.join("pip").into_os_string();
        assert!(env.contains(&("PIP_CACHE_DIR".to_owned(), pip)));
    }

    #[cfg(unix)]
    #[test]
    fn a_run_with_bwrap_and_hosts_gets_a_proxy_on_a_socket_in_its_temp_folder() {
        let h = folders();

        let run = proxied(&h, Sandbox::Bwrap(PathBuf::from("/usr/bin/bwrap"))).unwrap();

        let socket = run.walls.temp.join(PROXY_SOCKET);
        assert_eq!(
            run.walls.proxy,
            Some(ProxyEnd::Socket {
                socket: socket.clone(),
                forwarder: PathBuf::from("/usr/bin/gnomish-relay"),
            })
        );
        assert!(std::os::unix::net::UnixStream::connect(&socket).is_ok());
    }

    #[test]
    fn a_run_with_seatbelt_and_hosts_gets_a_proxy_on_a_loopback_port() {
        let h = folders();

        let run = proxied(&h, Sandbox::Seatbelt).unwrap();

        let Some(ProxyEnd::Port(port)) = run.walls.proxy else {
            panic!("no port: {:?}", run.walls.proxy);
        };
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
        drop(run);
        assert!(port_closes(port), "the proxy still listens on {port}");
    }

    /// A test that runs at the same time can take the free port for a moment, so the
    /// check waits for the port to be closed once.
    fn port_closes(port: u16) -> bool {
        (0..50).any(|_| {
            let closed = std::net::TcpStream::connect(("127.0.0.1", port)).is_err();
            if !closed {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            closed
        })
    }

    #[test]
    fn a_run_with_no_hosts_gets_no_proxy() {
        let h = folders();

        let run = run_walls(&h, &h.chat).unwrap();

        assert_eq!(run.walls.proxy, None);
        assert!(
            CommandSandbox::detect(HostList::default(), &[])
                .proxy
                .is_none()
        );
    }

    #[test]
    fn a_forwarder_inside_a_hidden_folder_gets_no_run() {
        let h = folders();
        let guarded = Guarded {
            config_dir: &h.config,
            data_dir: &h.data,
        };
        let hosts = HostList::new(crate::allow_hosts::Defaults::Keep, &[]).unwrap();
        let mut inside = sandbox(&h, Sandbox::Bwrap(PathBuf::from("/usr/bin/bwrap")))
            .with_proxy(ProxySettings::new(hosts));
        inside.wrapper = h.data.join("gnomish-relay");

        let error = prepare(&inside, &guarded, &h.chat, "chat test")
            .err()
            .unwrap();

        assert!(
            error.contains("inside a folder that the sandbox hides"),
            "{error}"
        );
    }

    #[test]
    fn bwrap_lays_each_overlay_after_the_writable_paths_and_before_the_hidden_paths() {
        let mut walls = sample();
        walls.overlays = vec![OverlayMount {
            folder: PathBuf::from("/home/x/.cargo"),
            upper: PathBuf::from("/tmp/run1/overlay.cargo"),
            work: PathBuf::from("/tmp/run1/overlay.cargo-work"),
        }];
        walls
            .hidden
            .push(PathBuf::from("/home/x/.cargo/credentials.toml"));

        let args = strings(&holder_args(&walls));

        let bind = position(&args, &["--bind", "/tmp/run1", "/tmp/run1"]);
        let overlay = position(
            &args,
            &[
                "--overlay-src",
                "/home/x/.cargo",
                "--overlay",
                "/tmp/run1/overlay.cargo",
                "/tmp/run1/overlay.cargo-work",
                "/home/x/.cargo",
            ],
        );
        let hide = position(
            &args,
            &[
                "--ro-bind",
                "/data/sandbox/empty",
                "/home/x/.cargo/credentials.toml",
            ],
        );
        assert!(bind < overlay && overlay < hide);
    }

    #[test]
    fn a_run_with_a_working_overlay_views_the_homes_of_cargo_and_rustup_through_the_temp_folder() {
        let h = folders();
        std::fs::create_dir_all(h.home.join(".cargo/registry")).unwrap();
        std::fs::create_dir_all(h.home.join(".rustup")).unwrap();
        let guarded = Guarded {
            config_dir: &h.config,
            data_dir: &h.data,
        };
        let mut sandbox = sandbox(&h, Sandbox::Bwrap(PathBuf::from("/usr/bin/bwrap")));
        sandbox.overlay = Overlay::Works;

        let run = prepare(&sandbox, &guarded, &h.chat, "chat test").unwrap();

        let folders: Vec<&PathBuf> = run.walls.overlays.iter().map(|m| &m.folder).collect();
        assert_eq!(folders, [&h.home.join(".cargo"), &h.home.join(".rustup")]);
        for mount in &run.walls.overlays {
            assert!(mount.upper.starts_with(&run.walls.temp) && mount.upper.is_dir());
            assert!(mount.work.starts_with(&run.walls.temp) && mount.work.is_dir());
        }
    }

    #[test]
    fn with_no_overlay_or_a_home_in_the_chat_folder_a_run_gets_no_overlay() {
        let h = folders();
        std::fs::create_dir_all(h.home.join(".cargo")).unwrap();
        std::fs::create_dir_all(h.chat.join(".cargo")).unwrap();
        let guarded = Guarded {
            config_dir: &h.config,
            data_dir: &h.data,
        };
        let mut in_chat = sandbox(&h, Sandbox::Bwrap(PathBuf::from("/usr/bin/bwrap")));
        in_chat.overlay = Overlay::Works;
        in_chat.home = Some(h.chat.clone());

        let missing = run_walls(&h, &h.chat).unwrap();
        let in_chat = prepare(&in_chat, &guarded, &h.chat, "chat test").unwrap();

        assert!(missing.walls.overlays.is_empty());
        assert!(in_chat.walls.overlays.is_empty());
    }

    #[test]
    fn the_summary_names_the_tool_the_network_and_the_downloads() {
        let bwrap = Sandbox::Bwrap(PathBuf::from("/usr/bin/bwrap"));
        let mut sandbox = CommandSandbox::new(bwrap, PathBuf::new(), None);
        assert_eq!(
            sandbox.summary(),
            "bwrap with no network, and a new download of cargo or rustup fails"
        );
        sandbox.overlay = Overlay::Works;
        let hosts = HostList::new(crate::allow_hosts::Defaults::Keep, &[]).unwrap();
        let sandbox = sandbox.with_proxy(ProxySettings::new(hosts));
        assert_eq!(
            sandbox.summary(),
            "bwrap with the allowed hosts through the proxy, and downloads of cargo and rustup go to the temp folder of the run"
        );
        assert_eq!(
            CommandSandbox::none().summary(),
            "no sandbox with no network"
        );
    }

    #[test]
    fn a_tool_that_is_not_bwrap_has_no_overlay() {
        assert_eq!(detect_overlay(&Sandbox::Seatbelt), Overlay::Missing);
        assert_eq!(detect_overlay(&Sandbox::None), Overlay::Missing);
        let missing = Sandbox::Bwrap(PathBuf::from("/no/such/bwrap"));
        assert_eq!(detect_overlay(&missing), Overlay::Missing);
    }

    #[test]
    fn a_hidden_folder_gets_an_empty_folder_and_a_hidden_file_an_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(".ssh");
        std::fs::create_dir(&folder).unwrap();
        let mut walls = sample();
        walls.hidden = vec![folder.clone(), PathBuf::from("/home/x/Code/app/.env")];

        let args = strings(&holder_args(&walls));

        let folder = folder.display().to_string();
        position(&args, &["--tmpfs", &folder]);
        position(&args, &["--remount-ro", &folder]);
        position(
            &args,
            &["--ro-bind", "/data/sandbox/empty", "/home/x/Code/app/.env"],
        );
    }

    #[test]
    fn a_writable_path_keeps_its_private_folder_open() {
        let tmp = if cfg!(target_os = "macos") {
            "/private/tmp"
        } else {
            "/tmp"
        };
        let mut walls = sample();
        walls.writable.push(PathBuf::from(tmp));

        assert!(!private_folders(&walls).contains(&tmp));
        assert!(private_folders(&sample()).contains(&tmp));
    }

    #[test]
    fn seatbelt_allows_the_writes_before_it_hides_and_escapes_each_path() {
        let mut walls = sample();
        walls
            .hidden
            .push(PathBuf::from("/home/x/a\") (allow default) (\""));

        let profile = String::from_utf8(seatbelt_profile(&walls).unwrap()).unwrap();

        assert!(
            profile
                .starts_with("(version 1)\n(allow default)\n(deny network*)\n(deny file-write*)\n")
        );
        let writes = profile
            .find("(allow file-write* (subpath \"/home/x/Code/app\") (subpath \"/tmp/run1\"))")
            .unwrap();
        let hides = profile
            .find("(deny file-read* file-write* (subpath \"/home/x/.ssh\")")
            .unwrap();
        assert!(writes < hides);
        assert!(profile.contains("(subpath \"/home/x/a\\\") (allow default) (\\\"\")"));
        let rules = profile.lines().filter(|l| l.starts_with("(allow default)"));
        assert_eq!(rules.count(), 1, "the quote in the name adds no rule");
    }

    #[test]
    fn a_path_with_a_nul_byte_gets_no_profile() {
        let mut walls = sample();
        walls.hidden.push(PathBuf::from("/home/x/a\0b"));

        assert!(seatbelt_profile(&walls).is_err());
    }

    #[test]
    fn seatbelt_runs_the_shell_with_the_profile() {
        let mut walls = sample();
        walls.tool = Sandbox::Seatbelt;

        let Launch::Program { program, args } = launch(
            &walls,
            Path::new("/nowhere"),
            Path::new("/"),
            Path::new("/bin/bash"),
            "make",
        )
        .unwrap();

        let args = strings(&args);
        assert_eq!(program, PathBuf::from(SANDBOX_EXEC));
        assert_eq!(args[0], "-p");
        assert!(args[1].starts_with("(version 1)"));
        assert_eq!(args[2..], ["--", "/bin/bash", "-c", "make"]);
    }

    #[test]
    fn with_no_sandbox_there_is_no_launch() {
        let mut walls = sample();
        walls.tool = Sandbox::None;

        let file = Path::new("/nowhere/run.json");
        assert!(launch(&walls, file, Path::new("/"), Path::new("/bin/bash"), "make").is_err());
    }

    #[test]
    fn with_bwrap_and_no_holder_a_command_does_not_start() {
        let dir = tempfile::tempdir().unwrap();

        let launched = launch(
            &sample(),
            &dir.path().join("run.json"),
            Path::new("/"),
            Path::new("/bin/bash"),
            "make",
        );

        let error = launched.unwrap_err();
        assert!(error.contains("not running"), "{error}");
    }

    #[test]
    fn a_command_gets_the_allowlist_and_the_temp_folder_but_no_key() {
        let own = |name: &str| match name {
            "PATH" => Some(OsString::from("/usr/bin")),
            "TMPDIR" => Some(OsString::from("/tmp")),
            "ANTHROPIC_API_KEY" => Some(OsString::from("sk-secret")),
            _ => None,
        };

        let env = command_env(&sample(), own);

        assert_eq!(
            env[..2],
            [
                ("PATH".to_owned(), OsString::from("/usr/bin")),
                ("TMPDIR".to_owned(), OsString::from("/tmp/run1")),
            ][..]
        );
        assert!(!env.iter().any(|(name, _)| name == "ANTHROPIC_API_KEY"));
    }

    #[test]
    fn claude_runs_this_program_before_each_command_with_the_walls_of_the_run() {
        let h = folders();
        let run = run_walls(&h, &h.chat).unwrap();

        let vars = run.claude_vars(Path::new("/opt/my -dir/gnomish-relay"));

        assert_eq!(vars[0].0, "CLAUDE_CODE_SHELL_PREFIX");
        assert_eq!(vars[0].1, "/opt/my -dir/gnomish-relay --sandbox-run");
        assert_eq!(vars[1], (WALLS_VAR.to_owned(), run.file.clone().into()));
        assert_eq!(
            vars[2],
            ("TMPDIR".to_owned(), run.walls.temp.clone().into())
        );
        assert!(!run.wrapper_ran());
        std::fs::write(run.walls.temp.join(MARKER), "").unwrap();
        assert!(run.wrapper_ran());
    }

    #[test]
    fn the_notice_of_no_sandbox_shows_once() {
        let none = CommandSandbox::none();
        let copy = none.clone();

        assert_eq!(none.notice(), Some(NO_SANDBOX));
        assert_eq!(copy.notice(), None);
        assert!(!none.is_on());
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_there_is_no_command_sandbox_so_every_command_asks() {
        use crate::gate::{Effect, Sandboxing, Step, without_sandbox};

        let sandbox = CommandSandbox::detect(HostList::default(), &[]);

        let sandboxing = if sandbox.is_on() {
            Sandboxing::On
        } else {
            Sandboxing::Off
        };
        assert_eq!(sandboxing, Sandboxing::Off);
        let step = without_sandbox(Step::Run, Effect::Command, sandboxing);
        assert_eq!(step, Step::AskGame);
        assert_eq!(sandbox.notice(), Some(NO_SANDBOX));
    }

    #[test]
    fn the_wrapper_with_no_walls_refuses_the_command() {
        // The test process has no walls variable.
        assert_eq!(run_wrapped("true"), 126);
    }

    #[test]
    fn a_name_can_match_a_pattern_only_by_its_last_part() {
        let lasts = last_parts();

        assert!(could_match(".ENV", &lasts));
        assert!(could_match(".env.local", &lasts));
        assert!(could_match("hooks", &lasts));
        assert!(!could_match("main.rs", &lasts));
    }
}
