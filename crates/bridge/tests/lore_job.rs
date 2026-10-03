//! The desktop app builds the Timeways lore pack in the background (SPEC.md 11.4). A
//! fake dump sits in a temp folder, so no test needs the network. The fake
//! `timeways-pack` is a shell script.
#![cfg(unix)]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use bridge::lore_job::{self, Finished, LoreJob, LoreParts, LoreState, REBUILD_FILE, read_state};
use bridge::lore_pack::DUMP_FILE;

const PACK_SCRIPT: &str = "#!/bin/sh\n\
    [ \"$1\" = from-dump ] || exit 2\n\
    [ -e \"$3\" ] && exit 3\n\
    grep -q wowpedia \"$2\" || exit 4\n\
    echo 'read 1 pages, skipped 0'\n\
    echo \"wrote 325 passages to $3\"\n\
    printf 'new lore' > \"$3\"\n";

/// A data folder, a pack folder, the fake `timeways-pack`, and a dump with `dump`.
fn computer(root: &Path, dump: &str) -> LoreParts {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let program = bin.join("timeways-pack");
    bridge::fake_program::write(&program, PACK_SCRIPT).unwrap();
    let dumps = root.join("dumps");
    fs::create_dir_all(&dumps).unwrap();
    fs::write(dumps.join(DUMP_FILE), dump).unwrap();
    let data = root.join("data");
    fs::create_dir_all(&data).unwrap();
    LoreParts {
        pack_program: program,
        pack: root.join("timeways/lore.sqlite"),
        dump_url: format!("file://{}/{DUMP_FILE}", dumps.display()),
        data,
    }
}

fn old_pack(parts: &LoreParts) {
    fs::create_dir_all(parts.pack.parent().unwrap()).unwrap();
    fs::write(&parts.pack, "old lore").unwrap();
}

/// Ticks the job until a build ends, for at most 30 seconds.
fn finish(job: &mut LoreJob) -> Finished {
    let start = Instant::now();
    loop {
        if let Some(finished) = job.tick(Instant::now()) {
            return finished;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "the build never ended"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_build_downloads_the_dump_builds_the_pack_and_says_ready() {
    let root = tempfile::tempdir().unwrap();
    let parts = computer(root.path(), "wowpedia dump");

    let summary = lore_job::build(&parts).unwrap();

    assert_eq!(fs::read_to_string(&parts.pack).unwrap(), "new lore");
    assert_eq!(summary[0], "read 1 pages, skipped 0");
    assert_eq!(read_state(&parts.data), Some(LoreState::Ready));
    assert!(
        !parts.data.join("timeways-lore").exists(),
        "the dump is gone"
    );
}

#[test]
fn a_broken_dump_keeps_the_old_pack_and_the_state_says_why() {
    let root = tempfile::tempdir().unwrap();
    let parts = computer(root.path(), "garbage");
    old_pack(&parts);

    let result = lore_job::build(&parts);

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&parts.pack).unwrap(), "old lore");
    assert!(!parts.pack.with_extension("sqlite.new").exists());
    assert_eq!(
        read_state(&parts.data),
        Some(LoreState::Failed("couldn't build the lore".into()))
    );
}

#[test]
fn a_dump_that_does_not_download_keeps_the_old_pack() {
    let root = tempfile::tempdir().unwrap();
    let mut parts = computer(root.path(), "wowpedia dump");
    parts.dump_url = format!("file://{}/none.7z", root.path().display());
    old_pack(&parts);

    let result = lore_job::build(&parts);

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&parts.pack).unwrap(), "old lore");
    let state = read_state(&parts.data).unwrap();
    assert!(
        matches!(&state, LoreState::Failed(reason) if reason.starts_with("couldn't download the Wowpedia lore")),
        "{state:?}"
    );
}

#[test]
fn a_rebuild_request_replaces_the_old_pack_and_ends() {
    let root = tempfile::tempdir().unwrap();
    let parts = computer(root.path(), "wowpedia dump");
    old_pack(&parts);
    fs::write(parts.data.join(REBUILD_FILE), "").unwrap();
    let mut job = LoreJob::new(parts.clone());

    let finished = finish(&mut job);

    assert_eq!(finished, Finished::Built);
    assert_eq!(fs::read_to_string(&parts.pack).unwrap(), "new lore");
    assert!(!parts.data.join(REBUILD_FILE).exists());
    assert_eq!(job.tick(Instant::now()), None, "no second build");
}

#[test]
fn with_a_pack_and_no_request_the_job_builds_nothing() {
    let root = tempfile::tempdir().unwrap();
    let parts = computer(root.path(), "wowpedia dump");
    old_pack(&parts);
    let mut job = LoreJob::new(parts.clone());

    assert_eq!(job.tick(Instant::now()), None);
    std::thread::sleep(Duration::from_millis(100));

    assert_eq!(job.tick(Instant::now()), None);
    assert_eq!(fs::read_to_string(&parts.pack).unwrap(), "old lore");
    assert_eq!(read_state(&parts.data), None);
}

#[test]
fn a_failed_build_tries_again_only_after_an_hour() {
    let root = tempfile::tempdir().unwrap();
    let parts = computer(root.path(), "garbage");
    let mut job = LoreJob::new(parts.clone());
    assert_eq!(finish(&mut job), Finished::Failed);
    fs::write(root.path().join("dumps").join(DUMP_FILE), "wowpedia dump").unwrap();

    let soon = job.tick(Instant::now() + Duration::from_mins(1));
    std::thread::sleep(Duration::from_millis(100));
    let still = job.tick(Instant::now() + Duration::from_mins(1));

    assert_eq!((soon, still), (None, None));
    assert!(!parts.pack.exists(), "no second try within the hour");
}
