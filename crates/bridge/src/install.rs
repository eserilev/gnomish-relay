//! `gnomish-relay setup` (SPEC.md 11.3): find the game, make the strip key, write the
//! key addons, and find the agents. The caller writes the config and the slots.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use protocol::apps::App;

use crate::app_files::{key_addon_name, key_global};
use crate::config::{Found, Kind};
use crate::fs_safe::{check_real_dir, make_private_dir, write_atomic_unsynced, write_private};
use crate::ids::random_hex;

pub const ADDON: &str = "GnomishRelay";
pub const TIMEWAYS: &str = "Timeways";
pub const KEY_FILE: &str = "Key.lua";
/// The `## Interface` of every addon that the desktop app writes: the Forever client.
pub const INTERFACE: &str = "16001";

const GAME: &str = "_classic_beta_";
const WOW: &str = "World of Warcraft";
const PRODUCT_DB: [&str; 4] = ["ProgramData", "Battle.net", "Agent", "product.db"];

fn join_all(base: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(base.to_owned(), |path, part| path.join(part))
}

/// A child whose name matches in any case. Some Linux guides make `Interface/Addons`.
fn child_any_case(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
        })
}

/// `Interface/AddOns` of a game folder, in the case that the disk has.
pub fn addons_dir(game: &Path) -> PathBuf {
    let interface = child_any_case(game, "Interface").unwrap_or_else(|| game.join("Interface"));
    child_any_case(&interface, "AddOns").unwrap_or_else(|| interface.join("AddOns"))
}

/// The `_classic_beta_` folder of a folder that the user gives: that folder, or the
/// one inside it. A dragged path comes with quotes.
pub fn game_folder(given: &str) -> PathBuf {
    let path = PathBuf::from(given.trim().trim_matches(|c| c == '"' || c == '\''));
    if path.file_name().is_some_and(|n| n == GAME) {
        return path;
    }
    let inner = path.join(GAME);
    if inner.is_dir() { inner } else { path }
}

/// The WoW install paths in a Battle.net `product.db`. The file is protobuf, and each
/// path is a string such as `C:/Program Files (x86)/World of Warcraft`.
pub fn product_paths(db: &[u8]) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for run in db.split(|b| !(b' '..=b'~').contains(b)) {
        let text = String::from_utf8_lossy(run);
        let Some(end) = text.rfind(WOW).map(|at| at + WOW.len()) else {
            continue;
        };
        // A length byte of protobuf can be printable, so a path starts at its drive
        // letter or at its first slash.
        let start = match text.find(":/").or_else(|| text.find(":\\")) {
            Some(colon) if colon > 0 && colon < end => colon - 1,
            _ => match text.find('/') {
                Some(slash) if slash < end => slash,
                _ => continue,
            },
        };
        let path = text[start..end].to_owned();
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// A Windows path from `product.db` inside a Wine prefix: `C:` is `drive_c`, and any
/// other drive is a link in `dosdevices`.
pub fn in_prefix(prefix: &Path, windows_path: &str) -> Option<PathBuf> {
    let (drive, rest) = windows_path.split_once(':')?;
    if drive.len() != 1 {
        return None;
    }
    let drive = drive.to_ascii_lowercase();
    let root = if drive == "c" {
        prefix.join("drive_c")
    } else {
        prefix.join("dosdevices").join(format!("{drive}:"))
    };
    let parts = rest.split(['/', '\\']).filter(|p| !p.is_empty());
    Some(parts.fold(root, |path, part| path.join(part)))
}

fn read_product_db(path: &Path) -> Vec<String> {
    fs::read(path)
        .map(|db| product_paths(&db))
        .unwrap_or_default()
}

fn children(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// Wine, Lutris, Bottles (also as a Flatpak), and Steam Proton prefixes.
fn wine_prefixes(home: &Path) -> Vec<PathBuf> {
    let mut prefixes = vec![home.join(".wine")];
    for parent in [
        "Games",
        ".local/share/wineprefixes",
        ".local/share/bottles/bottles",
        ".var/app/com.usebottles.bottles/data/bottles/bottles",
    ] {
        prefixes.extend(children(&home.join(parent)));
    }
    for steam in [
        ".steam/steam/steamapps/compatdata",
        ".local/share/Steam/steamapps/compatdata",
    ] {
        prefixes.extend(
            children(&home.join(steam))
                .into_iter()
                .map(|app| app.join("pfx")),
        );
    }
    prefixes
}

/// Every WoW Forever folder that setup can find: the default install places and
/// the paths in Battle.net's `product.db`.
pub fn find_games(home: &Path) -> Vec<PathBuf> {
    let mut installs: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(dir) = std::env::var_os(var) {
                installs.push(PathBuf::from(dir).join(WOW));
            }
        }
        if let Some(data) = std::env::var_os("ProgramData") {
            let db = join_all(&PathBuf::from(data), &PRODUCT_DB[1..]);
            installs.extend(read_product_db(&db).into_iter().map(PathBuf::from));
        }
    } else if cfg!(target_os = "macos") {
        installs.push(PathBuf::from("/Applications").join(WOW));
        let db = join_all(Path::new("/Users/Shared"), &PRODUCT_DB[1..]);
        installs.extend(read_product_db(&db).into_iter().map(PathBuf::from));
    } else {
        for prefix in wine_prefixes(home) {
            installs.push(join_all(&prefix, &["drive_c", "Program Files (x86)", WOW]));
            let db = join_all(&prefix, &["drive_c"]).join(join_all(Path::new(""), &PRODUCT_DB));
            let found = read_product_db(&db);
            installs.extend(found.iter().filter_map(|path| in_prefix(&prefix, path)));
        }
    }
    let mut games: Vec<PathBuf> = Vec::new();
    for game in installs.into_iter().map(|install| install.join(GAME)) {
        if game.is_dir() && !games.iter().any(|g| same_folder(g, &game)) {
            games.push(game);
        }
    }
    games
}

pub fn new_key() -> Result<String> {
    random_hex(32)
}

/// The old `Key.lua` inside the app addon: the key in the private table of the addon.
// TODO: remove when every Timeways release reads Timeways_Key.
pub fn key_lua(key_hex: &str) -> String {
    format!(
        "local _, ns = ...\nns.key = (\"{key_hex}\"):gsub(\"%x%x\", function(h)\n\
         \treturn string.char(tonumber(h, 16))\nend)\n"
    )
}

/// A key is 32 bytes in hex. Anything else never goes into a Lua file.
fn check_key_hex(key_hex: &str) -> Result<()> {
    if key_hex.len() != 64 || !key_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("the strip key is not 64 hex digits. Run gnomish-relay setup --new-key");
    }
    Ok(())
}

/// The `Key.lua` of a key addon. The app takes the global and clears it (SPEC.md 7.3.2).
pub fn key_addon_lua(app: App, key_hex: &str) -> Result<String> {
    check_key_hex(key_hex)?;
    Ok(format!("{} = \"{key_hex}\"\n", key_global(app)))
}

/// Load on demand, so the key loads only when its app asks for it (SPEC.md 7.3.2).
pub fn key_addon_toc(app: App) -> String {
    let title = match app {
        App::Relay => "Gnomish Relay",
        App::Timeways => "Timeways",
    };
    format!(
        "## Interface: {INTERFACE}\n## Title: |cff808080{title} key (leave on)|r\n\
         ## Notes: Made by the desktop app for this computer. Don't share it.\n\
         ## LoadOnDemand: 1\n\n{KEY_FILE}\n"
    )
}

fn same(path: &Path, content: &[u8]) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes == content)
}

/// A key that others can read gets written again with mode 0600, so a command of a game
/// run cannot read it (SPEC.md 6.6.4).
fn is_private_copy(path: &Path, key: &str) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return false;
        }
    }
    meta.is_file() && same(path, key.as_bytes())
}

/// What an install step changed.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Installed {
    /// The folder is new: WoW finds it only after a restart.
    New,
    /// Some files changed: a `/reload` loads them.
    Updated,
    Unchanged,
}

impl Installed {
    /// The change of two steps together: a new folder needs a restart, whatever the other did.
    #[must_use]
    pub fn and(self, other: Installed) -> Installed {
        match (self, other) {
            (Installed::New, _) | (_, Installed::New) => Installed::New,
            (Installed::Updated, _) | (_, Installed::Updated) => Installed::Updated,
            _ => Installed::Unchanged,
        }
    }

    fn from_change(new: bool, changed: bool) -> Installed {
        match (new, changed) {
            (true, _) => Installed::New,
            (false, true) => Installed::Updated,
            (false, false) => Installed::Unchanged,
        }
    }
}

/// Deletes `Key.lua` in the real folder of `dir`. Returns true when it was there.
pub fn remove_old_key_file(dir: &Path) -> Result<bool> {
    let Ok(real) = dir.canonicalize() else {
        return Ok(false);
    };
    let file = real.join(KEY_FILE);
    match fs::remove_file(&file) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("cannot delete {}", file.display())),
    }
}

/// Writes the key addon of `app` next to the app addon (SPEC.md 7.3.2). Its folder and
/// its key file are private: the key signs the strips.
pub fn write_key_addon(addons: &Path, app: App, key_hex: &str) -> Result<Installed> {
    let dir = addons.join(key_addon_name(app));
    let new = fs::symlink_metadata(&dir).is_err();
    if new {
        make_private_dir(&dir)?;
    }
    check_real_dir(&dir)?;
    let key = key_addon_lua(app, key_hex)?;
    let toc_name = format!("{}.toc", key_addon_name(app));
    let toc = key_addon_toc(app);
    let mut changed = false;
    if !same(&dir.join(&toc_name), toc.as_bytes()) {
        write_atomic_unsynced(&dir, &toc_name, toc.as_bytes())?;
        changed = true;
    }
    if !is_private_copy(&dir.join(KEY_FILE), &key) {
        write_private(&dir, KEY_FILE, &key)?;
        changed = true;
    }
    Ok(Installed::from_change(new, changed))
}

/// The relay key addon, and the old `Key.lua` of the relay addon gone. The desktop app
/// writes no other file of the relay addon: players get it from `CurseForge` (SPEC.md 11.3).
pub fn write_relay_keys(addons: &Path, key_hex: &str) -> Result<Installed> {
    let removed = match relay_dir(addons) {
        Some(dir) => remove_old_key_file(&dir)?,
        None => false,
    };
    let key = write_key_addon(addons, App::Relay, key_hex)?;
    Ok(key.and(Installed::from_change(false, removed)))
}

/// Whether the TOC of the addon in `dir` still loads `Key.lua` from its own folder.
fn toc_lists_key_file(dir: &Path, toc_name: &str) -> bool {
    fs::read_to_string(dir.join(toc_name))
        .is_ok_and(|toc| toc.lines().any(|line| line.trim() == KEY_FILE))
}

/// The Timeways key addon, and the old `Key.lua` in the Timeways folder for as long as
/// the installed Timeways TOC loads it. Timeways owns every other file of its folder
/// (SPEC.md 9.7, decision 15).
pub fn write_timeways_keys(addons: &Path, timeways: &Path, key_hex: &str) -> Result<Installed> {
    let key_addon = write_key_addon(addons, App::Timeways, key_hex)?;
    let real = timeways
        .canonicalize()
        .with_context(|| format!("{} is missing or a broken link", timeways.display()))?;
    let old = if toc_lists_key_file(&real, &format!("{TIMEWAYS}.toc")) {
        write_key_file(&real, key_hex)?
    } else {
        Installed::from_change(false, remove_old_key_file(&real)?)
    };
    Ok(key_addon.and(old))
}

/// Writes only `Key.lua`, into the real folder of `dir`: a link stays a link.
// TODO: remove when every Timeways release reads Timeways_Key.
pub fn write_key_file(dir: &Path, key_hex: &str) -> Result<Installed> {
    let real = dir
        .canonicalize()
        .with_context(|| format!("{} is missing or a broken link", dir.display()))?;
    let key = key_lua(key_hex);
    if is_private_copy(&real.join(KEY_FILE), &key) {
        return Ok(Installed::Unchanged);
    }
    write_private(&real, KEY_FILE, &key)?;
    Ok(Installed::Updated)
}

/// The folder of the relay addon, in any case, also through a link of a developer (SPEC.md 16).
pub fn relay_dir(addons: &Path) -> Option<PathBuf> {
    child_any_case(addons, ADDON).filter(|dir| dir.is_dir())
}

/// The folder of the Timeways addon, in any case. Setup never makes it: only a player
/// who installed Timeways gets its key and slots.
pub fn timeways_dir(addons: &Path) -> Option<PathBuf> {
    child_any_case(addons, TIMEWAYS).filter(|dir| dir.is_dir())
}

/// The agents that setup knows: the config entry name, the kind, and the command. Each
/// ACP command comes from the official ACP registry (SPEC.md 9.2).
pub const KNOWN_AGENTS: [Found<'static>; 13] = [
    ("claude", Kind::Claude, &["claude"]),
    ("codex", Kind::Codex, &["codex"]),
    ("gemini", Kind::Acp, &["gemini", "--acp"]),
    ("qwen", Kind::Acp, &["qwen", "--acp"]),
    ("opencode", Kind::Acp, &["opencode", "acp"]),
    ("goose", Kind::Acp, &["goose", "acp"]),
    ("copilot", Kind::Acp, &["copilot", "--acp"]),
    ("cursor", Kind::Acp, &["cursor-agent", "acp"]),
    ("kimi", Kind::Acp, &["kimi", "acp"]),
    ("auggie", Kind::Acp, &["auggie", "--acp"]),
    ("cline", Kind::Acp, &["cline", "--acp"]),
    ("kilo", Kind::Acp, &["kilo", "acp"]),
    ("vibe", Kind::Acp, &["vibe-acp"]),
];

/// The presets of harnesses with no ACP mode. Setup offers each one that is on `PATH`, and
/// adds it only when the player says yes: its commands never ask (SPEC.md 11.3).
pub const HARNESSES_WITH_NO_ACP: [&str; 2] = ["aider", "llm"];

pub fn find_harnesses(path: &OsStr) -> Vec<&'static str> {
    HARNESSES_WITH_NO_ACP
        .iter()
        .copied()
        .filter(|name| on_path(name, path))
        .collect()
}

fn on_path(program: &str, path: &OsStr) -> bool {
    crate::program::find_program(program, path, cfg!(windows)).is_some()
}

/// The command that logs in each known agent, for the message of setup.
pub fn login_command(agent: &str) -> Option<&'static str> {
    match agent {
        "claude" => Some("claude"),
        "codex" => Some("codex login"),
        "gemini" => Some("gemini"),
        "qwen" => Some("qwen"),
        _ => None,
    }
}

/// ACP agents answer `session/new` with an "auth required" error when nobody is logged
/// in. The native backends say "needs a login".
pub fn needs_login(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("auth") || error.contains("login") || error.contains("log in")
}

/// The usual folders of code projects that hold at least one git repository. On
/// Windows and macOS, `code` and `Code` are one folder, so it is named once.
pub fn suggest_roots(home: &Path) -> Vec<PathBuf> {
    const NAMES: [&str; 9] = [
        "Documents/Code",
        "code",
        "Code",
        "src",
        "dev",
        "projects",
        "Projects",
        "repos",
        "workspace",
    ];
    NAMES
        .iter()
        .map(|name| home.join(name))
        .filter(|dir| {
            fs::read_dir(dir)
                .is_ok_and(|entries| entries.flatten().any(|e| e.path().join(".git").exists()))
        })
        .fold(Vec::new(), |mut found: Vec<PathBuf>, dir| {
            if !found.iter().any(|f| same_folder(f, &dir)) {
                found.push(dir);
            }
            found
        })
}

#[cfg(unix)]
fn same_folder(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// `canonicalize` on Windows gives the name as the disk stores it.
#[cfg(not(unix))]
fn same_folder(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// The known agents whose program is on `path`, in the order of `KNOWN_AGENTS`.
pub fn find_agents(path: &OsStr) -> Vec<Found<'static>> {
    KNOWN_AGENTS
        .iter()
        .copied()
        .filter(|(_, _, command)| on_path(command[0], path))
        .collect()
}

/// The folders of the bridge move with these. A shell rc file sets them for setup, but
/// not for the service, so the unit carries them.
pub const XDG_VARS: [&str; 2] = ["XDG_CONFIG_HOME", "XDG_DATA_HOME"];

/// A systemd user service. It gets the `PATH` of setup, because a service starts with
/// almost none, and then finds no agent. `xdg` holds the set ones of `XDG_VARS`.
pub fn systemd_unit(exe: &Path, path_var: &str, xdg: &[(&str, String)]) -> String {
    let quote = |text: &str| format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""));
    let mut vars = vec![format!("PATH={path_var}")];
    vars.extend(xdg.iter().map(|(name, value)| format!("{name}={value}")));
    let lines: Vec<String> = vars
        .iter()
        .map(|var| format!("Environment={}\n", quote(var)))
        .collect();
    let env = lines.concat();
    format!(
        "[Unit]\nDescription=Gnomish Relay bridge\n\n[Service]\nExecStart={} run\n\
         {env}Restart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
        quote(&exe.to_string_lossy()),
    )
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub const LAUNCHD_LABEL: &str = "dev.gnomish-relay.bridge";

/// A launchd agent that starts at login and again after a crash.
pub fn launchd_plist(exe: &Path, path_var: &str, log: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\n\
         <key>Label</key><string>{LAUNCHD_LABEL}</string>\n\
         <key>ProgramArguments</key><array><string>{}</string><string>run</string></array>\n\
         <key>EnvironmentVariables</key><dict><key>PATH</key><string>{}</string></dict>\n\
         <key>StandardOutPath</key><string>{log}</string>\n<key>StandardErrorPath</key><string>{log}</string>\n\
         <key>RunAtLoad</key><true/>\n<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n\
         </dict></plist>\n",
        xml(&exe.to_string_lossy()),
        xml(path_var),
        log = xml(&log.to_string_lossy()),
    )
}

pub const SYSTEMD_UNIT: &str = "gnomish-relay.service";

/// The user manager of systemd starts with no `XDG_CONFIG_HOME` of a shell rc file, so
/// it reads units here, not under the `XDG_CONFIG_HOME` of the shell.
pub fn systemd_dir(home: &Path) -> PathBuf {
    home.join(".config").join("systemd").join("user")
}

/// The login service file of setup on this OS, if there is one.
pub fn service_file(home: &Path) -> Option<PathBuf> {
    let file = if cfg!(target_os = "linux") {
        systemd_dir(home).join(SYSTEMD_UNIT)
    } else if cfg!(target_os = "macos") {
        home.join("Library/LaunchAgents")
            .join(format!("{LAUNCHD_LABEL}.plist"))
    } else {
        return None;
    };
    file.is_file().then_some(file)
}

/// The `PATH` that `systemd_unit` or `launchd_plist` wrote. The service finds its agents
/// only on this `PATH`, not on the one of the shell.
pub fn service_path_var(text: &str) -> Option<String> {
    if let Some(at) = text.find("Environment=\"PATH=") {
        return Some(systemd_unquoted(&text[at + "Environment=\"PATH=".len()..]));
    }
    let start = text.find("<key>PATH</key><string>")? + "<key>PATH</key><string>".len();
    let end = text[start..].find("</string>")?;
    Some(xml_text(&text[start..start + end]))
}

/// The text up to the closing quote, with the escapes of `systemd_unit` undone.
fn systemd_unquoted(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    out
}

fn xml_text(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    fn game_in(prefix: &Path) -> PathBuf {
        let game = join_all(prefix, &["drive_c", "Program Files (x86)", WOW]).join(GAME);
        fs::create_dir_all(game.join("Interface/AddOns")).unwrap();
        game
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_game_is_found_in_a_lutris_prefix() {
        let home = tempfile::tempdir().unwrap();
        let game = game_in(&home.path().join("Games/battlenet"));
        fs::create_dir_all(home.path().join("Games/other/drive_c")).unwrap();
        assert_eq!(find_games(home.path()), [game]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn product_db_finds_a_game_on_another_drive_of_a_bottle() {
        let home = tempfile::tempdir().unwrap();
        let bottle = home.path().join(".local/share/bottles/bottles/wow");
        let game = bottle.join("dosdevices/d:/Games/World of Warcraft/_classic_beta_");
        fs::create_dir_all(&game).unwrap();
        let db_dir = bottle.join("drive_c/ProgramData/Battle.net/Agent");
        fs::create_dir_all(&db_dir).unwrap();
        fs::write(
            db_dir.join("product.db"),
            b"\n\x05wow_classic\x12)D:/Games/World of Warcraft\x1a\x02enUS",
        )
        .unwrap();
        assert_eq!(find_games(home.path()), [game]);
    }

    #[test]
    fn product_paths_start_at_the_drive_and_end_at_the_game_name() {
        let db = b"\x0a\x03wow\x12'C:/Program Files (x86)/World of Warcraft2\x04enUS\x12\x1f/Applications/World of Warcraft";
        assert_eq!(
            product_paths(db),
            [
                "C:/Program Files (x86)/World of Warcraft",
                "/Applications/World of Warcraft"
            ]
        );
    }

    // On Windows, `e:` is a drive and replaces the whole path. Wine runs only on Unix.
    #[cfg(unix)]
    #[test]
    fn a_windows_path_maps_into_a_wine_prefix() {
        let prefix = Path::new("/p");
        assert_eq!(
            in_prefix(prefix, "C:/A/B"),
            Some(PathBuf::from("/p/drive_c/A/B"))
        );
        assert_eq!(
            in_prefix(prefix, "e:\\G"),
            Some(PathBuf::from("/p/dosdevices/e:/G"))
        );
        assert_eq!(in_prefix(prefix, "/Applications/x"), None);
    }

    #[test]
    fn the_addons_folder_is_found_in_any_case_and_a_given_folder_can_be_the_parent() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("World of Warcraft").join(GAME);
        fs::create_dir_all(game.join("interface/Addons")).unwrap();
        let found = addons_dir(&game);
        assert!(
            same_folder(&found, &game.join("interface/Addons")),
            "{found:?}"
        );
        let given = format!("\"{}\"", root.path().join("World of Warcraft").display());
        assert_eq!(game_folder(&given), game);
    }

    #[test]
    fn a_new_key_is_64_hex_digits_and_never_the_same() {
        let (a, b) = (new_key().unwrap(), new_key().unwrap());
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn the_key_addon_holds_the_key_in_the_global_of_its_app() {
        let addons = tempfile::tempdir().unwrap();
        let key = "ab".repeat(32);

        assert_eq!(
            write_key_addon(addons.path(), App::Relay, &key).unwrap(),
            Installed::New
        );

        let dir = addons.path().join("GnomishRelay_Key");
        assert_eq!(
            fs::read_to_string(dir.join(KEY_FILE)).unwrap(),
            format!("GnomishRelayKey = \"{key}\"\n")
        );
        let toc = fs::read_to_string(dir.join("GnomishRelay_Key.toc")).unwrap();
        assert!(toc.contains("## LoadOnDemand: 1\n"), "{toc}");
        assert!(!toc.contains("Dependencies"), "{toc}");
        assert!(toc.ends_with("\nKey.lua\n"), "{toc}");
    }

    #[test]
    fn the_key_addon_has_the_interface_of_the_addon() {
        let addon_toc = include_str!("../../../addon/GnomishRelay/GnomishRelay.toc");
        let interface = format!("## Interface: {INTERFACE}\n");
        assert!(addon_toc.starts_with(&interface));
        assert!(key_addon_toc(App::Timeways).starts_with(&interface));
    }

    #[test]
    fn the_key_addon_is_written_once_and_again_when_its_key_is_missing() {
        let addons = tempfile::tempdir().unwrap();
        let key = "cd".repeat(32);
        write_key_addon(addons.path(), App::Timeways, &key).unwrap();

        let again = write_key_addon(addons.path(), App::Timeways, &key).unwrap();
        fs::remove_file(addons.path().join("Timeways_Key").join(KEY_FILE)).unwrap();
        let repaired = write_key_addon(addons.path(), App::Timeways, &key).unwrap();

        assert_eq!(again, Installed::Unchanged);
        assert_eq!(repaired, Installed::Updated);
    }

    #[test]
    fn a_key_that_is_not_64_hex_digits_never_goes_into_lua() {
        let addons = tempfile::tempdir().unwrap();
        let quote = format!("{}\"", "a".repeat(63));

        assert!(write_key_addon(addons.path(), App::Relay, &quote).is_err());
        assert!(key_addon_lua(App::Relay, "abcd").is_err());
        assert!(
            !addons
                .path()
                .join("GnomishRelay_Key")
                .join(KEY_FILE)
                .exists()
        );
    }

    /// A command of a game run must not read the strip key (SPEC.md 6.6.4).
    #[cfg(unix)]
    #[test]
    fn the_key_addon_is_private_also_after_an_older_write() {
        use std::os::unix::fs::PermissionsExt;
        let addons = tempfile::tempdir().unwrap();
        let dir = addons.path().join("GnomishRelay_Key");
        write_key_addon(addons.path(), App::Relay, &"ab".repeat(32)).unwrap();
        let key = dir.join(KEY_FILE);
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();

        write_key_addon(addons.path(), App::Relay, &"ab".repeat(32)).unwrap();

        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&key), 0o600);
        assert_eq!(mode(&dir), 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn a_key_addon_folder_that_is_a_link_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = root.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let addons = root.path().join("AddOns");
        fs::create_dir(&addons).unwrap();
        std::os::unix::fs::symlink(&elsewhere, addons.join("GnomishRelay_Key")).unwrap();

        assert!(write_key_addon(&addons, App::Relay, &"ab".repeat(32)).is_err());

        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
    }

    fn files_in(dir: &Path) -> Vec<(std::ffi::OsString, Vec<u8>)> {
        let mut files: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| (e.file_name(), fs::read(e.path()).unwrap()))
            .collect();
        files.sort();
        files
    }

    #[test]
    fn the_relay_keys_never_write_a_file_of_the_relay_addon() {
        let addons = tempfile::tempdir().unwrap();
        let dir = addons.path().join(ADDON);
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join("GnomishRelay.toc"),
            "## Version: 0.0.1\n\nCore.lua\n",
        )
        .unwrap();
        fs::write(dir.join("Core.lua"), "-- from CurseForge").unwrap();
        let before = files_in(&dir);

        let written = write_relay_keys(addons.path(), &"ab".repeat(32)).unwrap();

        assert_eq!(written, Installed::New, "the key addon is new");
        assert_eq!(files_in(&dir), before);
    }

    #[test]
    fn the_relay_keys_never_make_the_relay_addon_folder() {
        let addons = tempfile::tempdir().unwrap();

        write_relay_keys(addons.path(), &"ab".repeat(32)).unwrap();

        assert!(!addons.path().join(ADDON).exists());
        assert!(
            addons
                .path()
                .join("GnomishRelay_Key")
                .join(KEY_FILE)
                .is_file()
        );
    }

    #[test]
    fn the_relay_keys_delete_the_key_file_of_an_older_setup() {
        let addons = tempfile::tempdir().unwrap();
        let key = "ab".repeat(32);
        write_relay_keys(addons.path(), &key).unwrap();
        let dir = addons.path().join(ADDON);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join(KEY_FILE), key_lua(&key)).unwrap();

        assert_eq!(
            write_relay_keys(addons.path(), &key).unwrap(),
            Installed::Updated
        );

        assert!(!dir.join(KEY_FILE).exists());
        assert!(dir.is_dir(), "the folder stays");
    }

    #[test]
    fn the_relay_folder_is_found_in_any_case_and_only_when_it_exists() {
        let addons = tempfile::tempdir().unwrap();
        assert_eq!(relay_dir(addons.path()), None);
        fs::create_dir(addons.path().join("gnomishrelay")).unwrap();
        let found = relay_dir(addons.path()).unwrap();
        assert!(same_folder(&found, &addons.path().join("gnomishrelay")));
    }

    /// The key addon is a real folder in `AddOns`, never in the repository (SPEC.md 16).
    #[cfg(unix)]
    #[test]
    fn a_linked_repository_gets_no_file_and_loses_an_old_key() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        fs::write(repo.join(KEY_FILE), "old key").unwrap();
        fs::write(repo.join("Core.lua"), "-- mine").unwrap();
        let addons = root.path().join("AddOns");
        fs::create_dir(&addons).unwrap();
        std::os::unix::fs::symlink(&repo, addons.join(ADDON)).unwrap();

        let key = "ab".repeat(32);

        assert_eq!(write_relay_keys(&addons, &key).unwrap(), Installed::New);
        assert_eq!(
            write_relay_keys(&addons, &key).unwrap(),
            Installed::Unchanged
        );

        let names: Vec<_> = fs::read_dir(&repo)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, ["Core.lua"]);
        let link = fs::symlink_metadata(addons.join(ADDON)).unwrap();
        assert!(link.file_type().is_symlink());
    }

    #[test]
    fn a_timeways_toc_that_loads_key_lua_keeps_getting_it() {
        let addons = tempfile::tempdir().unwrap();
        let timeways = addons.path().join(TIMEWAYS);
        fs::create_dir(&timeways).unwrap();
        let toc = "## Title: Timeways\n\nKey.lua\nCore.lua\n";
        fs::write(timeways.join("Timeways.toc"), toc).unwrap();
        let key = "cd".repeat(32);

        let written = write_timeways_keys(addons.path(), &timeways, &key).unwrap();

        assert_eq!(written, Installed::New);
        assert_eq!(
            fs::read_to_string(timeways.join(KEY_FILE)).unwrap(),
            key_lua(&key)
        );
        assert!(addons.path().join("Timeways_Key").join(KEY_FILE).is_file());
    }

    #[test]
    fn a_timeways_toc_that_reads_the_key_addon_loses_its_old_key_file() {
        let addons = tempfile::tempdir().unwrap();
        let timeways = addons.path().join(TIMEWAYS);
        fs::create_dir(&timeways).unwrap();
        let toc = "## Title: Timeways\n\nKeyHandoff.lua\n";
        fs::write(timeways.join("Timeways.toc"), toc).unwrap();
        fs::write(timeways.join(KEY_FILE), "old").unwrap();

        write_timeways_keys(addons.path(), &timeways, &"cd".repeat(32)).unwrap();

        assert!(!timeways.join(KEY_FILE).exists());
        assert!(timeways.join("Timeways.toc").is_file());
    }

    #[test]
    fn a_new_folder_wins_over_an_update_and_an_update_over_no_change() {
        use Installed::{New, Unchanged, Updated};
        assert_eq!(Updated.and(New), New);
        assert_eq!(Unchanged.and(Updated), Updated);
        assert_eq!(Unchanged.and(Unchanged), Unchanged);
    }

    #[test]
    fn the_timeways_folder_is_found_in_any_case_and_only_when_it_exists() {
        let addons = tempfile::tempdir().unwrap();
        assert_eq!(timeways_dir(addons.path()), None);
        fs::create_dir(addons.path().join("timeways")).unwrap();
        let found = timeways_dir(addons.path()).unwrap();
        assert!(same_folder(&found, &addons.path().join("timeways")));
    }

    #[test]
    fn the_key_file_goes_into_a_folder_alone_and_again_when_it_is_missing() {
        let addons = tempfile::tempdir().unwrap();
        let dir = addons.path().join(TIMEWAYS);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("Core.lua"), "-- Timeways").unwrap();
        let key = "cd".repeat(32);

        assert_eq!(write_key_file(&dir, &key).unwrap(), Installed::Updated);
        assert_eq!(write_key_file(&dir, &key).unwrap(), Installed::Unchanged);
        fs::remove_file(dir.join(KEY_FILE)).unwrap();
        assert_eq!(write_key_file(&dir, &key).unwrap(), Installed::Updated);

        assert_eq!(
            fs::read_to_string(dir.join(KEY_FILE)).unwrap(),
            key_lua(&key)
        );
        assert_eq!(
            fs::read_to_string(dir.join("Core.lua")).unwrap(),
            "-- Timeways"
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
    }

    #[test]
    fn only_folders_with_a_git_repository_are_suggested() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join("code/lighthouse/.git")).unwrap();
        fs::create_dir_all(home.path().join("src/notes")).unwrap();
        assert_eq!(suggest_roots(home.path()), [home.path().join("code")]);
    }

    #[test]
    fn agents_are_found_on_the_path_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let name = |p: &str| {
            if cfg!(windows) {
                format!("{p}.exe")
            } else {
                p.to_owned()
            }
        };
        fs::write(dir.path().join(name("gemini")), "").unwrap();
        fs::write(dir.path().join(name("claude")), "").unwrap();
        fs::write(dir.path().join(name("opencode")), "").unwrap();
        let found: Vec<(&str, Kind)> = find_agents(dir.path().as_os_str())
            .iter()
            .map(|(n, kind, _)| (*n, *kind))
            .collect();
        assert_eq!(
            found,
            [
                ("claude", Kind::Claude),
                ("gemini", Kind::Acp),
                ("opencode", Kind::Acp)
            ]
        );
    }

    #[test]
    fn only_harnesses_with_no_acp_mode_are_offered() {
        let dir = tempfile::tempdir().unwrap();
        for program in ["aider", "gemini", "llm"] {
            let file = if cfg!(windows) {
                format!("{program}.exe")
            } else {
                program.to_owned()
            };
            fs::write(dir.path().join(file), "").unwrap();
        }

        assert_eq!(find_harnesses(dir.path().as_os_str()), ["aider", "llm"]);
    }

    #[test]
    fn every_known_agent_has_a_valid_name_and_a_command() {
        for name in HARNESSES_WITH_NO_ACP {
            let preset = crate::harness_presets::find(name).unwrap();
            assert_eq!(preset.program, name);
            assert!(KNOWN_AGENTS.iter().all(|(known, _, _)| *known != name));
        }
        for (name, _, command) in KNOWN_AGENTS {
            assert!(protocol::record::is_valid_id(name.as_bytes()), "{name}");
            assert!(!command[0].is_empty(), "{name}");
        }
    }

    #[test]
    fn the_systemd_unit_runs_the_bridge_with_the_path_of_setup() {
        let unit = systemd_unit(
            Path::new("/opt/my relay/gnomish-relay"),
            "/usr/bin:/home/x/.npm/bin",
            &[],
        );
        assert!(unit.contains("ExecStart=\"/opt/my relay/gnomish-relay\" run\n"));
        assert!(unit.contains("Environment=\"PATH=/usr/bin:/home/x/.npm/bin\"\n"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn the_systemd_unit_gets_the_xdg_folders_of_setup_when_they_are_set() {
        let exe = Path::new("/opt/gnomish-relay");
        let xdg = [
            ("XDG_CONFIG_HOME", "/home/x/cfg".to_owned()),
            ("XDG_DATA_HOME", "/home/x/my data".to_owned()),
        ];

        let unit = systemd_unit(exe, "/usr/bin", &xdg);

        assert!(unit.contains("Environment=\"XDG_CONFIG_HOME=/home/x/cfg\"\n"));
        assert!(unit.contains("Environment=\"XDG_DATA_HOME=/home/x/my data\"\n"));
        assert_eq!(service_path_var(&unit).as_deref(), Some("/usr/bin"));
        assert!(!systemd_unit(exe, "/usr/bin", &[]).contains("XDG"));
    }

    #[test]
    fn the_path_of_the_service_comes_back_from_the_unit_and_the_plist() {
        let path = "/usr/bin:/home/x/my \"odd\" \\dir:/a&b<c>";
        let exe = Path::new("/opt/gnomish-relay");

        let from_unit = service_path_var(&systemd_unit(exe, path, &[]));
        let from_plist = service_path_var(&launchd_plist(exe, path, Path::new("/l.log")));

        assert_eq!(from_unit.as_deref(), Some(path));
        assert_eq!(from_plist.as_deref(), Some(path));
        assert_eq!(service_path_var("[Service]\nExecStart=x run\n"), None);
    }

    #[test]
    fn the_launchd_plist_escapes_the_paths() {
        let plist = launchd_plist(
            Path::new("/Apps/R&D/gnomish-relay"),
            "/bin:<x>",
            Path::new("/L/r.log"),
        );
        assert!(plist.contains("<key>StandardErrorPath</key><string>/L/r.log</string>"));
        assert!(plist.contains("<string>/Apps/R&amp;D/gnomish-relay</string><string>run</string>"));
        assert!(plist.contains("<string>/bin:&lt;x&gt;</string>"));
    }

    #[test]
    fn an_auth_error_means_a_login() {
        assert!(needs_login(
            "The agent failed at session/new: Authentication required"
        ));
        assert!(!needs_login("Cannot start gemini: not found on PATH"));
        assert!(needs_login("Claude Code needs a login."));
        assert_eq!(login_command("claude"), Some("claude"));
        assert_eq!(login_command("codex"), Some("codex login"));
    }
}
