//! `config.toml`: the ceiling for every message from the game (SPEC.md 6.6.2, 12).

use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use protocol::folder::resolve_folder;
use protocol::policy::{Level, effective_level};
use protocol::record::is_valid_id;
use serde::{Deserialize, Serialize};

use crate::claude;
use crate::relay::Folders;

pub const FILE: &str = "config.toml";
const MAX_FILE: u64 = 64 * 1024;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    #[default]
    Ask,
    AutoEdit,
    FullAuto,
}

impl Permission {
    /// An unknown level from the game counts as the strictest one.
    pub fn from_game(word: &str) -> Permission {
        match word {
            "auto-edit" => Permission::AutoEdit,
            "full-auto" => Permission::FullAuto,
            _ => Permission::Ask,
        }
    }

    fn level(self) -> Level {
        match self {
            Permission::Ask => Level::Ask,
            Permission::AutoEdit => Level::AutoEdit,
            Permission::FullAuto => Level::FullAuto,
        }
    }

    fn from_level(level: Level) -> Permission {
        match level {
            Level::Ask => Permission::Ask,
            Level::AutoEdit => Permission::AutoEdit,
            Level::FullAuto => Permission::FullAuto,
        }
    }

    /// The game can lower the level of the config, never raise it (S6).
    #[must_use]
    pub fn ceiling(self, requested: Option<Permission>) -> Permission {
        match requested {
            Some(requested) => {
                Permission::from_level(effective_level(self.level(), requested.level()))
            }
            None => self,
        }
    }
}

/// What a message from the game can reach.
pub struct Policy {
    pub folders: Folders,
    pub agents: BTreeMap<String, Permission>,
    pub default_agent: String,
}

pub struct Config {
    /// The game folder that holds `Interface`, `Screenshots`, and `WTF`.
    pub wow: PathBuf,
    pub policy: Policy,
    pub agents: BTreeMap<String, AgentSpec>,
    pub timeout: Duration,
    pub permission_timeout: Duration,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Any agent that speaks the Agent Client Protocol.
    Acp,
    /// Claude Code with no adapter, through `claude -p` (SPEC.md 9.2).
    Claude,
    /// Answers with the message. It tests the path through the game with no agent.
    Echo,
}

impl Kind {
    /// The word of the config.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Acp => "acp",
            Kind::Claude => "claude",
            Kind::Echo => "echo",
        }
    }
}

/// How to start one agent. A new ACP agent is one `[agents.<name>]` entry.
#[derive(Debug, PartialEq, Eq)]
pub struct AgentSpec {
    pub kind: Kind,
    pub command: Vec<String>,
    pub env: Vec<String>,
    pub modes: BTreeMap<Permission, String>,
}

/// Only the keys that the bridge uses. Any other key is an error, so a typo never
/// leaves a wider default in place.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    allowed_roots: Vec<String>,
    default_cwd: Option<String>,
    default_agent: String,
    timeout_minutes: Option<u64>,
    permission_timeout_minutes: Option<u64>,
    wow: Wow,
    agents: BTreeMap<String, Agent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wow {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Agent {
    kind: Kind,
    permission: Permission,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    modes: BTreeMap<Permission, String>,
}

const DEFAULT_TIMEOUT_MINUTES: u64 = 30;
const MAX_TIMEOUT_MINUTES: u64 = 240;
const DEFAULT_PERMISSION_MINUTES: u64 = 10;
const MAX_PERMISSION_MINUTES: u64 = 60;

fn minutes(value: Option<u64>, default: u64, max: u64, key: &str) -> Result<Duration> {
    let minutes = value.unwrap_or(default);
    if !(1..=max).contains(&minutes) {
        bail!("{key} must be 1 to {max}");
    }
    Ok(Duration::from_mins(minutes))
}

fn is_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn check_agent(name: &str, agent: &Agent) -> Result<()> {
    if !is_valid_id(name.as_bytes()) {
        bail!("agent name {name:?} is not a valid id");
    }
    match agent.kind {
        Kind::Acp | Kind::Claude if agent.command.first().is_none_or(String::is_empty) => {
            bail!("[agents.{name}] needs a command")
        }
        Kind::Echo if !agent.command.is_empty() => {
            bail!("[agents.{name}] is kind echo, so it has no command")
        }
        _ => {}
    }
    if let Some(bad) = agent.env.iter().find(|n| !is_env_name(n)) {
        bail!("[agents.{name}] env name {bad:?} is not A-Z, 0-9, and _");
    }
    if agent.kind == Kind::Claude {
        check_claude_modes(name, &agent.modes)?;
    }
    Ok(())
}

/// A typo in a mode name fails here, not at the first message from the game.
fn check_claude_modes(name: &str, modes: &BTreeMap<Permission, String>) -> Result<()> {
    for mode in modes.values() {
        if mode == claude::REFUSED_MODE {
            bail!("[agents.{name}] mode {mode} asks nothing, so the game cannot bound it");
        }
        if !claude::MODES.contains(&mode.as_str()) {
            bail!(
                "[agents.{name}] has no mode {mode:?}. The modes are {}",
                claude::MODES.join(", ")
            );
        }
    }
    Ok(())
}

pub fn expand(path: &str, home: &Path) -> Result<PathBuf> {
    let path = match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home.to_owned(),
        None => PathBuf::from(path),
    };
    if !path.is_absolute() {
        bail!("{} is not an absolute path", path.display());
    }
    Ok(path)
}

/// The proved resolver (S5) knows only `/`. Windows also splits at `\`, and
/// `canonicalize` there adds a `\\?\` prefix.
fn portable(path: &str, windows: bool) -> Vec<u8> {
    if !windows {
        return path.as_bytes().to_vec();
    }
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    path.replace('\\', "/").into_bytes()
}

/// A path in the form that the folder check takes, on every OS.
pub fn path_bytes(path: &Path) -> Vec<u8> {
    portable(&path.to_string_lossy(), cfg!(windows))
}

/// A folder from the game, in the form that the resolver checks. On Windows a `\`
/// becomes `/`, so each `..` counts. A `:` never passes there: it starts a drive
/// or names a stream.
/// The resolver starts its result with `/`. On Windows the drive comes first.
pub fn native_folder(resolved: Vec<u8>, windows: bool) -> Vec<u8> {
    match resolved.as_slice() {
        [b'/', _, b':', ..] if windows => resolved[1..].to_vec(),
        _ => resolved,
    }
}

/// The path from `base` to `target`, both resolved. The game sends it back, and it
/// resolves to `target` again. An absolute path can hold a drive, which the game
/// cannot send on Windows.
pub fn relative_folder(base: &[u8], target: &[u8]) -> Vec<u8> {
    let parts = |path: &'_ [u8]| -> Vec<Vec<u8>> {
        path.split(|&b| b == b'/')
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec)
            .collect()
    };
    let (base, target) = (parts(base), parts(target));
    let same = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut out: Vec<Vec<u8>> = vec![b"..".to_vec(); base.len() - same];
    out.extend(target[same..].iter().cloned());
    out.join(&b'/')
}

pub fn folder_request(raw: &[u8], windows: bool) -> Option<Vec<u8>> {
    if !windows {
        return Some(raw.to_vec());
    }
    if raw.contains(&b':') {
        return None;
    }
    Some(
        raw.iter()
            .map(|&b| if b == b'\\' { b'/' } else { b })
            .collect(),
    )
}

/// `canonicalize` resolves links, so a root that is a link names its real folder
/// (SPEC.md 6.2, rule 10).
fn real_root(path: &str, home: &Path) -> Result<Vec<u8>> {
    let path = expand(path, home)?;
    let real = path
        .canonicalize()
        .with_context(|| format!("allowed root {} does not exist", path.display()))?;
    Ok(path_bytes(&real))
}

pub fn parse(text: &str, home: &Path) -> Result<Config> {
    let file: File = toml::from_str(text)?;
    let roots = file
        .allowed_roots
        .iter()
        .map(|root| real_root(root, home))
        .collect::<Result<Vec<_>>>()?;
    let base = match &file.default_cwd {
        Some(cwd) => path_bytes(&expand(cwd, home)?),
        None => roots.first().context("allowed_roots is empty")?.clone(),
    };
    if resolve_folder(&roots, &base, b"").is_none() {
        bail!("default_cwd is outside allowed_roots");
    }
    for (name, agent) in &file.agents {
        check_agent(name, agent)?;
    }
    let timeout = minutes(
        file.timeout_minutes,
        DEFAULT_TIMEOUT_MINUTES,
        MAX_TIMEOUT_MINUTES,
        "timeout_minutes",
    )?;
    let permission_timeout = minutes(
        file.permission_timeout_minutes,
        DEFAULT_PERMISSION_MINUTES,
        MAX_PERMISSION_MINUTES,
        "permission_timeout_minutes",
    )?;
    if !file.agents.contains_key(&file.default_agent) {
        bail!(
            "default_agent {:?} has no [agents] entry",
            file.default_agent
        );
    }
    let levels = file
        .agents
        .iter()
        .map(|(name, agent)| (name.clone(), agent.permission))
        .collect();
    let agents = file
        .agents
        .into_iter()
        .map(|(name, agent)| {
            let spec = AgentSpec {
                kind: agent.kind,
                command: agent.command,
                env: agent.env,
                modes: agent.modes,
            };
            (name, spec)
        })
        .collect();
    Ok(Config {
        wow: expand(&file.wow.path, home)?,
        policy: Policy {
            folders: Folders { roots, base },
            agents: levels,
            default_agent: file.default_agent,
        },
        agents,
        timeout,
        permission_timeout,
    })
}

#[cfg(unix)]
fn others_can_write(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o022 != 0
}

// TODO: check the ACL when the Windows build ships.
#[cfg(not(unix))]
fn others_can_write(_meta: &fs::Metadata) -> bool {
    false
}

pub fn load(dir: &Path, home: &Path) -> Result<Config> {
    let path = dir.join(FILE);
    let meta = fs::symlink_metadata(&path).with_context(|| {
        format!(
            "cannot read {}. Run `gnomish-relay setup <wow folder>`",
            path.display()
        )
    })?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        bail!("{} is not a config file", path.display());
    }
    // Any user who can write the config can raise the ceiling of every message.
    if others_can_write(&meta) {
        bail!(
            "other users can write {}. Run: chmod 600 {}",
            path.display(),
            path.display()
        );
    }
    let text = fs::read_to_string(&path)?;
    parse(&text, home).with_context(|| format!("{} is not valid", path.display()))
}

/// An agent that setup found: its entry name, its kind, and its command.
pub type Found<'a> = (&'a str, Kind, &'a [&'a str]);

/// The first config: every agent asks, and agents work only in `roots`. It names the
/// agents that setup found; with none, the echo agent.
pub fn default_text(wow: &Path, agents: &[Found], roots: &[String]) -> String {
    let quote = |text: &str| format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""));
    let mut text = format!(
        "allowed_roots = [{}]\ndefault_agent = {}\n\n[wow]\npath = {}\n",
        roots
            .iter()
            .map(|r| quote(r))
            .collect::<Vec<_>>()
            .join(", "),
        quote(agents.first().map_or("echo", |(name, _, _)| name)),
        quote(&wow.to_string_lossy()),
    );
    for (name, kind, command) in agents {
        let command: Vec<String> = command.iter().map(|word| quote(word)).collect();
        let _ = write!(
            text,
            "\n[agents.{name}]\nkind = \"{}\"\ncommand = [{}]\npermission = \"ask\"\n",
            kind.word(),
            command.join(", ")
        );
    }
    if agents.is_empty() {
        text.push_str("\n[agents.echo]\nkind = \"echo\"\npermission = \"ask\"\n");
    }
    text.push_str(
        "\n# Any ACP agent is one entry. Run `gnomish-relay check-agent <name>` to test it.\n\
         # [agents.gemini]\n# kind = \"acp\"\n# command = [\"gemini\", \"--acp\"]\n\
         # permission = \"ask\"\n# env = [\"GEMINI_API_KEY\"]\n",
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_folder_goes_down_from_the_base_or_up_and_over() {
        let rel = |base: &str, target: &str| {
            String::from_utf8(relative_folder(base.as_bytes(), target.as_bytes())).unwrap()
        };
        assert_eq!(rel("/home/x/Code", "/home/x/Code/app/src"), "app/src");
        assert_eq!(rel("/home/x/Code", "/home/x/Code"), "");
        assert_eq!(rel("/home/x/Code/a", "/home/x/Work/b"), "../../Work/b");
        assert_eq!(rel("/C:/Users/x", "/C:/Users/x/repo"), "repo");
    }

    #[test]
    fn a_relative_folder_resolves_back_to_its_target() {
        let roots = [b"/home/x".to_vec()];
        let base = b"/home/x/Code/a";
        let target = b"/home/x/Work/b";
        let rel = relative_folder(base, target);
        assert_eq!(
            protocol::folder::resolve_folder(&roots, base, &rel).as_deref(),
            Some(target.as_slice())
        );
    }

    struct Home {
        dir: tempfile::TempDir,
    }

    impl Home {
        fn new() -> Home {
            let dir = tempfile::tempdir().unwrap();
            fs::create_dir_all(dir.path().join("Code/lighthouse")).unwrap();
            Home { dir }
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        fn parse(&self, text: &str) -> Result<Config> {
            parse(text, self.path())
        }
    }

    const GOOD: &str = r#"
        allowed_roots = ["~/Code"]
        default_agent = "claude"
        [wow]
        path = "~/wow"
        [agents.claude]
        kind = "acp"
        command = ["claude-agent-acp"]
        permission = "auto-edit"
    "#;

    #[test]
    fn a_good_config_gives_the_roots_the_agents_and_the_game_folder() {
        let home = Home::new();
        let config = home.parse(GOOD).unwrap();
        let root = home.path().join("Code").canonicalize().unwrap();
        assert_eq!(config.policy.folders.roots, [path_bytes(&root)]);
        assert_eq!(config.policy.folders.base, path_bytes(&root));
        assert_eq!(config.policy.agents["claude"], Permission::AutoEdit);
        assert_eq!(config.wow, home.path().join("wow"));
    }

    #[test]
    fn an_unknown_key_is_an_error() {
        let home = Home::new();
        assert!(
            home.parse(&format!("{GOOD}\nfull_auto_everything = true"))
                .is_err()
        );
        let typo = GOOD.replace("permission = ", "permision = ");
        assert!(home.parse(&typo).is_err());
    }

    #[test]
    fn an_agent_entry_gives_its_command_its_variables_and_its_modes() {
        let home = Home::new();
        let text = format!(
            "{GOOD}\nenv = [\"ANTHROPIC_API_KEY\"]\nmodes = {{ ask = \"plan\", auto-edit = \"default\" }}\n"
        );
        let config = home.parse(&text).unwrap();
        let claude = &config.agents["claude"];
        assert_eq!(claude.kind, Kind::Acp);
        assert_eq!(claude.command, ["claude-agent-acp"]);
        assert_eq!(claude.env, ["ANTHROPIC_API_KEY"]);
        assert_eq!(claude.modes[&Permission::Ask], "plan");
        assert_eq!(config.timeout, Duration::from_mins(30));
    }

    #[test]
    fn a_bad_agent_entry_is_an_error() {
        let home = Home::new();
        let bad = [
            GOOD.replace("kind = \"acp\"\n", ""),
            GOOD.replace("[\"claude-agent-acp\"]", "[]"),
            GOOD.replace("kind = \"acp\"", "kind = \"echo\""),
            format!("{GOOD}\nenv = [\"PATH; rm\"]\n"),
            format!("{GOOD}\nmodes = {{ root = \"x\" }}\n"),
        ];
        for text in bad {
            assert!(home.parse(&text).is_err(), "{text}");
        }
    }

    const CLAUDE: &str = r#"
        allowed_roots = ["~/Code"]
        default_agent = "claude"
        [wow]
        path = "~/wow"
        [agents.claude]
        kind = "claude"
        command = ["claude"]
        permission = "ask"
    "#;

    #[test]
    fn a_claude_entry_takes_the_modes_of_claude_code() {
        let home = Home::new();
        let text = format!("{CLAUDE}\nmodes = {{ ask = \"manual\", full-auto = \"auto\" }}\n");
        let config = home.parse(&text).unwrap();
        assert_eq!(config.agents["claude"].kind, Kind::Claude);
        assert_eq!(config.agents["claude"].modes[&Permission::Ask], "manual");
    }

    #[test]
    fn a_claude_entry_refuses_an_unknown_mode_and_the_mode_that_asks_nothing() {
        let home = Home::new();
        for mode in ["default", "bypassPermissions"] {
            let text = format!("{CLAUDE}\nmodes = {{ ask = \"{mode}\" }}\n");
            let error = format!("{:#}", home.parse(&text).err().unwrap());
            assert!(error.contains(mode), "{error}");
        }
        assert!(home.parse(&CLAUDE.replace("[\"claude\"]", "[]")).is_err());
    }

    #[test]
    fn an_old_entry_with_the_acp_adapter_of_claude_still_works() {
        let home = Home::new();
        let config = home.parse(GOOD).unwrap();
        assert_eq!(config.agents["claude"].kind, Kind::Acp);
        assert_eq!(config.agents["claude"].command, ["claude-agent-acp"]);
    }

    #[test]
    fn the_timeout_has_limits() {
        let home = Home::new();
        let with = |minutes: u64| {
            GOOD.replace(
                "default_agent",
                &format!("timeout_minutes = {minutes}\ndefault_agent"),
            )
        };
        assert_eq!(
            home.parse(&with(5)).unwrap().timeout,
            Duration::from_mins(5)
        );
        assert!(home.parse(&with(0)).is_err());
        assert!(home.parse(&with(241)).is_err());
    }

    #[test]
    fn an_unknown_permission_is_an_error() {
        let home = Home::new();
        assert!(home.parse(&GOOD.replace("auto-edit", "yolo")).is_err());
    }

    #[test]
    fn a_missing_root_is_an_error() {
        let home = Home::new();
        assert!(home.parse(&GOOD.replace("~/Code", "~/Nope")).is_err());
    }

    #[test]
    fn a_relative_root_is_an_error() {
        let home = Home::new();
        assert!(home.parse(&GOOD.replace("~/Code", "Code")).is_err());
    }

    #[test]
    fn a_default_folder_outside_the_roots_is_an_error() {
        let home = Home::new();
        let text = GOOD.replace("default_agent", "default_cwd = \"/etc\"\ndefault_agent");
        assert!(home.parse(&text).is_err());
    }

    #[test]
    fn a_default_agent_with_no_entry_is_an_error() {
        let home = Home::new();
        assert!(
            home.parse(&GOOD.replace("\"claude\"", "\"codex\""))
                .is_err()
        );
    }

    #[test]
    fn the_default_config_parses_and_every_agent_asks() {
        let home = Home::new();
        fs::create_dir_all(home.path().join("Documents/Code")).unwrap();
        // A quote and a backslash in the folder name must not break the TOML string.
        let wow = if cfg!(windows) {
            r#"C:\Games\"wow""#
        } else {
            r#"/games/"wow"\x"#
        };
        let agents: [Found; 2] = [
            ("claude", Kind::Claude, &["claude"]),
            ("gemini", Kind::Acp, &["gemini", "--acp"]),
        ];
        let roots = ["~/Documents/Code".to_owned()];
        let config = home
            .parse(&default_text(Path::new(wow), &agents, &roots))
            .unwrap();
        assert_eq!(config.policy.agents["claude"], Permission::Ask);
        assert_eq!(config.policy.default_agent, "claude");
        assert_eq!(config.agents["gemini"].command, ["gemini", "--acp"]);
        assert_eq!(config.agents["claude"].kind, Kind::Claude);
        assert_eq!(config.wow, PathBuf::from(wow));
    }

    #[test]
    fn a_windows_request_splits_at_backslashes_and_never_names_a_drive() {
        assert_eq!(folder_request(br"..\..\x", true).unwrap(), b"../../x");
        assert_eq!(folder_request(br"sub\dir", true).unwrap(), b"sub/dir");
        assert_eq!(folder_request(br"C:\Windows", true), None);
        assert_eq!(folder_request(b"file.txt:stream", true), None);
        assert_eq!(folder_request(br"a\b", false).unwrap(), br"a\b");
    }

    #[test]
    fn a_windows_folder_starts_with_its_drive() {
        assert_eq!(native_folder(b"/C:/Code/x".to_vec(), true), b"C:/Code/x");
        assert_eq!(native_folder(b"/home/x".to_vec(), true), b"/home/x");
        assert_eq!(native_folder(b"/C:/x".to_vec(), false), b"/C:/x");
    }

    #[test]
    fn a_windows_root_loses_its_prefix_and_uses_slashes() {
        assert_eq!(portable(r"\\?\C:\Users\x\Code", true), b"C:/Users/x/Code");
        assert_eq!(portable(r"/home/x\y", false), br"/home/x\y");
    }

    #[test]
    fn with_no_agent_found_the_default_config_uses_echo() {
        let home = Home::new();
        fs::create_dir_all(home.path().join("Documents/Code")).unwrap();
        let roots = ["~/Documents/Code".to_owned()];
        let config = home
            .parse(&default_text(&home.path().join("wow"), &[], &roots))
            .unwrap();
        assert_eq!(config.policy.default_agent, "echo");
        assert_eq!(config.agents["echo"].kind, Kind::Echo);
    }

    #[test]
    fn the_game_can_lower_the_level_but_never_raise_it() {
        let ask = Permission::Ask;
        let edit = Permission::AutoEdit;
        assert_eq!(edit.ceiling(Some(Permission::FullAuto)), edit);
        assert_eq!(edit.ceiling(Some(ask)), ask);
        assert_eq!(edit.ceiling(None), edit);
        assert_eq!(Permission::from_game("root"), ask);
    }

    #[cfg(unix)]
    #[test]
    fn a_config_that_others_can_write_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let home = Home::new();
        let dir = home.path().join("config");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join(FILE), GOOD).unwrap();
        fs::set_permissions(dir.join(FILE), fs::Permissions::from_mode(0o666)).unwrap();
        assert!(load(&dir, home.path()).is_err());
        fs::set_permissions(dir.join(FILE), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load(&dir, home.path()).is_ok());
    }
}
