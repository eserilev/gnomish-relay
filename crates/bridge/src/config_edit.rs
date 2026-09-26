//! The one change that the bridge makes to `config.toml` by itself: the `permission`
//! of one agent, after a click on the desktop (SPEC.md 9.3). It changes one line, so
//! the comments and the other keys stay.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::config::{self, Permission};

/// The text with `permission = "<level>"` for `agent`. It fails, and changes nothing,
/// for any form but a plain `[agents.<agent>]` table with one `permission = "..."`
/// line, and for a result in which anything but that level changed.
pub fn with_permission(text: &str, agent: &str, level: Permission, home: &Path) -> Result<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let at = permission_line(&lines, agent)?;
    let mut changed = String::with_capacity(text.len());
    for (i, line) in lines.iter().enumerate() {
        if i == at {
            changed.push_str(&new_value(line, level)?);
        } else {
            changed.push_str(line);
        }
    }
    check_only_the_level_changed(text, &changed, agent, level, home)?;
    Ok(changed)
}

fn without_comment(line: &str) -> &str {
    line.split('#').next().unwrap_or_default().trim()
}

fn is_permission_key(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("permission")
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// The table runs from its header to the next line that starts with `[`.
fn permission_line(lines: &[&str], agent: &str) -> Result<usize> {
    let header = format!("[agents.{agent}]");
    let start = lines
        .iter()
        .position(|line| without_comment(line) == header)
        .with_context(|| format!("config.toml has no plain {header} table"))?;
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |n| start + 1 + n);
    let found: Vec<usize> = (start + 1..end)
        .filter(|&i| is_permission_key(lines[i]))
        .collect();
    match found.as_slice() {
        [one] => Ok(*one),
        _ => bail!("{header} in config.toml has no single permission = \"...\" line"),
    }
}

/// Keeps the indent, and the comment and the line end after the value.
fn new_value(line: &str, level: Permission) -> Result<String> {
    let indent = &line[..line.len() - line.trim_start().len()];
    let rest = line
        .split_once('=')
        .map(|(_, value)| value.trim_start())
        .and_then(|value| value.strip_prefix('"'))
        .and_then(|value| value.split_once('"'))
        .map(|(_, rest)| rest)
        .context("the permission line of config.toml has no plain \"...\" value")?;
    Ok(format!("{indent}permission = \"{}\"{rest}", level.word()))
}

fn check_only_the_level_changed(
    old: &str,
    new: &str,
    agent: &str,
    level: Permission,
    home: &Path,
) -> Result<()> {
    let before = config::parse(old, home).context("config.toml does not load now")?;
    let after = config::parse(new, home).context("the changed config.toml does not load")?;
    let mut expected = before.require_relay()?.policy.agents.clone();
    expected.insert(agent.to_owned(), level);
    if after.require_relay()?.policy.agents != expected {
        bail!("the change of config.toml changed more than the level of {agent}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "allowed_roots = [\"~/Code\"]\n\
        default_agent = \"claude\"\n\
        # my own comment\n\
        [wow]\npath = \"~/wow\"\n\
        \n[agents.claude]\nkind = \"claude\"\ncommand = [\"claude\"]\n\
        permission = \"ask\"   # set by me\n\
        \n[agents.codex]\nkind = \"codex\"\ncommand = [\"codex\"]\npermission = \"ask\"\n";

    fn home() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("Code")).unwrap();
        home
    }

    #[test]
    fn a_raise_changes_only_the_permission_line_of_that_agent() {
        let home = home();
        let changed = with_permission(CONFIG, "claude", Permission::AutoEdit, home.path()).unwrap();
        let expected = CONFIG.replace(
            "permission = \"ask\"   # set by me",
            "permission = \"auto-edit\"   # set by me",
        );
        assert_eq!(changed, expected);
    }

    #[test]
    fn a_config_in_another_form_is_never_changed() {
        let home = home();
        let forms = [
            CONFIG.replace("[agents.claude]", "[agents.\"claude\"]"),
            CONFIG.replace("permission = \"ask\"   # set by me\n", ""),
            CONFIG.replace("permission = \"ask\"   #", "permission = 'ask'   #"),
            CONFIG.replace("# set by me", "\npermission = \"ask\""),
        ];
        for form in forms {
            let result = with_permission(&form, "claude", Permission::AutoEdit, home.path());
            assert!(result.is_err(), "{form}");
        }
    }

    #[test]
    fn a_malformed_config_is_never_changed() {
        let home = home();
        let broken = CONFIG.replace("[wow]", "[wow");
        let result = with_permission(&broken, "claude", Permission::AutoEdit, home.path());
        assert!(result.is_err());
    }

    #[test]
    fn the_permission_of_another_agent_stays() {
        let home = home();
        let changed = with_permission(CONFIG, "codex", Permission::FullAuto, home.path()).unwrap();
        let config = config::parse(&changed, home.path()).unwrap();
        let agents = &config.require_relay().unwrap().policy.agents;
        assert_eq!(agents["codex"], Permission::FullAuto);
        assert_eq!(agents["claude"], Permission::Ask);
    }
}
