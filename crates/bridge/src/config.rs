//! `config.toml`: the ceiling for every message from the game (SPEC.md 6.6.2, 12).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use protocol::folder::resolve_folder;
use protocol::policy::{Level, effective_level};
use protocol::record::is_valid_id;
use serde::{Deserialize, Serialize};

use crate::relay::Folders;

pub const FILE: &str = "config.toml";
const MAX_FILE: u64 = 64 * 1024;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
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
}

/// Only the keys that the bridge uses. Any other key is an error, so a typo never
/// leaves a wider default in place.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    allowed_roots: Vec<String>,
    default_cwd: Option<String>,
    default_agent: String,
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
    permission: Permission,
}

fn expand(path: &str, home: &Path) -> Result<PathBuf> {
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

fn bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

/// `canonicalize` resolves links, so a root that is a link names its real folder
/// (SPEC.md 6.2, rule 10).
fn real_root(path: &str, home: &Path) -> Result<Vec<u8>> {
    let path = expand(path, home)?;
    let real = path
        .canonicalize()
        .with_context(|| format!("allowed root {} does not exist", path.display()))?;
    Ok(bytes(&real))
}

pub fn parse(text: &str, home: &Path) -> Result<Config> {
    let file: File = toml::from_str(text)?;
    let roots = file
        .allowed_roots
        .iter()
        .map(|root| real_root(root, home))
        .collect::<Result<Vec<_>>>()?;
    let base = match &file.default_cwd {
        Some(cwd) => bytes(&expand(cwd, home)?),
        None => roots.first().context("allowed_roots is empty")?.clone(),
    };
    if resolve_folder(&roots, &base, b"").is_none() {
        bail!("default_cwd is outside allowed_roots");
    }
    if let Some(name) = file.agents.keys().find(|n| !is_valid_id(n.as_bytes())) {
        bail!("agent name {name:?} is not a valid id");
    }
    if !file.agents.contains_key(&file.default_agent) {
        bail!(
            "default_agent {:?} has no [agents] entry",
            file.default_agent
        );
    }
    Ok(Config {
        wow: expand(&file.wow.path, home)?,
        policy: Policy {
            folders: Folders { roots, base },
            agents: file
                .agents
                .into_iter()
                .map(|(name, agent)| (name, agent.permission))
                .collect(),
            default_agent: file.default_agent,
        },
    })
}

/// Any user who can write the config can raise the ceiling of every message.
#[cfg(unix)]
fn check_owner_only(meta: &fs::Metadata, path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if meta.permissions().mode() & 0o022 != 0 {
        bail!(
            "other users can write {}. Run: chmod 600 {}",
            path.display(),
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_owner_only(_meta: &fs::Metadata, _path: &Path) -> Result<()> {
    Ok(())
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
    check_owner_only(&meta, &path)?;
    let text = fs::read_to_string(&path)?;
    parse(&text, home).with_context(|| format!("{} is not valid", path.display()))
}

/// The first config: every agent asks, and agents work only under `Documents/Code`.
pub fn default_text(wow: &Path) -> String {
    let wow = wow
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!(
        "allowed_roots = [\"~/Documents/Code\"]\n\
         default_agent = \"claude\"\n\
         \n\
         [wow]\n\
         path = \"{wow}\"\n\
         \n\
         [agents.claude]\n\
         permission = \"ask\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        path = "/games/wow"
        [agents.claude]
        permission = "auto-edit"
    "#;

    #[test]
    fn a_good_config_gives_the_roots_the_agents_and_the_game_folder() {
        let home = Home::new();
        let config = home.parse(GOOD).unwrap();
        let root = home.path().join("Code").canonicalize().unwrap();
        assert_eq!(config.policy.folders.roots, [bytes(&root)]);
        assert_eq!(config.policy.folders.base, bytes(&root));
        assert_eq!(config.policy.agents["claude"], Permission::AutoEdit);
        assert_eq!(config.wow, PathBuf::from("/games/wow"));
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
        let config = home.parse(&default_text(Path::new(wow))).unwrap();
        assert_eq!(config.policy.agents["claude"], Permission::Ask);
        assert_eq!(config.wow, PathBuf::from(wow));
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
