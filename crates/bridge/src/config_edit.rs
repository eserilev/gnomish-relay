//! The changes that the bridge makes to `config.toml` by itself, each after a click on
//! the desktop: the `permission` of one agent (SPEC.md 9.3), and a new root (9.12).
//! Each changes one line, so the comments and the other keys stay.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::config::{self, Permission};
use crate::config_text::quote;

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

const ROOTS: &str = "allowed_roots";

/// The text with `root` added at the end of `allowed_roots`. With no root and no
/// `default_cwd` before, it also adds `default_cwd = "~"`, so the base of the folders
/// of the game stays the home folder.
pub fn with_root(text: &str, root: &str, home: &Path) -> Result<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let at = roots_line(&lines)?;
    let (roots, rest) = roots_and_rest(lines[at])?;
    if roots.iter().any(|r| r == root) {
        bail!("{root} is already in allowed_roots");
    }
    let before = config::parse(text, home).context("config.toml does not load now")?;
    let mut new_roots = roots.clone();
    new_roots.push(root.to_owned());
    let quoted: Vec<String> = new_roots.iter().map(|r| quote(r)).collect();
    let mut new_line = format!("{ROOTS} = [{}]{rest}", quoted.join(", "));
    if roots.is_empty() && !has_default_cwd(text)? {
        new_line.push_str("default_cwd = \"~\"\n");
    }
    let mut changed = String::with_capacity(text.len() + root.len() + 32);
    for (i, line) in lines.iter().enumerate() {
        changed.push_str(if i == at { &new_line } else { line });
    }
    check_only_the_roots_changed(&before, &changed, root, home)?;
    Ok(changed)
}

fn is_roots_key(line: &str) -> bool {
    line.trim_start()
        .strip_prefix(ROOTS)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// The top keys come before the first table.
fn roots_line(lines: &[&str]) -> Result<usize> {
    let top = lines
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .unwrap_or(lines.len());
    let found: Vec<usize> = (0..top).filter(|&i| is_roots_key(lines[i])).collect();
    match found.as_slice() {
        [one] => Ok(*one),
        _ => bail!("config.toml has no single {ROOTS} = [...] line before its first table"),
    }
}

#[derive(serde::Deserialize)]
struct RootsOnly {
    allowed_roots: Vec<String>,
}

/// The roots of the line, and what comes after the list: a comment and the line end.
/// The shortest start of the line that parses ends at the `]` of the list.
fn roots_and_rest(line: &str) -> Result<(Vec<String>, &str)> {
    for (end, _) in line.match_indices(']') {
        if let Ok(parsed) = toml::from_str::<RootsOnly>(&line[..=end]) {
            return Ok((parsed.allowed_roots, &line[end + 1..]));
        }
    }
    bail!("the {ROOTS} line of config.toml has no list of paths on one line")
}

fn has_default_cwd(text: &str) -> Result<bool> {
    let table: toml::Table = toml::from_str(text).context("config.toml does not parse")?;
    Ok(table.contains_key("default_cwd"))
}

fn check_only_the_roots_changed(
    before: &config::Config,
    new: &str,
    root: &str,
    home: &Path,
) -> Result<()> {
    let after = config::parse(new, home).context("the changed config.toml does not load")?;
    let (old, now) = (before.require_relay()?, after.require_relay()?);
    let real_root = config::expand(root, home)?
        .canonicalize()
        .context("the new root does not exist")?;
    let mut expected = old.policy.folders.roots.clone();
    expected.push(crate::folder_path::path_bytes(&real_root));
    let same_rest = old.policy.folders.base == now.policy.folders.base
        && old.policy.agents == now.policy.agents
        && old.policy.default_agent == now.policy.default_agent;
    if now.policy.folders.roots != expected || !same_rest {
        bail!("the change of config.toml changed more than allowed_roots");
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

    fn with_lighthouse(home: &tempfile::TempDir) {
        std::fs::create_dir_all(home.path().join("lighthouse")).unwrap();
    }

    #[test]
    fn a_new_root_goes_at_the_end_of_the_roots_and_every_other_line_stays() {
        let home = home();
        with_lighthouse(&home);

        let changed = with_root(CONFIG, "~/lighthouse", home.path()).unwrap();

        let expected = CONFIG.replace(
            "allowed_roots = [\"~/Code\"]\n",
            "allowed_roots = [\"~/Code\", \"~/lighthouse\"]\n",
        );
        assert_eq!(changed, expected);
    }

    #[test]
    fn a_comment_after_the_roots_stays() {
        let home = home();
        with_lighthouse(&home);
        let text = CONFIG.replace("[\"~/Code\"]\n", "[\"~/Code\"]   # mine]\n");

        let changed = with_root(&text, "~/lighthouse", home.path()).unwrap();

        assert!(
            changed.starts_with("allowed_roots = [\"~/Code\", \"~/lighthouse\"]   # mine]\n"),
            "{changed}"
        );
    }

    #[test]
    fn the_first_root_keeps_the_home_folder_as_the_default_folder() {
        let home = home();
        with_lighthouse(&home);
        let text = CONFIG.replace("[\"~/Code\"]", "[]");

        let changed = with_root(&text, "~/lighthouse", home.path()).unwrap();

        assert!(
            changed.starts_with(
                "allowed_roots = [\"~/lighthouse\"]\ndefault_cwd = \"~\"\ndefault_agent"
            ),
            "{changed}"
        );
        let config = config::parse(&changed, home.path()).unwrap();
        let base = &config.require_relay().unwrap().policy.folders.base;
        let real_home = home.path().canonicalize().unwrap();
        assert_eq!(base, &crate::folder_path::path_bytes(&real_home));
    }

    #[test]
    fn a_config_with_its_roots_in_another_form_gets_no_new_root() {
        let home = home();
        with_lighthouse(&home);
        let forms = [
            CONFIG.replace(
                "allowed_roots = [\"~/Code\"]\n",
                "allowed_roots = [\n  \"~/Code\",\n]\n",
            ),
            CONFIG.replace("allowed_roots = [\"~/Code\"]\n", ""),
            format!("allowed_roots = [\"~/Code\"]\n{CONFIG}"),
            CONFIG.replace("[wow]", "[wow"),
        ];
        for form in forms {
            assert!(
                with_root(&form, "~/lighthouse", home.path()).is_err(),
                "{form}"
            );
        }
    }

    #[test]
    fn a_missing_folder_or_a_root_that_is_there_is_never_added() {
        let home = home();

        assert!(with_root(CONFIG, "~/lighthouse", home.path()).is_err());
        assert!(with_root(CONFIG, "~/Code", home.path()).is_err());
    }
}
