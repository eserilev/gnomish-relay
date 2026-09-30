//! `gnomish-relay hooks install`, `remove`, and `status`: our hooks in the settings of
//! Claude Code and Codex (SPEC.md 10.5). The settings of an agent belong to the user, so
//! only this explicit command changes them.

use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config;
use crate::dirs::{Dirs, EnvVar, claude_dir, codex_dir};
use crate::fs_safe::{make_private_dir, write_private};
use crate::hooks_merge::{self, our_programs};
use crate::program::find_program;
use crate::spool::Source;

const BACKUP: &str = "gnomish-relay.bak";

/// The files of one agent that hold our hooks.
#[derive(Clone, Debug)]
pub struct HookFiles {
    pub source: Source,
    /// `settings.json` of Claude Code, or `hooks.json` of Codex.
    pub hooks: PathBuf,
    /// `config.toml` of Codex, which can turn all hooks off.
    pub codex_config: Option<PathBuf>,
}

impl HookFiles {
    pub fn claude(home: &Path, var: EnvVar) -> HookFiles {
        HookFiles::claude_in(&claude_dir(home, var))
    }

    fn claude_in(dir: &Path) -> HookFiles {
        HookFiles {
            source: Source::Claude,
            hooks: dir.join("settings.json"),
            codex_config: None,
        }
    }

    pub fn codex(home: &Path, var: EnvVar) -> HookFiles {
        HookFiles::codex_in(&codex_dir(home, var))
    }

    fn codex_in(dir: &Path) -> HookFiles {
        HookFiles {
            source: Source::Codex,
            hooks: dir.join("hooks.json"),
            codex_config: Some(dir.join("config.toml")),
        }
    }

    pub fn both(home: &Path, var: EnvVar) -> Vec<HookFiles> {
        vec![HookFiles::claude(home, var), HookFiles::codex(home, var)]
    }
}

/// The service of the bridge lacks the variables of a shell rc file, such as
/// `CLAUDE_CONFIG_DIR`. So each `hooks` command saves its folders for the bridge.
const SAVED_FOLDERS: &str = "hook-folders.json";

#[derive(Serialize, Deserialize)]
struct SavedFolders {
    claude: PathBuf,
    codex: PathBuf,
}

fn folder_of(files: &HookFiles) -> PathBuf {
    files
        .hooks
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

pub(crate) fn save_folders(data: &Path, all: &[HookFiles]) -> Result<()> {
    let folder = |source| all.iter().find(|f| f.source == source).map(folder_of);
    let (Some(claude), Some(codex)) = (folder(Source::Claude), folder(Source::Codex)) else {
        return Ok(());
    };
    make_private_dir(data)?;
    let text = serde_json::to_string(&SavedFolders { claude, codex })?;
    write_private(data, SAVED_FOLDERS, &text)
}

/// The folders of Claude Code and Codex that the last `hooks` command used.
pub fn saved_folders(data: &Path) -> Option<(PathBuf, PathBuf)> {
    let bytes = fs::read(data.join(SAVED_FOLDERS)).ok()?;
    let saved: SavedFolders = serde_json::from_slice(&bytes).ok()?;
    Some((saved.claude, saved.codex))
}

/// The files that the last `hooks` command used, else the ones of `var`.
pub fn files_for_bridge(home: &Path, data: &Path, var: EnvVar) -> Vec<HookFiles> {
    let Some((claude, codex)) = saved_folders(data) else {
        return HookFiles::both(home, var);
    };
    vec![HookFiles::claude_in(&claude), HookFiles::codex_in(&codex)]
}

pub fn agent_name(source: Source) -> &'static str {
    match source {
        Source::Claude => "Claude Code",
        Source::Codex => "Codex",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookState {
    On,
    Off,
    /// The program of a hook does not exist: the binary moved.
    Moved,
    /// Our hooks are there, but the agent runs no hooks.
    Disabled,
}

impl HookState {
    pub fn word(self) -> &'static str {
        match self {
            HookState::On => "on",
            HookState::Off => "off",
            HookState::Moved => "moved",
            HookState::Disabled => "disabled",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Changed,
    Unchanged,
}

struct Settings {
    /// The real file: a link from a dotfiles folder stays a link.
    real: PathBuf,
    value: Value,
    existed: bool,
}

fn real_path(path: &Path) -> Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(real) => Ok(real),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(path.to_owned()),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn read_settings(path: &Path) -> Result<Settings> {
    let real = real_path(path)?;
    let (value, existed) = match fs::read_to_string(&real) {
        Ok(text) => {
            let value = serde_json::from_str(&text)
                .with_context(|| format!("{} is not valid JSON", path.display()))?;
            (value, true)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => (Value::Object(Map::new()), false),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    Ok(Settings {
        real,
        value,
        existed,
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Mode 0600 from the first byte: the settings of Claude Code can hold an API key.
/// `create_new` fails at any name that exists, a link too, so it never follows a link.
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// The file from before the first install. A later install never writes over it.
fn backup(real: &Path) -> Result<()> {
    let backup = real.with_file_name(format!("{}.{BACKUP}", file_name(real)));
    match create_private(&backup) {
        Ok(mut file) => Ok(file.write_all(&fs::read(real)?)?),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e).with_context(|| format!("cannot write {}", backup.display())),
    }
}

/// An atomic rename in the folder of the real file, with the mode of the old file.
fn write_settings(settings: &Settings, value: &Value) -> Result<()> {
    let real = &settings.real;
    let dir = real.parent().context("the settings file has no folder")?;
    fs::create_dir_all(dir)?;
    if settings.existed {
        backup(real)?;
    }
    let tmp = dir.join(format!(".{}.gnomish-relay.tmp", file_name(real)));
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    // A crash can leave the temp file. Removing a link removes only the link.
    let _ = fs::remove_file(&tmp);
    create_private(&tmp)
        .with_context(|| format!("cannot write {}", tmp.display()))?
        .write_all(text.as_bytes())?;
    if settings.existed {
        fs::set_permissions(&tmp, fs::metadata(real)?.permissions())?;
    }
    fs::rename(&tmp, real).with_context(|| format!("cannot write {}", real.display()))
}

fn change(files: &HookFiles, merge: impl FnOnce(Value) -> Result<Value>) -> Result<Change> {
    let settings = read_settings(&files.hooks)?;
    let merged = merge(settings.value.clone())
        .with_context(|| format!("{} did not change", files.hooks.display()))?;
    if merged == settings.value {
        return Ok(Change::Unchanged);
    }
    write_settings(&settings, &merged)?;
    Ok(Change::Changed)
}

/// Codex runs no hooks when `config.toml` sets `hooks = false` (or the old name
/// `codex_hooks = false`) under `[features]`.
fn codex_hooks_off(config: &Path) -> bool {
    let Ok(text) = fs::read_to_string(config) else {
        return false;
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return false;
    };
    let features = table.get("features").and_then(toml::Value::as_table);
    let off = |key: &str| {
        features
            .and_then(|f| f.get(key))
            .and_then(toml::Value::as_bool)
            == Some(false)
    };
    off("hooks") || off("codex_hooks")
}

fn hooks_off(files: &HookFiles, settings: &Value) -> bool {
    match &files.codex_config {
        Some(config) => codex_hooks_off(config),
        None => settings.get("disableAllHooks") == Some(&Value::Bool(true)),
    }
}

/// Refuses when the user turned the hooks of Codex off: that is the choice of the user.
pub fn install(files: &HookFiles, program: &Path) -> Result<Change> {
    if let Some(config) = files.codex_config.as_deref().filter(|c| codex_hooks_off(c)) {
        bail!(
            "{} turns hooks off under [features]. Gnomish Relay leaves that choice to you.",
            config.display()
        );
    }
    change(files, |value| {
        hooks_merge::install(value, program, files.source)
    })
}

pub fn remove(files: &HookFiles) -> Result<Change> {
    change(files, |value| hooks_merge::remove(value, files.source))
}

pub fn state(files: &HookFiles) -> HookState {
    let Ok(settings) = read_settings(&files.hooks) else {
        return HookState::Off;
    };
    let programs = our_programs(&settings.value, files.source);
    if programs.is_empty() {
        HookState::Off
    } else if hooks_off(files, &settings.value) {
        HookState::Disabled
    } else if programs.iter().any(|p| !Path::new(p).is_file()) {
        HookState::Moved
    } else {
        HookState::On
    }
}

/// The agents that a flag names, or with no flag each agent on `PATH`.
fn chosen(flags: &[&str], all: Vec<HookFiles>, path: &OsStr) -> Result<Vec<HookFiles>> {
    let mut wanted = Vec::new();
    for flag in flags {
        match *flag {
            "--claude" => wanted.push(Source::Claude),
            "--codex" => wanted.push(Source::Codex),
            other => bail!("unknown option {other}. Use --claude or --codex"),
        }
    }
    if wanted.is_empty() {
        let on_path = |f: &HookFiles| {
            let name = hooks_merge::agent_word(f.source);
            find_program(name, path, cfg!(windows)).is_some()
        };
        return Ok(all.into_iter().filter(on_path).collect());
    }
    Ok(all
        .into_iter()
        .filter(|f| wanted.contains(&f.source))
        .collect())
}

pub fn install_command(
    flags: &[&str],
    all: Vec<HookFiles>,
    program: &Path,
    path: &OsStr,
    out: &mut dyn Write,
) -> Result<()> {
    let agents = chosen(flags, all, path)?;
    if agents.is_empty() {
        writeln!(
            out,
            "Can't find claude or codex on your PATH. To pick one, run gnomish-relay hooks install --claude or --codex"
        )?;
        return Ok(());
    }
    let mut changed = Vec::new();
    let mut failed = false;
    for files in &agents {
        let name = agent_name(files.source);
        match install(files, program) {
            Ok(Change::Changed) => {
                writeln!(
                    out,
                    "{name}: notifications on. Hooks added to {}",
                    files.hooks.display()
                )?;
                changed.push(files.source);
            }
            Ok(Change::Unchanged) => writeln!(out, "{name}: notifications already on")?,
            Err(e) => {
                writeln!(out, "{name}: can't turn on notifications: {e:#}")?;
                failed = true;
            }
        }
    }
    if !changed.is_empty() {
        let names: Vec<&str> = changed.iter().map(|s| agent_name(*s)).collect();
        // Both agents load hooks only at the start of a session.
        writeln!(
            out,
            "Restart any {} sessions that are open now.",
            names.join(" and ")
        )?;
    }
    if changed.contains(&Source::Codex) {
        writeln!(
            out,
            "Codex asks you to trust the new hooks when its next session starts. Trust them to get notifications."
        )?;
    }
    if !changed.is_empty() {
        // The game learns about an open session only at its next poll.
        writeln!(
            out,
            "The first notification can take up to 10 minutes. To check now, type /relay poll in the game."
        )?;
    }
    if failed {
        bail!("some notifications can't be turned on");
    }
    Ok(())
}

pub fn remove_command(flags: &[&str], all: Vec<HookFiles>, out: &mut dyn Write) -> Result<()> {
    let agents = if flags.is_empty() {
        all
    } else {
        chosen(flags, all, OsStr::new(""))?
    };
    for files in &agents {
        let name = agent_name(files.source);
        match remove(files)? {
            Change::Changed => writeln!(
                out,
                "{name}: notifications off. Hooks taken out of {}",
                files.hooks.display()
            )?,
            Change::Unchanged => writeln!(out, "{name}: notifications already off")?,
        }
    }
    Ok(())
}

fn state_line(files: &HookFiles) -> String {
    let name = agent_name(files.source);
    match state(files) {
        HookState::On => format!("{name}: notifications on"),
        HookState::Off => format!("{name}: notifications off"),
        HookState::Moved => format!(
            "{name}: notifications broken: the hooks point to a program that moved. Run gnomish-relay hooks install"
        ),
        HookState::Disabled if files.source == Source::Claude => format!(
            "{name}: notifications off: disableAllHooks is true in {}",
            files.hooks.display()
        ),
        HookState::Disabled => {
            format!("{name}: notifications off: its config.toml turns hooks off under [features]")
        }
    }
}

pub fn status_command(all: &[HookFiles], out: &mut dyn Write) -> Result<()> {
    for files in all {
        writeln!(out, "{}", state_line(files))?;
    }
    Ok(())
}

/// `gnomish-relay hooks <action> [flags]`.
pub fn command(dirs: &Dirs, action: &str, flags: &[&str], out: &mut dyn Write) -> Result<()> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let all = HookFiles::both(&dirs.home, &var);
    save_folders(&dirs.data, &all)?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    match action {
        "install" => install_command(flags, all, &std::env::current_exe()?, &path, out)?,
        "remove" => return remove_command(flags, all, out),
        "status" if flags.is_empty() => status_command(&all, out)?,
        _ => {
            bail!(
                "usage: gnomish-relay hooks install|remove [--claude] [--codex], or gnomish-relay hooks status"
            )
        }
    }
    if let Some(line) = relay_off_line(dirs) {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

const RELAY_OFF: &str =
    "The relay is off, so no notification comes. Run: gnomish-relay setup --relay";

/// Only the relay lane makes the spool folder, so with no relay a hook writes nothing.
/// A config that does not load is the job of `gnomish-relay status`.
fn relay_off_line(dirs: &Dirs) -> Option<&'static str> {
    let off = match config::load(&dirs.config, &dirs.home) {
        Ok(config) => config.relay.is_none(),
        Err(_) => !dirs.config.join(config::FILE).exists(),
    };
    off.then_some(RELAY_OFF)
}

/// The last line of `setup`, which changes no agent settings itself.
pub const SETUP_HINT: &str = "To get notified in WoW about Claude Code and Codex in a terminal, run gnomish-relay hooks install";

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::consts::EXE_SUFFIX;

    const USER_SETTINGS: &str = r#"{
  "model": "opus",
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "notify-send done"
          }
        ]
      }
    ]
  }
}
"#;

    struct Home {
        dir: tempfile::TempDir,
        program: PathBuf,
    }

    fn home() -> Home {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("bin").join("gnomish-relay");
        fs::create_dir_all(program.parent().unwrap()).unwrap();
        fs::write(&program, b"").unwrap();
        Home { dir, program }
    }

    fn no_env(_: &str) -> Option<PathBuf> {
        None
    }

    impl Home {
        fn claude(&self) -> HookFiles {
            HookFiles::claude(self.dir.path(), &no_env)
        }

        fn codex(&self) -> HookFiles {
            HookFiles::codex(self.dir.path(), &no_env)
        }

        fn write(&self, relative: &str, text: &str) -> PathBuf {
            let path = self.dir.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            path
        }
    }

    fn json_of(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn install_keeps_the_hooks_of_the_user_with_an_indent_of_two_spaces() {
        let home = home();
        let settings = home.write(".claude/settings.json", USER_SETTINGS);

        assert_eq!(
            install(&home.claude(), &home.program).unwrap(),
            Change::Changed
        );

        let text = fs::read_to_string(&settings).unwrap();
        assert!(
            text.starts_with("{\n  \"model\": \"opus\",\n  \"hooks\": {\n    \"Stop\": [\n"),
            "{text}"
        );
        let value = json_of(&settings);
        assert_eq!(
            value["hooks"]["Stop"][0]["hooks"][0]["command"],
            "notify-send done"
        );
        assert_eq!(our_programs(&value, Source::Claude).len(), 6);
        assert_eq!(state(&home.claude()), HookState::On);
    }

    #[test]
    fn a_second_install_writes_nothing() {
        let home = home();
        install(&home.claude(), &home.program).unwrap();
        assert_eq!(
            install(&home.claude(), &home.program).unwrap(),
            Change::Unchanged
        );
    }

    #[test]
    fn install_then_remove_gives_the_same_json_value_as_before() {
        let home = home();
        let settings = home.write(".claude/settings.json", USER_SETTINGS);
        install(&home.claude(), &home.program).unwrap();
        assert_eq!(remove(&home.claude()).unwrap(), Change::Changed);
        assert_eq!(fs::read_to_string(&settings).unwrap(), USER_SETTINGS);
        assert_eq!(state(&home.claude()), HookState::Off);
    }

    #[test]
    fn a_broken_file_changes_nothing_and_is_named() {
        let home = home();
        let settings = home.write(".claude/settings.json", "{\"model\": ");
        let error = install(&home.claude(), &home.program).unwrap_err();
        assert!(
            format!("{error:#}").contains("settings.json is not valid JSON"),
            "{error:#}"
        );
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{\"model\": ");
    }

    #[test]
    fn a_wrong_type_changes_nothing_and_names_the_key() {
        let home = home();
        let settings = home.write(".claude/settings.json", r#"{"hooks": {"Stop": 1}}"#);
        let error = install(&home.claude(), &home.program).unwrap_err();
        assert!(
            format!("{error:#}").contains("hooks.Stop is not a list"),
            "{error:#}"
        );
        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            r#"{"hooks": {"Stop": 1}}"#
        );
    }

    #[test]
    fn the_backup_is_the_file_from_before_the_first_install() {
        let home = home();
        let settings = home.write(".claude/settings.json", USER_SETTINGS);
        install(&home.claude(), &home.program).unwrap();
        remove(&home.claude()).unwrap();
        install(&home.claude(), &home.program).unwrap();
        let backup = settings.with_file_name("settings.json.gnomish-relay.bak");
        assert_eq!(fs::read_to_string(backup).unwrap(), USER_SETTINGS);
    }

    #[test]
    fn a_missing_file_is_made_with_no_backup() {
        let home = home();
        install(&home.codex(), &home.program).unwrap();
        let hooks = home.dir.path().join(".codex/hooks.json");
        assert_eq!(our_programs(&json_of(&hooks), Source::Codex).len(), 5);
        assert!(
            !hooks
                .with_file_name("hooks.json.gnomish-relay.bak")
                .exists()
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_follows_a_link_to_the_real_file_and_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let home = home();
        let real = home.write("dotfiles/claude-settings.json", USER_SETTINGS);
        fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
        fs::create_dir_all(home.dir.path().join(".claude")).unwrap();
        let link = home.dir.path().join(".claude/settings.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        install(&home.claude(), &home.program).unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(our_programs(&json_of(&real), Source::Claude).len(), 6);
        assert_eq!(
            fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_backup_and_a_new_settings_file_have_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let home = home();
        let settings = home.write(".claude/settings.json", USER_SETTINGS);
        fs::set_permissions(&settings, fs::Permissions::from_mode(0o600)).unwrap();

        install(&home.claude(), &home.program).unwrap();
        install(&home.codex(), &home.program).unwrap();

        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let backup = settings.with_file_name("settings.json.gnomish-relay.bak");
        assert_eq!(mode(&backup), 0o600);
        assert_eq!(mode(&home.dir.path().join(".codex/hooks.json")), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_at_the_temp_name_is_not_followed() {
        let home = home();
        let settings = home.write(".claude/settings.json", USER_SETTINGS);
        let victim = home.write("victim.txt", "keep me");
        let tmp = settings.with_file_name(".settings.json.gnomish-relay.tmp");
        std::os::unix::fs::symlink(&victim, &tmp).unwrap();

        install(&home.claude(), &home.program).unwrap();

        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep me");
        assert!(fs::symlink_metadata(&settings).unwrap().is_file());
        assert_eq!(our_programs(&json_of(&settings), Source::Claude).len(), 6);
    }

    #[test]
    fn codex_install_refuses_when_its_config_turns_hooks_off() {
        let home = home();
        for line in ["hooks = false", "codex_hooks = false"] {
            home.write(
                ".codex/config.toml",
                &format!("model = \"x\"\n[features]\n{line}\n"),
            );
            let error = install(&home.codex(), &home.program).unwrap_err();
            assert!(error.to_string().contains("turns hooks off"), "{error}");
        }
        assert!(!home.dir.path().join(".codex/hooks.json").exists());
    }

    #[test]
    fn codex_install_never_changes_its_config() {
        let home = home();
        let config = home.write(".codex/config.toml", "notify = [\"say\"]\n");
        install(&home.codex(), &home.program).unwrap();
        assert_eq!(fs::read_to_string(config).unwrap(), "notify = [\"say\"]\n");
        assert_eq!(state(&home.codex()), HookState::On);
    }

    #[test]
    fn the_state_shows_a_moved_binary_and_hooks_that_are_off() {
        let home = home();
        install(&home.claude(), &home.program).unwrap();
        install(&home.codex(), &home.program).unwrap();
        fs::remove_file(&home.program).unwrap();
        assert_eq!(state(&home.claude()), HookState::Moved);

        let settings = home.dir.path().join(".claude/settings.json");
        let mut value = json_of(&settings);
        value["disableAllHooks"] = Value::Bool(true);
        fs::write(&settings, value.to_string()).unwrap();
        assert_eq!(state(&home.claude()), HookState::Disabled);

        home.write(".codex/config.toml", "[features]\nhooks = false\n");
        assert_eq!(state(&home.codex()), HookState::Disabled);
    }

    fn dirs_with_config(home: &Home, config: Option<&str>) -> Dirs {
        let dirs = Dirs {
            home: home.dir.path().to_owned(),
            config: home.dir.path().join("config"),
            data: home.dir.path().join("data"),
        };
        if let Some(text) = config {
            crate::setup::write_config(&dirs.config, text, &dirs.home).unwrap();
        }
        dirs
    }

    #[test]
    fn with_no_relay_the_hooks_command_says_that_no_notification_comes() {
        let home = home();
        fs::create_dir_all(home.dir.path().join("Code")).unwrap();
        let timeways = "[wow]\npath = \"~/wow\"\n\n[story]\nmodel = \"claude\"\n";
        let relay = "allowed_roots = [\"~/Code\"]\ndefault_agent = \"echo\"\n\
            [wow]\npath = \"~/wow\"\n[agents.echo]\nkind = \"echo\"\npermission = \"ask\"\n";

        let no_config = relay_off_line(&dirs_with_config(&home, None));
        let only_timeways = relay_off_line(&dirs_with_config(&home, Some(timeways)));
        let with_relay = relay_off_line(&dirs_with_config(&home, Some(relay)));

        assert_eq!(no_config, Some(RELAY_OFF));
        assert_eq!(only_timeways, Some(RELAY_OFF));
        assert_eq!(with_relay, None);
    }

    #[test]
    fn the_bridge_reads_the_folders_that_the_last_hooks_command_used() {
        let home = home();
        let data = home.dir.path().join("data");
        let moved =
            |name: &str| (name == "CLAUDE_CONFIG_DIR").then(|| PathBuf::from("/cfg/claude"));
        save_folders(&data, &HookFiles::both(home.dir.path(), &moved)).unwrap();

        let files = files_for_bridge(home.dir.path(), &data, &no_env);

        assert_eq!(files[0].hooks, PathBuf::from("/cfg/claude/settings.json"));
        assert_eq!(files[1].hooks, home.dir.path().join(".codex/hooks.json"));
        assert_eq!(
            files[1].codex_config,
            Some(home.dir.path().join(".codex/config.toml"))
        );
    }

    #[test]
    fn with_no_saved_folders_the_bridge_takes_its_own_env() {
        let home = home();
        let data = home.dir.path().join("data");

        let files = files_for_bridge(home.dir.path(), &data, &no_env);

        assert_eq!(
            files[0].hooks,
            home.dir.path().join(".claude/settings.json")
        );
    }

    #[test]
    fn the_env_moves_the_folders_of_both_agents() {
        let env = |name: &str| match name {
            "CLAUDE_CONFIG_DIR" => Some(PathBuf::from("/c")),
            "CODEX_HOME" => Some(PathBuf::from("/x")),
            _ => None,
        };
        let home = Path::new("/home/u");
        assert_eq!(
            HookFiles::claude(home, &env).hooks,
            PathBuf::from("/c/settings.json")
        );
        assert_eq!(
            HookFiles::codex(home, &env).hooks,
            PathBuf::from("/x/hooks.json")
        );
        assert_eq!(
            HookFiles::claude(home, &no_env).hooks,
            PathBuf::from("/home/u/.claude/settings.json")
        );
    }

    fn output(run: impl FnOnce(&mut Vec<u8>) -> Result<()>) -> String {
        let mut out = Vec::new();
        run(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn install_with_no_flag_takes_each_agent_on_path_and_says_to_restart() {
        let home = home();
        let bin = home.dir.path().join("agents");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join(format!("codex{EXE_SUFFIX}")), b"").unwrap();
        let all = HookFiles::both(home.dir.path(), &no_env);

        let text = output(|out| install_command(&[], all, &home.program, bin.as_os_str(), out));

        assert!(text.contains("Codex: notifications on"), "{text}");
        assert!(!text.contains("Claude Code"), "{text}");
        assert!(text.contains("Restart any Codex sessions that are open now."));
        assert!(text.contains("trust the new hooks"));
        assert!(
            text.contains("To check now, type /relay poll in the game."),
            "{text}"
        );
    }

    #[test]
    fn install_with_no_agent_on_path_names_the_flag() {
        let home = home();
        let all = HookFiles::both(home.dir.path(), &no_env);
        let text = output(|out| install_command(&[], all, &home.program, OsStr::new(""), out));
        assert!(text.contains("hooks install --claude"), "{text}");
    }

    #[test]
    fn remove_and_status_speak_of_notifications() {
        let home = home();
        let all = HookFiles::both(home.dir.path(), &no_env);
        output(|out| {
            install_command(
                &["--claude"],
                all.clone(),
                &home.program,
                OsStr::new(""),
                out,
            )
        });

        let status = output(|out| status_command(&all, out));
        let removed = output(|out| remove_command(&[], all.clone(), out));

        assert_eq!(
            status,
            "Claude Code: notifications on\nCodex: notifications off\n"
        );
        assert!(
            removed.contains("Claude Code: notifications off. Hooks taken out of"),
            "{removed}"
        );
        assert!(
            removed.contains("Codex: notifications already off"),
            "{removed}"
        );
    }

    #[test]
    fn an_unknown_flag_is_an_error() {
        let home = home();
        let all = HookFiles::both(home.dir.path(), &no_env);
        let mut out = Vec::new();
        assert!(
            install_command(&["--gemini"], all, &home.program, OsStr::new(""), &mut out).is_err()
        );
    }
}
