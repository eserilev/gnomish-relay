//! `gnomish-relay check-agent <name>`: starts one agent of the config and opens a session
//! in the default folder, with no prompt. It shows that a new `[agents.<name>]` entry works.

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::{Context, Result, bail};

use crate::agent::{self, Report};
use crate::config::{self, AgentSpec, Permission, RelayConfig};
use crate::desktop::Prompt;
use crate::dirs::Dirs;
use crate::gate::{Gate, Places};
use crate::install;
use crate::status;

/// A check sends no prompt, so no tool call reaches this gate.
pub fn check_gate(dirs: &Dirs, config: &RelayConfig) -> Gate {
    Gate::new(config, &Places::of(dirs), Prompt::Off)
}

/// The lines of the report come before an error, so the user sees what the agent offers.
pub fn check_agent(dirs: &Dirs, name: &str, out: &mut dyn Write) -> Result<()> {
    let config = config::load(&dirs.config, &dirs.home)?;
    let config = config.require_relay()?;
    let spec = config
        .agents
        .get(name)
        .with_context(|| format!("the config has no [agents.{name}]"))?;
    let cwd = String::from_utf8_lossy(&config.policy.folders.base).into_owned();
    let report = agent::check(name, spec, &cwd, &check_gate(dirs, config))
        .with_context(|| format!("[agents.{name}] is the echo agent: it starts nothing"))?
        .map_err(anyhow::Error::msg)?;
    for line in report_lines(name, &report) {
        writeln!(out, "{line}")?;
    }
    if let Some(line) = service_path_line(dirs, spec) {
        writeln!(out, "{line}")?;
    }
    check_modes(&spec.modes, &report.modes)?;
    writeln!(out, "ok")?;
    Ok(())
}

fn report_lines(name: &str, report: &Report) -> Vec<String> {
    let resumes = if report.load_session { "yes" } else { "no" };
    let modes = if report.modes.is_empty() {
        "none".to_owned()
    } else {
        report.modes.join(", ")
    };
    let mut lines = vec![
        format!("{name}: {} {}", report.name, report.version),
        format!("resumes sessions: {resumes}"),
        format!("modes: {modes}"),
    ];
    lines.extend(report.details.iter().cloned());
    lines
}

/// The login service has its own `PATH`, which can miss the program of the agent.
fn service_path_line(dirs: &Dirs, spec: &AgentSpec) -> Option<String> {
    let service = install::service_file(&dirs.config, &dirs.home)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| install::service_path_var(&text));
    let program = spec.command.first()?;
    status::service_path_line(program, service.as_deref())
}

fn check_modes(modes: &BTreeMap<Permission, String>, offered: &[String]) -> Result<()> {
    for (level, mode) in modes {
        if !offered.contains(mode) {
            bail!("the agent has no mode {mode:?}, which the config names for {level:?}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(modes: &[&str]) -> Report {
        Report {
            name: "Claude Code".into(),
            version: "9.9.9".into(),
            load_session: true,
            modes: modes.iter().map(|m| (*m).to_owned()).collect(),
            details: vec!["one more fact".into()],
        }
    }

    #[test]
    fn the_report_shows_the_version_the_sessions_the_modes_and_the_details() {
        let lines = report_lines("claude", &report(&["plan", "auto"]));
        assert_eq!(
            lines,
            [
                "claude: Claude Code 9.9.9",
                "resumes sessions: yes",
                "modes: plan, auto",
                "one more fact",
            ]
        );
    }

    #[test]
    fn a_report_with_no_modes_says_none() {
        let lines = report_lines("codex", &report(&[]));
        assert_eq!(lines[2], "modes: none");
    }

    #[test]
    fn a_mode_of_the_config_that_the_agent_does_not_offer_is_an_error() {
        let modes = BTreeMap::from([(Permission::Ask, "plan".to_owned())]);
        assert!(check_modes(&modes, &["plan".to_owned()]).is_ok());
        let error = check_modes(&modes, &["auto".to_owned()]).unwrap_err();
        assert!(error.to_string().contains("no mode \"plan\""), "{error}");
    }
}
