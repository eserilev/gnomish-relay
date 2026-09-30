//! `gnomish-relay setup` (SPEC.md 11.3): find the game, make the strip key, install
//! the addon, and find the agents. The caller writes the config and the slots.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{Found, Kind};
use crate::fs_safe::{write_atomic_unsynced, write_private};
use crate::ids::random_hex;

pub const ADDON: &str = "GnomishRelay";
pub const TIMEWAYS: &str = "Timeways";
pub const KEY_FILE: &str = "Key.lua";

/// The addon, built into the program, so one download installs everything. The files of
/// `addon/transport` are shared with other apps (SPEC.md 9.7, decision 14). They go into
/// this addon here, so the repo never holds a copy of them.
pub const ADDON_FILES: [(&str, &[u8]); 30] = [
    (
        "GnomishRelay.toc",
        include_bytes!("../../../addon/GnomishRelay/GnomishRelay.toc"),
    ),
    (
        "App.lua",
        include_bytes!("../../../addon/GnomishRelay/App.lua"),
    ),
    (
        "Sha256.lua",
        include_bytes!("../../../addon/transport/Sha256.lua"),
    ),
    (
        "Codec.lua",
        include_bytes!("../../../addon/transport/Codec.lua"),
    ),
    (
        "Saved.lua",
        include_bytes!("../../../addon/transport/Saved.lua"),
    ),
    (
        "Store.lua",
        include_bytes!("../../../addon/GnomishRelay/Store.lua"),
    ),
    (
        "Health.lua",
        include_bytes!("../../../addon/transport/Health.lua"),
    ),
    (
        "Strip.lua",
        include_bytes!("../../../addon/transport/Strip.lua"),
    ),
    (
        "Slots.lua",
        include_bytes!("../../../addon/transport/Slots.lua"),
    ),
    (
        "Messages.lua",
        include_bytes!("../../../addon/transport/Messages.lua"),
    ),
    (
        "Transport.lua",
        include_bytes!("../../../addon/GnomishRelay/Transport.lua"),
    ),
    (
        "Notices.lua",
        include_bytes!("../../../addon/GnomishRelay/Notices.lua"),
    ),
    (
        "Blocks.lua",
        include_bytes!("../../../addon/GnomishRelay/Blocks.lua"),
    ),
    (
        "QuickActions.lua",
        include_bytes!("../../../addon/GnomishRelay/QuickActions.lua"),
    ),
    (
        "QuickBar.lua",
        include_bytes!("../../../addon/GnomishRelay/QuickBar.lua"),
    ),
    (
        "QuickEditor.lua",
        include_bytes!("../../../addon/GnomishRelay/QuickEditor.lua"),
    ),
    (
        "Transcript.lua",
        include_bytes!("../../../addon/GnomishRelay/Transcript.lua"),
    ),
    (
        "Folders.lua",
        include_bytes!("../../../addon/GnomishRelay/Folders.lua"),
    ),
    (
        "Browser.lua",
        include_bytes!("../../../addon/GnomishRelay/Browser.lua"),
    ),
    (
        "BridgeSettings.lua",
        include_bytes!("../../../addon/GnomishRelay/BridgeSettings.lua"),
    ),
    (
        "RulesGroup.lua",
        include_bytes!("../../../addon/GnomishRelay/RulesGroup.lua"),
    ),
    (
        "SettingsTab.lua",
        include_bytes!("../../../addon/GnomishRelay/SettingsTab.lua"),
    ),
    (
        "DiagTab.lua",
        include_bytes!("../../../addon/GnomishRelay/DiagTab.lua"),
    ),
    (
        "Window.lua",
        include_bytes!("../../../addon/GnomishRelay/Window.lua"),
    ),
    (
        "Popup.lua",
        include_bytes!("../../../addon/GnomishRelay/Popup.lua"),
    ),
    (
        "NoticeFrames.lua",
        include_bytes!("../../../addon/GnomishRelay/NoticeFrames.lua"),
    ),
    (
        "Core.lua",
        include_bytes!("../../../addon/GnomishRelay/Core.lua"),
    ),
    // The game reads it from the folder by itself: the key binding of the window.
    (
        "Bindings.xml",
        include_bytes!("../../../addon/GnomishRelay/Bindings.xml"),
    ),
    // The mono font of code boxes, under the SIL Open Font License.
    (
        "JetBrainsMono-Regular.ttf",
        include_bytes!("../../../addon/GnomishRelay/JetBrainsMono-Regular.ttf"),
    ),
    (
        "JetBrainsMono-OFL.txt",
        include_bytes!("../../../addon/GnomishRelay/JetBrainsMono-OFL.txt"),
    ),
];

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

/// The key in the private table of the addon. An addon that loads first can still
/// replace the string functions that this code calls, and read the key (SPEC.md 6.5).
pub fn key_lua(key_hex: &str) -> String {
    format!(
        "local _, ns = ...\nns.key = (\"{key_hex}\"):gsub(\"%x%x\", function(h)\n\
         \treturn string.char(tonumber(h, 16))\nend)\n"
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

/// What `install_addon` changed.
#[derive(Debug, PartialEq, Eq)]
pub enum Installed {
    /// The folder is new: WoW finds it only after a restart.
    New,
    /// Some files changed: a `/reload` loads them.
    Updated,
    Unchanged,
}

/// Writes the addon and its key into `addons/GnomishRelay`. A folder that is a link
/// is a developer checkout (SPEC.md 16): only `Key.lua` goes into its real folder.
pub fn install_addon(addons: &Path, key_hex: &str) -> Result<Installed> {
    let dir = addons.join(ADDON);
    let key = key_lua(key_hex);
    let meta = fs::symlink_metadata(&dir);
    if meta.as_ref().is_ok_and(|m| m.file_type().is_symlink()) {
        return write_key_file(&dir, key_hex);
    }
    let new = meta.is_err();
    fs::create_dir_all(&dir).with_context(|| format!("cannot make {}", dir.display()))?;
    let mut changed = false;
    for (name, content) in ADDON_FILES.iter().copied() {
        if !same(&dir.join(name), content) {
            write_atomic_unsynced(&dir, name, content)?;
            changed = true;
        }
    }
    if !is_private_copy(&dir.join(KEY_FILE), &key) {
        write_private(&dir, KEY_FILE, &key)?;
        changed = true;
    }
    Ok(match (new, changed) {
        (true, _) => Installed::New,
        (false, true) => Installed::Updated,
        (false, false) => Installed::Unchanged,
    })
}

/// Writes only `Key.lua`, into the real folder of `dir`: a link stays a link. The
/// Timeways addon owns every other file of its folder (SPEC.md 9.7, decision 15).
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
    fn every_file_of_the_toc_is_built_in() {
        let toc = std::str::from_utf8(ADDON_FILES[0].1).unwrap();
        let is_lua = |name: &&str| Path::new(name).extension().is_some_and(|e| e == "lua");
        let listed: Vec<&str> = toc
            .lines()
            .filter(|l| is_lua(l) && *l != KEY_FILE)
            .collect();
        let built: Vec<&str> = ADDON_FILES[1..]
            .iter()
            .map(|(name, _)| *name)
            .filter(is_lua)
            .collect();
        assert_eq!(listed, built);
    }

    /// The self-test addon is for developers only (SPEC.md 14.3). The name is split, so
    /// this test does not find itself.
    #[test]
    fn the_self_test_addon_is_never_built_in() {
        let self_test = ["GnomishRelay", "SelfTest"].concat();
        assert!(!include_str!("install.rs").contains(&self_test));
        let toc = std::str::from_utf8(ADDON_FILES[0].1).unwrap();
        assert!(!toc.contains(&self_test));
    }

    #[test]
    fn a_new_key_is_64_hex_digits_and_never_the_same() {
        let (a, b) = (new_key().unwrap(), new_key().unwrap());
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn install_writes_the_addon_once_and_repairs_a_missing_key() {
        let addons = tempfile::tempdir().unwrap();
        let key = "ab".repeat(32);
        assert_eq!(install_addon(addons.path(), &key).unwrap(), Installed::New);
        assert_eq!(
            install_addon(addons.path(), &key).unwrap(),
            Installed::Unchanged
        );
        fs::remove_file(addons.path().join(ADDON).join(KEY_FILE)).unwrap();
        assert_eq!(
            install_addon(addons.path(), &key).unwrap(),
            Installed::Updated
        );
        let written = fs::read_to_string(addons.path().join(ADDON).join(KEY_FILE)).unwrap();
        assert_eq!(written, key_lua(&key));
    }

    /// A command of a game run must not read the strip key (SPEC.md 6.6.4).
    #[cfg(unix)]
    #[test]
    fn the_key_file_has_mode_0600_also_after_an_older_install() {
        use std::os::unix::fs::PermissionsExt;
        let addons = tempfile::tempdir().unwrap();
        let key = addons.path().join(ADDON).join(KEY_FILE);
        install_addon(addons.path(), &"ab".repeat(32)).unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();

        install_addon(addons.path(), &"ab".repeat(32)).unwrap();

        let mode = fs::metadata(&key).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn install_writes_the_mono_font_with_its_license() {
        let addons = tempfile::tempdir().unwrap();
        install_addon(addons.path(), &"ab".repeat(32)).unwrap();
        let dir = addons.path().join(ADDON);
        let font = fs::read(dir.join("JetBrainsMono-Regular.ttf")).unwrap();
        // A TrueType file starts with the version 1.0 tag.
        assert_eq!(font[..4], [0, 1, 0, 0]);
        let license = fs::read_to_string(dir.join("JetBrainsMono-OFL.txt")).unwrap();
        assert!(license.contains("SIL Open Font License"));
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_checkout_gets_only_the_key() {
        let root = tempfile::tempdir().unwrap();
        let checkout = root.path().join("checkout");
        fs::create_dir(&checkout).unwrap();
        let addons = root.path().join("AddOns");
        fs::create_dir(&addons).unwrap();
        std::os::unix::fs::symlink(&checkout, addons.join(ADDON)).unwrap();

        assert_eq!(
            install_addon(&addons, &"cd".repeat(32)).unwrap(),
            Installed::Updated
        );
        let names: Vec<_> = fs::read_dir(&checkout)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, [KEY_FILE]);
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
