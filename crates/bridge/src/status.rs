//! The lines of `gnomish-relay status` and of setup: one line for each part that a player
//! checks first, with the next step when it does not work.

use std::ffi::OsStr;

use crate::agent;
use crate::config::RelayConfig;
use crate::gate::Gate;
use crate::install;
use crate::program::find_program;
use crate::story_sandbox::Sandbox;

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
            "Sandbox: none. Install bubblewrap so that allowed commands run with no question".into()
        }
        SandboxFound::Blocked => "Sandbox: none. bwrap is installed, but the system blocks \
             its user namespaces (on Ubuntu 24.04, AppArmor does). Add an AppArmor profile \
             for bwrap with userns, then run: gnomish-relay restart"
            .into(),
        SandboxFound::NoneOnThisOs => "Sandbox: none. Every command asks in the game".into(),
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
        return format!("Agent: none. {}", agent::NO_AGENT);
    };
    match checked {
        Ok(report) => format!("Agent: {name} ({} {})", report.name, report.version),
        Err(e) if install::needs_login(&e) => match install::login_command(name) {
            Some(login) => format!("Agent: {name} needs a login. Run: {login}"),
            None => format!("Agent: {name} needs a login."),
        },
        Err(e) => format!("Agent: {name} does not start: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
