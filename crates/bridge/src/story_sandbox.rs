//! The sandbox of the story program (SPEC.md 6.6.4 and 9.7, decision 9): bubblewrap on
//! Linux, Seatbelt on macOS, and none on Windows or on a Linux with no working `bwrap`.

use std::ffi::OsString;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::action_input::DESKTOP_PATHS;
use crate::process;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sandbox {
    /// bubblewrap, at this path.
    Bwrap(PathBuf),
    /// `sandbox-exec` with a generated profile.
    Seatbelt,
    None,
}

impl Sandbox {
    pub fn name(&self) -> &'static str {
        match self {
            Sandbox::Bwrap(_) => "bwrap",
            Sandbox::Seatbelt => "sandbox-exec",
            Sandbox::None => "no sandbox",
        }
    }
}

/// The one folder that the story program writes, the paths that it cannot read, and
/// the files that it reads even inside a hidden path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Walls {
    pub folder: PathBuf,
    /// Each one exists and has no link in it.
    pub hidden: Vec<PathBuf>,
    /// Read-only, for example the lore pack. Each one exists and has no link in it.
    pub readable: Vec<PathBuf>,
}

/// `folder` sits inside `data`, which the sandbox hides as a whole. The config folder
/// holds both keys, and the data folder holds the replay stores and the desktop approvals.
pub fn walls(folder: &Path, config: &Path, data: &Path, home: &Path, readable: &[&Path]) -> Walls {
    let desktop = DESKTOP_PATHS
        .iter()
        .filter(|pattern| !pattern.contains('*'))
        .map(|pattern| home.join(pattern));
    let mut hidden: Vec<PathBuf> = [config.to_owned(), data.to_owned()]
        .into_iter()
        .chain(desktop)
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    hidden.sort();
    hidden.dedup();
    Walls {
        folder: folder.canonicalize().unwrap_or_else(|_| folder.to_owned()),
        hidden,
        readable: readable
            .iter()
            .filter_map(|path| path.canonicalize().ok())
            .collect(),
    }
}

/// The program and its arguments that start `program` inside `sandbox`.
pub fn command_line(
    sandbox: &Sandbox,
    walls: &Walls,
    program: &Path,
    args: &[String],
) -> (PathBuf, Vec<OsString>) {
    let mut inner: Vec<OsString> = vec![program.into()];
    inner.extend(args.iter().map(OsString::from));
    match sandbox {
        Sandbox::Bwrap(bwrap) => (bwrap.clone(), [bwrap_args(walls), inner].concat()),
        Sandbox::Seatbelt => (
            PathBuf::from(SANDBOX_EXEC),
            [seatbelt_args(walls), inner].concat(),
        ),
        Sandbox::None => {
            let program = inner.remove(0);
            (PathBuf::from(program), inner)
        }
    }
}

/// A read-only system, private `/tmp` and `/run` (they hold the sockets of the ssh agent
/// and the desktop), and no network. `--die-with-parent` stops it when the bridge dies.
fn bwrap_args(walls: &Walls) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--tmpfs",
        "/tmp",
        "--tmpfs",
        "/var/tmp",
        "--tmpfs",
        "/run",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    for path in &walls.hidden {
        args.extend(hide(path));
    }
    let folder = walls.folder.as_os_str();
    args.extend(["--bind".into(), folder.into(), folder.into()]);
    for path in &walls.readable {
        args.extend(["--ro-bind".into(), path.into(), path.into()]);
    }
    // Only after the binds: each needs a mount point inside a hidden folder.
    for path in walls.hidden.iter().filter(|p| p.is_dir()) {
        args.extend(["--remount-ro".into(), path.into()]);
    }
    args.extend(["--chdir".into(), folder.into()]);
    args.extend(
        ["--unshare-all", "--die-with-parent", "--new-session", "--"]
            .iter()
            .map(OsString::from),
    );
    args
}

/// An empty folder over a folder, and an empty file over a file.
fn hide(path: &Path) -> Vec<OsString> {
    if path.is_dir() {
        return vec!["--tmpfs".into(), path.into()];
    }
    vec!["--ro-bind".into(), "/dev/null".into(), path.into()]
}

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The paths go in as parameters, never into the profile text, so a quote in a path
/// cannot change a rule.
fn seatbelt_args(walls: &Walls) -> Vec<OsString> {
    let profile = seatbelt_profile(walls.hidden.len(), walls.readable.len());
    let mut args: Vec<OsString> = vec!["-p".into(), profile.into()];
    args.push("-D".into());
    args.push(parameter("STORY", &walls.folder));
    for (n, path) in walls.hidden.iter().enumerate() {
        args.push("-D".into());
        args.push(parameter(&format!("HIDE{n}"), path));
    }
    for (n, path) in walls.readable.iter().enumerate() {
        args.push("-D".into());
        args.push(parameter(&format!("READ{n}"), path));
    }
    args.push("--".into());
    args
}

fn parameter(name: &str, path: &Path) -> OsString {
    let mut text = OsString::from(format!("{name}="));
    text.push(path);
    text
}

/// A later rule wins over an earlier one in Seatbelt, so the allows come last.
pub fn seatbelt_profile(hidden: usize, readable: usize) -> String {
    let mut profile = String::from(
        "(version 1)\n(allow default)\n(deny network*)\n(deny file-write*)\n\
         (allow file-write* (literal \"/dev/null\"))\n",
    );
    for n in 0..hidden {
        let _ = writeln!(
            profile,
            "(deny file-read* file-write* (subpath (param \"HIDE{n}\")))"
        );
    }
    for n in 0..readable {
        let _ = writeln!(profile, "(allow file-read* (literal (param \"READ{n}\")))");
    }
    profile.push_str("(allow file-read* file-write* (subpath (param \"STORY\")))\n");
    profile
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// The sandbox of this computer. A `bwrap` that cannot make its namespaces counts as
/// none: some systems block them for normal users.
pub fn detect() -> Sandbox {
    if cfg!(target_os = "macos") && Path::new(SANDBOX_EXEC).is_file() {
        return Sandbox::Seatbelt;
    }
    if !cfg!(target_os = "linux") {
        return Sandbox::None;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(bwrap) = crate::program::find_program("bwrap", &path, false) else {
        return Sandbox::None;
    };
    if bwrap_works(&bwrap) {
        Sandbox::Bwrap(bwrap)
    } else {
        Sandbox::None
    }
}

/// Runs `bwrap --version` inside a sandbox of the same kind as the story program's.
fn bwrap_works(bwrap: &Path) -> bool {
    let bwrap = bwrap.to_string_lossy().into_owned();
    let args: Vec<String> = [
        "--ro-bind",
        "/",
        "/",
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--",
        &bwrap,
        "--version",
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    process::output(std::slice::from_ref(&bwrap), &args, &[], "/", PROBE_TIMEOUT)
        .is_ok_and(|out| out.success && out.stdout.starts_with("bubblewrap"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Walls {
        Walls {
            folder: PathBuf::from("/data/timeways/story"),
            hidden: vec![PathBuf::from("/config"), PathBuf::from("/data")],
            readable: vec![PathBuf::from("/data/lore.sqlite")],
        }
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn position(args: &[String], pair: &[&str]) -> usize {
        args.windows(pair.len())
            .position(|w| w == pair)
            .unwrap_or_else(|| panic!("no {pair:?} in {args:?}"))
    }

    #[test]
    fn with_no_sandbox_the_program_starts_as_it_is() {
        let (program, args) = command_line(
            &Sandbox::None,
            &sample(),
            Path::new("/opt/story"),
            &["echo".into()],
        );
        assert_eq!(program, PathBuf::from("/opt/story"));
        assert_eq!(strings(&args), ["echo"]);
    }

    #[test]
    fn bwrap_hides_the_folders_then_binds_the_story_folder_and_the_pack_then_makes_them_read_only()
    {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data").display().to_string();
        let story = root.path().join("data/story").display().to_string();
        let pack = root.path().join("data/lore.sqlite").display().to_string();
        let walls = Walls {
            folder: PathBuf::from(&story),
            hidden: vec![PathBuf::from(&data)],
            readable: vec![PathBuf::from(&pack)],
        };
        std::fs::create_dir_all(&story).unwrap();

        let (program, args) = command_line(
            &Sandbox::Bwrap(PathBuf::from("/usr/bin/bwrap")),
            &walls,
            Path::new("/opt/story"),
            &["echo".into()],
        );

        let args = strings(&args);
        assert_eq!(program, PathBuf::from("/usr/bin/bwrap"));
        assert_eq!(args[..3], ["--ro-bind", "/", "/"]);
        let hidden = position(&args, &["--tmpfs", &data]);
        let bind = position(&args, &["--bind", &story, &story]);
        let pack = position(&args, &["--ro-bind", &pack, &pack]);
        let read_only = position(&args, &["--remount-ro", &data]);
        assert!(hidden < bind && bind < pack && pack < read_only);
        for flag in ["--unshare-all", "--die-with-parent", "--new-session"] {
            assert!(args.contains(&flag.to_owned()), "{flag}");
        }
        position(&args, &["--chdir", &story]);
        assert_eq!(args[args.len() - 3..], ["--", "/opt/story", "echo"]);
    }

    #[test]
    fn bwrap_hides_a_folder_with_an_empty_folder_and_a_file_with_an_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".netrc");
        std::fs::write(&file, "machine x password y").unwrap();
        let folder = hide(dir.path());
        assert_eq!(folder[0], "--tmpfs");
        assert_eq!(folder[1], dir.path().as_os_str());
        let hidden = hide(&file);
        assert_eq!(strings(&hidden)[..2], ["--ro-bind", "/dev/null"]);
        assert_eq!(hidden[2], file.as_os_str());
    }

    #[test]
    fn seatbelt_takes_the_paths_as_parameters_and_the_story_folder_last() {
        let (program, args) =
            command_line(&Sandbox::Seatbelt, &sample(), Path::new("/opt/story"), &[]);
        let args = strings(&args);
        assert_eq!(program, PathBuf::from(SANDBOX_EXEC));
        assert_eq!(args[0], "-p");
        let profile = &args[1];
        assert!(profile.contains("(deny network*)"));
        assert!(profile.contains("(subpath (param \"HIDE1\"))"));
        assert!(profile.contains("(allow file-read* (literal (param \"READ0\")))"));
        assert!(!profile.contains("/data"), "no path in the profile text");
        assert!(profile.trim_end().ends_with("(subpath (param \"STORY\")))"));
        position(&args, &["-D", "STORY=/data/timeways/story"]);
        position(&args, &["-D", "HIDE0=/config"]);
        position(&args, &["-D", "READ0=/data/lore.sqlite"]);
        assert_eq!(args[args.len() - 2..], ["--", "/opt/story"]);
    }

    #[test]
    fn the_walls_hide_the_folders_and_the_credentials_that_exist() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".config/gnomish-relay");
        let data = home.path().join(".local/share/gnomish-relay");
        let folder = data.join("timeways/story");
        for dir in [&config, &folder, &home.path().join(".ssh")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(home.path().join(".netrc"), "").unwrap();
        let pack = home.path().join("lore.sqlite");
        std::fs::write(&pack, "").unwrap();

        let walls = walls(&folder, &config, &data, home.path(), &[&pack]);

        let real = |p: &Path| p.canonicalize().unwrap();
        assert_eq!(walls.folder, real(&folder));
        assert_eq!(walls.readable, [real(&pack)]);
        for path in [
            &config,
            &data,
            &home.path().join(".ssh"),
            &home.path().join(".netrc"),
        ] {
            assert!(walls.hidden.contains(&real(path)), "{}", path.display());
        }
        assert!(
            !walls.hidden.iter().any(|p| p.ends_with(".aws")),
            "missing paths are left out"
        );
    }

    #[test]
    fn each_sandbox_has_a_name_for_the_log() {
        assert_eq!(Sandbox::Bwrap(PathBuf::new()).name(), "bwrap");
        assert_eq!(Sandbox::Seatbelt.name(), "sandbox-exec");
        assert_eq!(Sandbox::None.name(), "no sandbox");
    }
}
