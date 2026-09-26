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
use crate::process::BASE_ENV;
use crate::story_sandbox::{self, Sandbox};

/// The flag of this program that runs one command inside the walls of a run.
pub const RUN_FLAG: &str = "--sandbox-run";
/// Names the walls file of the run, for the wrapper.
pub const WALLS_VAR: &str = "GNOMISH_RELAY_SANDBOX";
/// The wrapper writes it into the temp folder, so the bridge sees that the wrapper ran.
const MARKER: &str = ".gnomish-relay-sandbox";
/// A folder with more is too large to check at the start of each run.
const MAX_WALK: usize = 1_000_000;
pub const NO_SANDBOX: &str = "(No sandbox on this computer: every command asks in the game.)";
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The sandbox of this computer, for the commands of runs from the game.
#[derive(Clone, Debug)]
pub struct CommandSandbox {
    pub tool: Sandbox,
    /// This program. Claude Code runs it before each command.
    pub wrapper: PathBuf,
    pub home: Option<PathBuf>,
    /// The notice of no sandbox shows once for each start of the bridge.
    told: Arc<AtomicBool>,
}

impl CommandSandbox {
    pub fn new(tool: Sandbox, wrapper: PathBuf, home: Option<PathBuf>) -> CommandSandbox {
        CommandSandbox {
            tool,
            wrapper,
            home,
            told: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Windows has none: its sandbox needs `unsafe` calls of the Windows API.
    pub fn detect() -> CommandSandbox {
        let wrapper = std::env::current_exe().unwrap_or_default();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        CommandSandbox::new(story_sandbox::detect(), wrapper, home)
    }

    pub fn none() -> CommandSandbox {
        CommandSandbox::new(Sandbox::None, PathBuf::new(), None)
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

/// What the wrapper needs for each command of one run.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Walls {
    pub tool: Sandbox,
    /// The chat folder and the temp folder.
    pub writable: Vec<PathBuf>,
    pub temp: PathBuf,
    /// Each one exists, and none of them holds a writable path.
    pub hidden: Vec<PathBuf>,
    /// An empty file that shows in place of a hidden file.
    pub empty: PathBuf,
}

/// The walls of one run. The temp folder and the walls file go away with it.
pub struct RunWalls {
    pub walls: Walls,
    file: PathBuf,
    _temp: tempfile::TempDir,
}

impl Drop for RunWalls {
    fn drop(&mut self) {
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

/// Makes the temp folder and the walls file of one run in `chat`.
pub fn prepare(
    sandbox: &CommandSandbox,
    guarded: &Guarded,
    chat: &Path,
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
    let mut hidden = hidden_paths(&policy, &deny, sandbox.home.as_deref(), &chat)?;
    hidden.retain(|h| !writable.iter().any(|w| w.starts_with(h)));
    let place = guarded.data_dir.join("sandbox");
    let walls = Walls {
        tool: sandbox.tool.clone(),
        writable,
        temp: temp_path,
        hidden,
        empty: empty_file(&place)?,
    };
    let file = write_walls(&place, &walls)?;
    Ok(RunWalls {
        walls,
        file,
        _temp: temp,
    })
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
/// paths in the home folder, and the ones in the chat folder.
pub fn hidden_paths(
    policy: &SandboxPolicy,
    deny: &[PathBuf],
    home: Option<&Path>,
    chat: &Path,
) -> Result<Vec<PathBuf>, String> {
    let mut found: Vec<PathBuf> = deny.iter().filter_map(|d| d.canonicalize().ok()).collect();
    if let Some(home) = home {
        found.extend(home_matches(policy, home));
    }
    found.extend(walk_matches(policy, chat)?);
    found.sort();
    found.dedup();
    Ok(found)
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

/// The hidden paths in the chat folder. The walk does not follow links, but a link with
/// a hidden name hides its target.
fn walk_matches(policy: &SandboxPolicy, chat: &Path) -> Result<Vec<PathBuf>, String> {
    let lasts = last_parts();
    let mut found = Vec::new();
    let mut folders = vec![chat.to_path_buf()];
    let mut seen = 0;
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            seen += 1;
            if seen > MAX_WALK {
                return Err(format!(
                    "The chat folder holds more than {MAX_WALK} files and folders, more than the sandbox checks."
                ));
            }
            let path = entry.path();
            let name = entry.file_name();
            if could_match(&name.to_string_lossy(), &lasts) && hides(policy, &path) {
                push_real(&mut found, &path);
            } else if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                folders.push(path);
            }
        }
    }
    Ok(found)
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
/// and made read-only. No network, and the command dies with the shell of the agent.
pub fn bwrap_args(walls: &Walls, cwd: &Path, shell: &Path, command: &str) -> Vec<OsString> {
    let mut args = os(&["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]);
    let private = private_folders(walls);
    for folder in &private {
        args.extend(os(&["--tmpfs", folder]));
    }
    for path in &walls.writable {
        args.extend(["--bind".into(), path.into(), path.into()]);
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
    args.extend(["--chdir".into(), cwd.into()]);
    args.extend(os(&[
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--",
    ]));
    args.extend([shell.into(), "-c".into(), command.into()]);
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

/// One rule with one `subpath` filter for each path. No path gives no rule.
fn rule(out: &mut Vec<u8>, head: &str, paths: &[&Path]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    out.extend_from_slice(head.as_bytes());
    for path in paths {
        out.extend_from_slice(b" (subpath ");
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

/// A later rule wins in Seatbelt, so the hidden paths come last. Each path is an escaped
/// string literal (S32).
pub fn seatbelt_profile(walls: &Walls) -> Result<Vec<u8>, String> {
    let mut out = PROFILE_START.as_bytes().to_vec();
    let writable: Vec<&Path> = walls.writable.iter().map(PathBuf::as_path).collect();
    rule(&mut out, "(allow file-write*", &writable)?;
    let private = private_folders(walls).into_iter().map(Path::new);
    let hidden: Vec<&Path> = walls
        .hidden
        .iter()
        .map(PathBuf::as_path)
        .chain(private)
        .collect();
    rule(&mut out, "(deny file-read* file-write*", &hidden)?;
    Ok(out)
}

/// The program and its arguments that run `command` with `shell` inside the walls.
pub fn command_line(
    walls: &Walls,
    cwd: &Path,
    shell: &Path,
    command: &str,
) -> Result<(PathBuf, Vec<OsString>), String> {
    match &walls.tool {
        Sandbox::Bwrap(bwrap) => Ok((bwrap.clone(), bwrap_args(walls, cwd, shell, command))),
        Sandbox::Seatbelt => {
            let profile = seatbelt_profile(walls)?;
            let mut args = vec![OsString::from("-p"), bytes_arg(profile)];
            args.extend(["--".into(), shell.into(), "-c".into(), command.into()]);
            Ok((PathBuf::from(SANDBOX_EXEC), args))
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

/// The allowlist of SPEC.md 6.2 rule 12, and the temp folder of the run. The agent keeps
/// its keys, such as `ANTHROPIC_API_KEY`, and the command never sees them.
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
    let (program, args) = command_line(&walls, &cwd, &shell, command)?;
    std::fs::write(walls.temp.join(MARKER), b"").map_err(|e| format!("no marker: {e}"))?;
    let mut child = std::process::Command::new(program);
    child
        .args(args)
        .current_dir(&cwd)
        .env_clear()
        .envs(command_env(&walls, |name| std::env::var_os(name)));
    run(child)
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
        prepare(&sandbox(h, Sandbox::Seatbelt), &guarded, chat)
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

        let error = prepare(&inside, &guarded, &h.chat).err().unwrap();

        assert!(error.contains("inside the chat folder"), "{error}");
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
            empty: PathBuf::from("/data/sandbox/empty"),
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
    fn bwrap_binds_the_writable_paths_before_it_hides_and_ends_with_the_command() {
        let args = strings(&bwrap_args(
            &sample(),
            Path::new("/home/x/Code/app"),
            Path::new("/bin/bash"),
            "cargo test",
        ));

        assert_eq!(args[..3], ["--ro-bind", "/", "/"]);
        let tmp = position(&args, &["--tmpfs", "/tmp"]);
        let bind = position(&args, &["--bind", "/tmp/run1", "/tmp/run1"]);
        let hide = position(&args, &["--ro-bind", "/data/sandbox/empty", "/home/x/.ssh"]);
        let read_only = position(&args, &["--remount-ro", "/tmp"]);
        assert!(tmp < bind && bind < hide && hide < read_only);
        position(&args, &["--tmpfs", "/run"]);
        position(&args, &["--chdir", "/home/x/Code/app"]);
        for flag in ["--unshare-all", "--die-with-parent", "--new-session"] {
            assert!(args.contains(&flag.to_owned()), "{flag}");
        }
        assert_eq!(
            args[args.len() - 4..],
            ["--", "/bin/bash", "-c", "cargo test"]
        );
    }

    #[test]
    fn a_hidden_folder_gets_an_empty_folder_and_a_hidden_file_an_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(".ssh");
        std::fs::create_dir(&folder).unwrap();
        let mut walls = sample();
        walls.hidden = vec![folder.clone(), PathBuf::from("/home/x/Code/app/.env")];

        let args = strings(&bwrap_args(
            &walls,
            Path::new("/"),
            Path::new("/bin/bash"),
            "x",
        ));

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

        let (program, args) =
            command_line(&walls, Path::new("/"), Path::new("/bin/bash"), "make").unwrap();

        let args = strings(&args);
        assert_eq!(program, PathBuf::from(SANDBOX_EXEC));
        assert_eq!(args[0], "-p");
        assert!(args[1].starts_with("(version 1)"));
        assert_eq!(args[2..], ["--", "/bin/bash", "-c", "make"]);
    }

    #[test]
    fn with_no_sandbox_there_is_no_command_line() {
        let mut walls = sample();
        walls.tool = Sandbox::None;

        assert!(command_line(&walls, Path::new("/"), Path::new("/bin/bash"), "make").is_err());
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
            env,
            [
                ("PATH".to_owned(), OsString::from("/usr/bin")),
                ("TMPDIR".to_owned(), OsString::from("/tmp/run1")),
            ]
        );
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
