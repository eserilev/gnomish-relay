//! Setup and update install the programs of a Timeways release and build the lore pack
//! (SPEC.md 11.4). A fake release and a fake dump sit in temp folders, so no test
//! needs the network. The fake programs are shell scripts.
#![cfg(unix)]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use bridge::config;
use bridge::config_text::timeways_config;
use bridge::dirs::Dirs;
use bridge::ids::hex;
use bridge::lore_pack;
use bridge::setup::write_config;
use bridge::timeways_install::{self, Lore, Places, Sources};
use bridge::timeways_release::{MANIFEST, SUMS};
use sha2::{Digest, Sha256};

const PACK_SCRIPT: &str = "#!/bin/sh\n\
    [ \"$1\" = from-dump ] || exit 2\n\
    [ -e \"$3\" ] && exit 3\n\
    grep -q wowpedia \"$2\" || exit 4\n\
    echo 'page Thrall'\n\
    echo 'read 1 pages, skipped 0'\n\
    echo \"wrote 325 passages to $3\"\n\
    printf 'lore' > \"$3\"\n";

fn file_url(dir: &Path) -> String {
    format!("file://{}", dir.display())
}

fn write_program(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn sum(path: &Path) -> String {
    hex(&Sha256::digest(fs::read(path).unwrap()))
}

/// A release folder as the release job of Timeways makes it, for this computer.
struct Release {
    dir: tempfile::TempDir,
    asset: String,
}

impl Release {
    fn new(story: &str, app_version: u32) -> Release {
        let dir = tempfile::tempdir().unwrap();
        let build = tempfile::tempdir().unwrap();
        write_program(&build.path().join("timeways-story"), story);
        write_program(&build.path().join("timeways-pack"), PACK_SCRIPT);
        fs::write(build.path().join("LICENSE"), "MIT").unwrap();
        let target = bridge::update::target().unwrap();
        let asset = format!("timeways-{target}.tar.gz");
        let archive = dir.path().join(&asset);
        let made = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(build.path())
            .args(["timeways-story", "timeways-pack", "LICENSE"])
            .status()
            .unwrap();
        assert!(made.success());
        let sha = sum(&archive);
        fs::write(dir.path().join(SUMS), format!("{sha}  {asset}\n")).unwrap();
        let manifest = format!(
            r#"{{"version": "0.1.0", "tag": "v0.1.0", "app_version": {app_version},
            "targets": {{"{target}": {{"asset": "{asset}", "sha256": "{sha}",
            "programs": ["timeways-story", "timeways-pack"]}}}},
            "addon": {{"asset": "timeways-addon.zip", "sha256": "{sha}"}}}}"#
        );
        fs::write(dir.path().join(MANIFEST), manifest).unwrap();
        Release { dir, asset }
    }

    fn url(&self) -> String {
        file_url(self.dir.path())
    }
}

/// A home with a Timeways config, and a dump in a folder of its own.
struct Computer {
    root: tempfile::TempDir,
    dirs: Dirs,
}

impl Computer {
    fn new() -> Computer {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let dirs = Dirs {
            config: home.join(".config/gnomish-relay"),
            data: home.join(".local/share/gnomish-relay"),
            home,
        };
        fs::create_dir_all(&dirs.data).unwrap();
        let text = timeways_config(&dirs.home.join("wow"), &[]);
        write_config(&dirs.config, &text, &dirs.home).unwrap();
        fs::create_dir(root.path().join("dump")).unwrap();
        Computer { root, dirs }
    }

    fn dump(&self, text: &str) -> String {
        let dir = self.root.path().join("dump");
        fs::write(dir.join(lore_pack::DUMP_FILE), text).unwrap();
        format!("{}/{}", file_url(&dir), lore_pack::DUMP_FILE)
    }

    fn places(&self) -> Places {
        Places::of(&self.dirs, None)
    }

    fn config_text(&self) -> String {
        fs::read_to_string(self.dirs.config.join(config::FILE)).unwrap()
    }

    fn install(&self, release: &Release, dump: &str) -> anyhow::Result<timeways_install::Report> {
        let sources = Sources {
            release: release.url(),
            dump: dump.to_owned(),
        };
        timeways_install::install(&self.dirs, &sources, &self.places(), |_| {})
    }
}

fn story_program(computer: &Computer) -> PathBuf {
    computer.places().bin.join("timeways-story")
}

#[test]
fn setup_installs_both_programs_builds_the_pack_and_sets_the_config() {
    let computer = Computer::new();
    let release = Release::new("#!/bin/sh\necho story\n", 1);
    let dump = computer.dump("wowpedia dump");

    let report = computer.install(&release, &dump).unwrap();

    assert_eq!(report.version, "0.1.0");
    assert_eq!(report.changed, ["timeways-story", "timeways-pack"]);
    assert_eq!(
        report.lore,
        Lore::Built(vec![
            "read 1 pages, skipped 0".into(),
            format!(
                "wrote 325 passages to {}.new",
                computer.places().pack.display()
            ),
        ])
    );
    let places = computer.places();
    assert_eq!(places.bin, computer.dirs.home.join(".local/bin"));
    assert!(places.bin.join("timeways-pack").is_file());
    assert_eq!(fs::read_to_string(&places.pack).unwrap(), "lore");
    assert_eq!(
        places.pack,
        computer.dirs.home.join(".local/share/timeways/lore.sqlite")
    );
    let text = computer.config_text();
    assert!(
        text.contains("program = \"~/.local/bin/timeways-story\"\n"),
        "{text}"
    );
    assert!(
        text.contains("lore_pack = \"~/.local/share/timeways/lore.sqlite\"\n"),
        "{text}"
    );
    let story = config::load(&computer.dirs.config, &computer.dirs.home)
        .unwrap()
        .story
        .unwrap();
    assert_eq!(story.program.unwrap().program, story_program(&computer));
}

#[test]
fn the_dump_and_the_downloads_are_gone_after_the_install() {
    let computer = Computer::new();
    let release = Release::new("#!/bin/sh\n", 1);

    computer
        .install(&release, &computer.dump("wowpedia"))
        .unwrap();

    assert!(!computer.places().work.exists());
    assert!(computer.places().work.starts_with(&computer.dirs.data));
}

#[test]
fn an_archive_with_a_wrong_sum_is_refused_and_nothing_is_installed() {
    let computer = Computer::new();
    let release = Release::new("#!/bin/sh\n", 1);
    fs::write(release.dir.path().join(&release.asset), b"changed").unwrap();
    let before = computer.config_text();

    let error = computer
        .install(&release, &computer.dump("wowpedia"))
        .unwrap_err();

    assert!(format!("{error:#}").contains("wrong SHA-256"), "{error:#}");
    assert!(!story_program(&computer).exists());
    assert_eq!(computer.config_text(), before);
}

#[test]
fn an_archive_that_sha256sums_does_not_list_is_refused() {
    let computer = Computer::new();
    let release = Release::new("#!/bin/sh\n", 1);
    fs::write(release.dir.path().join(SUMS), "").unwrap();

    let error = computer
        .install(&release, &computer.dump("wowpedia"))
        .unwrap_err();

    assert!(format!("{error:#}").contains("wrong SHA-256"), "{error:#}");
    assert!(!story_program(&computer).exists());
}

#[test]
fn a_release_with_a_missing_archive_installs_nothing() {
    let computer = Computer::new();
    let release = Release::new("#!/bin/sh\n", 1);
    fs::remove_file(release.dir.path().join(&release.asset)).unwrap();

    assert!(
        computer
            .install(&release, &computer.dump("wowpedia"))
            .is_err()
    );

    assert!(!story_program(&computer).exists());
    assert!(!computer.places().work.exists());
}

#[test]
fn a_release_for_a_newer_desktop_app_is_refused() {
    let computer = Computer::new();
    let release = Release::new("#!/bin/sh\n", 99);

    let error = computer
        .install(&release, &computer.dump("wowpedia"))
        .unwrap_err();

    assert!(
        error.to_string().contains("gnomish-relay update"),
        "{error}"
    );
}

#[test]
fn a_broken_dump_keeps_the_old_pack() {
    let computer = Computer::new();
    let release = Release::new("#!/bin/sh\n", 1);
    computer
        .install(&release, &computer.dump("wowpedia"))
        .unwrap();
    let pack = computer.places().pack;
    fs::write(&pack, "old lore").unwrap();

    let report = computer
        .install(&release, &computer.dump("garbage"))
        .unwrap();

    assert!(matches!(report.lore, Lore::Kept(ref e) if e.contains("old lore stays")));
    assert_eq!(fs::read_to_string(&pack).unwrap(), "old lore");
    assert!(!pack.with_extension("sqlite.new").exists());
}

#[test]
fn a_broken_dump_with_no_old_pack_sets_no_program() {
    let computer = Computer::new();
    let release = Release::new("#!/bin/sh\n", 1);
    let before = computer.config_text();

    let error = computer
        .install(&release, &computer.dump("garbage"))
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Couldn't build the Timeways lore"),
        "{error}"
    );
    assert_eq!(computer.config_text(), before);
}

#[test]
fn a_missing_dump_keeps_the_old_pack() {
    let computer = Computer::new();
    let release = Release::new("#!/bin/sh\n", 1);
    computer
        .install(&release, &computer.dump("wowpedia"))
        .unwrap();
    let missing = format!("{}/none.7z", file_url(computer.root.path()));

    let report = computer.install(&release, &missing).unwrap();

    assert!(matches!(report.lore, Lore::Kept(_)));
    assert_eq!(fs::read_to_string(computer.places().pack).unwrap(), "lore");
}

#[test]
fn the_download_of_the_dump_reports_its_size() {
    let computer = Computer::new();
    let url = computer.dump("wowpedia dump");
    let work = tempfile::tempdir().unwrap();
    let mut sizes = Vec::new();

    let dump = lore_pack::download_dump(&url, work.path(), |bytes| sizes.push(bytes)).unwrap();

    assert_eq!(sizes.last(), Some(&13));
    assert_eq!(fs::read_to_string(dump).unwrap(), "wowpedia dump");
}

#[test]
fn update_installs_new_programs_into_the_folder_of_the_story_program() {
    let computer = Computer::new();
    computer
        .install(
            &Release::new("#!/bin/sh\necho 1\n", 1),
            &computer.dump("wowpedia"),
        )
        .unwrap();
    let config = config::load(&computer.dirs.config, &computer.dirs.home).unwrap();
    let program = timeways_install::installed_story_program(&config).unwrap();
    let newer = Release::new("#!/bin/sh\necho 2\n", 1);
    let sources = Sources {
        release: newer.url(),
        dump: String::new(),
    };

    let changed = timeways_install::update(&computer.dirs, &sources, &program).unwrap();
    let again = timeways_install::update(&computer.dirs, &sources, &program).unwrap();

    assert_eq!(changed, ["timeways-story"]);
    assert!(again.is_empty());
    assert_eq!(fs::read_to_string(&program).unwrap(), "#!/bin/sh\necho 2\n");
}

#[test]
fn update_leaves_a_story_program_that_setup_did_not_install() {
    let computer = Computer::new();
    let text =
        computer.config_text() + "program = \"~/mine/story\"\nlore_pack = \"~/lore.sqlite\"\n";
    let config = config::parse(&text, &computer.dirs.home).unwrap();

    assert_eq!(timeways_install::installed_story_program(&config), None);
}
