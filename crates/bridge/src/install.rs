//! `gnomish-relay setup` (SPEC.md 11.3): find the game, make the strip key, install
//! the addon, and find the agents. The caller writes the config and the slots.

use std::ffi::OsStr;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::fs_safe::write_atomic_unsynced;

pub const ADDON: &str = "GnomishRelay";
const KEY_FILE: &str = "Key.lua";

/// The addon, built into the program, so one download installs everything.
pub const ADDON_FILES: [(&str, &str); 10] = [
    (
        "GnomishRelay.toc",
        include_str!("../../../addon/GnomishRelay/GnomishRelay.toc"),
    ),
    (
        "Sha256.lua",
        include_str!("../../../addon/GnomishRelay/Sha256.lua"),
    ),
    (
        "Codec.lua",
        include_str!("../../../addon/GnomishRelay/Codec.lua"),
    ),
    (
        "Store.lua",
        include_str!("../../../addon/GnomishRelay/Store.lua"),
    ),
    (
        "Health.lua",
        include_str!("../../../addon/GnomishRelay/Health.lua"),
    ),
    (
        "Strip.lua",
        include_str!("../../../addon/GnomishRelay/Strip.lua"),
    ),
    (
        "Transport.lua",
        include_str!("../../../addon/GnomishRelay/Transport.lua"),
    ),
    (
        "Window.lua",
        include_str!("../../../addon/GnomishRelay/Window.lua"),
    ),
    (
        "Popup.lua",
        include_str!("../../../addon/GnomishRelay/Popup.lua"),
    ),
    (
        "Core.lua",
        include_str!("../../../addon/GnomishRelay/Core.lua"),
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
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(|e| anyhow::anyhow!("no random bytes from the OS: {e}"))?;
    Ok(key.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    }))
}

/// The key in the private table of the addon. No other addon can read it.
pub fn key_lua(key_hex: &str) -> String {
    format!(
        "local _, ns = ...\nns.key = (\"{key_hex}\"):gsub(\"%x%x\", function(h)\n\
         \treturn string.char(tonumber(h, 16))\nend)\n"
    )
}

fn same(path: &Path, text: &str) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes == text.as_bytes())
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
        let real = dir
            .canonicalize()
            .with_context(|| format!("{} is a broken link", dir.display()))?;
        if same(&real.join(KEY_FILE), &key) {
            return Ok(Installed::Unchanged);
        }
        write_atomic_unsynced(&real, KEY_FILE, key.as_bytes())?;
        return Ok(Installed::Updated);
    }
    let new = meta.is_err();
    fs::create_dir_all(&dir).with_context(|| format!("cannot make {}", dir.display()))?;
    let mut changed = false;
    for (name, text) in ADDON_FILES
        .iter()
        .copied()
        .chain([(KEY_FILE, key.as_str())])
    {
        if !same(&dir.join(name), text) {
            write_atomic_unsynced(&dir, name, text.as_bytes())?;
            changed = true;
        }
    }
    Ok(match (new, changed) {
        (true, _) => Installed::New,
        (false, true) => Installed::Updated,
        (false, false) => Installed::Unchanged,
    })
}

/// The ACP agents that setup knows, with their config entry name and command.
pub const KNOWN_AGENTS: [(&str, &[&str]); 3] = [
    ("claude", &["claude-agent-acp"]),
    ("codex", &["codex-acp"]),
    ("gemini", &["gemini", "--acp"]),
];

fn on_path(program: &str, path: &OsStr) -> bool {
    crate::program::find_program(program, path, cfg!(windows)).is_some()
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
pub fn find_agents(path: &OsStr) -> Vec<(&'static str, &'static [&'static str])> {
    KNOWN_AGENTS
        .iter()
        .copied()
        .filter(|(_, command)| on_path(command[0], path))
        .collect()
}

/// A systemd user service. It gets the `PATH` of setup, because a service starts with
/// almost none, and then finds no agent.
pub fn systemd_unit(exe: &Path, path_var: &str) -> String {
    let quote = |text: &str| format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""));
    format!(
        "[Unit]\nDescription=Gnomish Relay bridge\n\n[Service]\nExecStart={} run\n\
         Environment={}\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
        quote(&exe.to_string_lossy()),
        quote(&format!("PATH={path_var}")),
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
        assert_eq!(addons_dir(&game), game.join("interface/Addons"));
        let given = format!("\"{}\"", root.path().join("World of Warcraft").display());
        assert_eq!(game_folder(&given), game);
    }

    #[test]
    fn every_file_of_the_toc_is_built_in() {
        let toc = ADDON_FILES[0].1;
        let listed: Vec<&str> = toc
            .lines()
            .filter(|l| Path::new(l).extension().is_some_and(|e| e == "lua") && *l != KEY_FILE)
            .collect();
        let built: Vec<&str> = ADDON_FILES[1..].iter().map(|(name, _)| *name).collect();
        assert_eq!(listed, built);
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
        fs::write(dir.path().join(name("claude-agent-acp")), "").unwrap();
        let found: Vec<&str> = find_agents(dir.path().as_os_str())
            .iter()
            .map(|(n, _)| *n)
            .collect();
        assert_eq!(found, ["claude", "gemini"]);
    }

    #[test]
    fn the_systemd_unit_runs_the_bridge_with_the_path_of_setup() {
        let unit = systemd_unit(
            Path::new("/opt/my relay/gnomish-relay"),
            "/usr/bin:/home/x/.npm/bin",
        );
        assert!(unit.contains("ExecStart=\"/opt/my relay/gnomish-relay\" run\n"));
        assert!(unit.contains("Environment=\"PATH=/usr/bin:/home/x/.npm/bin\"\n"));
        assert!(unit.contains("WantedBy=default.target"));
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
}
