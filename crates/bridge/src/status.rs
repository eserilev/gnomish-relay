//! The lines of `gnomish-relay status` and of setup: one line for each part that a player
//! checks first, with the next step when it does not work.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::agent;
use crate::config::{self, RelayConfig};
use crate::desktop::Prompt;
use crate::fs_safe::write_atomic_unsynced;
use crate::gate::{Gate, Places};
use crate::install;
use crate::lock::{self, Bridge};
use crate::program::find_program;
use crate::story_sandbox::Sandbox;

/// In the data folder. The bridge writes the time of each strip that it takes.
const LAST_STRIP_FILE: &str = "last-strip";

/// The lines of `gnomish-relay status`. `path` is the `PATH` for the sandbox probe.
pub fn status_lines(places: &Places, path: &OsStr, now: u32) -> Vec<String> {
    let mut lines = vec![
        bridge_line(&lock::status(places.data_dir)),
        last_strip_line(last_strip(places.data_dir), now),
    ];
    let config = match config::load(places.config_dir, places.home) {
        Ok(config) => config,
        Err(e) => {
            lines.push(format!("Config: has an error. {e:#}"));
            return lines;
        }
    };
    lines.push("Config: OK".into());
    let Some(relay) = &config.relay else {
        lines.push("Coding agents: off. To turn them on, run gnomish-relay setup --relay".into());
        return lines;
    };
    let gate = Gate::new(relay, places, Prompt::Off);
    lines.push(sandbox_line(&SandboxFound::of(&gate.sandbox.tool, path)));
    lines.push(agent_line(relay, &gate));
    lines.extend(default_agent_off_service_path(relay, places));
    lines
}

fn default_agent_off_service_path(relay: &RelayConfig, places: &Places) -> Option<String> {
    let spec = relay.agents.get(&relay.policy.default_agent)?;
    let program = spec.command.first()?;
    let file = install::service_file(places.home)?;
    let service_path = install::service_path_var(&fs::read_to_string(file).ok()?);
    service_path_line(program, service_path.as_deref())
}

pub fn bridge_line(bridge: &Result<Bridge>) -> String {
    match bridge {
        Ok(Bridge::Stopped) => {
            "Desktop app: stopped. To start it, run gnomish-relay restart".into()
        }
        Ok(Bridge::Runs(Some(pid))) => format!("Desktop app: running (process {pid})"),
        Ok(Bridge::Runs(None)) => "Desktop app: running".into(),
        Err(e) => format!("Desktop app: can't tell whether it's running. {e:#}"),
    }
}

pub fn mark_strip(data_dir: &Path, now: u32) -> Result<()> {
    write_atomic_unsynced(data_dir, LAST_STRIP_FILE, now.to_string().as_bytes())
}

pub fn last_strip(data_dir: &Path) -> Option<u32> {
    fs::read_to_string(data_dir.join(LAST_STRIP_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn last_strip_line(last: Option<u32>, now: u32) -> String {
    let Some(last) = last else {
        return "Last message from WoW: none yet. Send one in the game to test".into();
    };
    format!(
        "Last message from WoW: {} ago",
        duration_text(now.saturating_sub(last))
    )
}

fn duration_text(seconds: u32) -> String {
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        3600..86400 => format!("{} h", seconds / 3600),
        _ => format!("{} days", seconds / 86400),
    }
}

/// What the sandbox probe found (SPEC.md 6.6.4).
#[derive(Debug, PartialEq, Eq)]
pub enum SandboxFound {
    Tool(&'static str),
    /// Linux with no `bwrap` on `PATH`.
    NotInstalled,
    /// Linux with a `bwrap` that cannot make its namespaces.
    Blocked,
    /// Windows has no sandbox for the commands of Claude (SPEC.md 11.2).
    NoneOnThisOs,
}

impl SandboxFound {
    pub fn of(tool: &Sandbox, path: &OsStr) -> SandboxFound {
        if *tool != Sandbox::None {
            return SandboxFound::Tool(tool.name());
        }
        if !cfg!(target_os = "linux") {
            return SandboxFound::NoneOnThisOs;
        }
        match find_program("bwrap", path, false) {
            Some(_) => SandboxFound::Blocked,
            None => SandboxFound::NotInstalled,
        }
    }
}

pub fn sandbox_line(found: &SandboxFound) -> String {
    match found {
        SandboxFound::Tool(name) => format!("Sandbox: {name}"),
        SandboxFound::NotInstalled => {
            "Sandbox: none. Install bubblewrap so allowed commands can run without asking".into()
        }
        SandboxFound::Blocked => "Sandbox: none. bwrap is installed, but the system blocks \
             its user namespaces (on Ubuntu 24.04, AppArmor does). Add an AppArmor profile \
             for bwrap with userns, then run gnomish-relay restart"
            .into(),
        SandboxFound::NoneOnThisOs => "Sandbox: none. Every command asks in the game first".into(),
    }
}

/// The default agent, started once with no prompt, so a missing login shows here and
/// not as the first reply in the game.
pub fn agent_line(config: &RelayConfig, gate: &Gate) -> String {
    let name = &config.policy.default_agent;
    let cwd = String::from_utf8_lossy(&config.policy.folders.base).into_owned();
    let checked = config
        .agents
        .get(name)
        .and_then(|spec| agent::check(name, spec, &cwd, gate));
    let Some(checked) = checked else {
        return "Agent: none. Install Claude Code or Codex, then run gnomish-relay setup".into();
    };
    match checked {
        Ok(report) => format!("Agent: {name} ({} {})", report.name, report.version),
        Err(e) if install::needs_login(&e) => match install::login_command(name) {
            Some(login) => format!("Agent: {name} isn't logged in. Run {login}"),
            None => format!("Agent: {name} isn't logged in."),
        },
        Err(e) => format!("Agent: {name} doesn't start: {e}"),
    }
}

/// `None` when the login service finds `program`, or when there is no service. The
/// service keeps the `PATH` of the last setup or restart, and a shell can have another.
pub fn service_path_line(program: &str, service_path: Option<&str>) -> Option<String> {
    let path = service_path?;
    if find_program(program, OsStr::new(path), cfg!(windows)).is_some() {
        return None;
    }
    Some(format!(
        "{program} isn't on the PATH that the desktop app gets at login. To fix it, run gnomish-relay restart in this shell"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_strip_says_how_long_ago_it_came() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            last_strip_line(last_strip(dir.path()), 1000),
            "Last message from WoW: none yet. Send one in the game to test"
        );

        mark_strip(dir.path(), 1000).unwrap();

        assert_eq!(last_strip(dir.path()), Some(1000));
        assert_eq!(
            last_strip_line(Some(1000), 1030),
            "Last message from WoW: 30 s ago"
        );
        assert_eq!(
            last_strip_line(Some(1000), 1000 + 180),
            "Last message from WoW: 3 min ago"
        );
        assert_eq!(
            last_strip_line(Some(1000), 1000 + 7200),
            "Last message from WoW: 2 h ago"
        );
        assert_eq!(
            last_strip_line(Some(1000), 1000 + 3 * 86400),
            "Last message from WoW: 3 days ago"
        );
    }

    #[test]
    fn the_bridge_line_says_whether_the_bridge_runs() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            bridge_line(&lock::status(dir.path())),
            "Desktop app: stopped. To start it, run gnomish-relay restart"
        );

        let _lock = lock::take(dir.path()).unwrap();

        assert_eq!(
            bridge_line(&lock::status(dir.path())),
            format!("Desktop app: running (process {})", std::process::id())
        );
    }

    #[test]
    fn an_agent_that_the_service_cannot_find_asks_for_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().into_owned();

        let line = service_path_line("claude", Some(&path));

        assert_eq!(
            line.as_deref(),
            Some(
                "claude isn't on the PATH that the desktop app gets at login. To fix it, run gnomish-relay restart in this shell"
            )
        );
    }

    #[test]
    fn an_agent_on_the_path_of_the_service_or_no_service_needs_no_line() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        std::fs::write(dir.path().join(name), "").unwrap();
        let path = dir.path().to_string_lossy().into_owned();

        assert_eq!(service_path_line("claude", Some(&path)), None);
        assert_eq!(service_path_line("claude", None), None);
    }

    #[test]
    fn a_working_sandbox_shows_its_tool() {
        let found = SandboxFound::of(&Sandbox::Seatbelt, OsStr::new(""));

        assert_eq!(sandbox_line(&found), "Sandbox: sandbox-exec");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn with_no_bwrap_on_the_path_the_line_says_to_install_bubblewrap() {
        let empty = tempfile::tempdir().unwrap();

        let found = SandboxFound::of(&Sandbox::None, empty.path().as_os_str());

        assert_eq!(found, SandboxFound::NotInstalled);
        assert!(sandbox_line(&found).contains("Install bubblewrap"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_bwrap_that_fails_its_probe_gets_the_apparmor_hint() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bwrap"), "").unwrap();

        let found = SandboxFound::of(&Sandbox::None, dir.path().as_os_str());

        assert_eq!(found, SandboxFound::Blocked);
        assert!(sandbox_line(&found).contains("AppArmor"));
    }
}
