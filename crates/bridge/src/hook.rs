//! `gnomish-relay hook claude` and `gnomish-relay hook codex`: one hook event of a terminal
//! session into the spool folder (SPEC.md 10.1). It never prints, and always exits 0, so
//! a hook never changes or fails a turn of the agent.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use protocol::notice::{MAX_REPO, MAX_TEXT};

use crate::dirs::Dirs;
use crate::hook_input::parse_hook_input;
use crate::spool::{Source, SpoolFile, Written, spool_dir, unique_name, write_file};

/// The bridge sets it for each process of a run, so a run from the game never notifies.
pub const JOB_VAR: &str = "GNOMISH_RELAY_JOB";
pub const MAX_INPUT: u64 = 1 << 20;
pub const TIME_LIMIT: Duration = Duration::from_millis(300);

pub fn source_of(agent: &str) -> Option<Source> {
    match agent {
        "claude" => Some(Source::Claude),
        "codex" => Some(Source::Codex),
        _ => None,
    }
}

/// A control character becomes a space, and the cut falls between two characters. The
/// file then stays below 4 KiB, because JSON doubles at most `"` and `\`.
pub fn cut_text(text: &str, max: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    out
}

/// The name of the git top folder of `cwd`, else the name of `cwd`. Never the full path:
/// a notification can show on a stream or a screenshot.
pub fn repo_name(cwd: &Path) -> String {
    let top = cwd.ancestors().find(|folder| folder.join(".git").exists());
    let folder = top.unwrap_or(cwd);
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned());
    cut_text(&name.unwrap_or_default(), MAX_REPO)
}

/// The spool file for the stdin of one hook, or `None` for an event that gives no
/// notification.
pub fn hook_file(source: Source, input: &[u8]) -> Option<SpoolFile> {
    let event = parse_hook_input(source, input)?;
    let repo = event.cwd.as_deref().map(Path::new).map(repo_name);
    Some(SpoolFile {
        v: 1,
        source,
        event: event.event,
        session: event.session,
        repo: repo.unwrap_or_default(),
        text: cut_text(&event.text, MAX_TEXT),
    })
}

static COUNT: AtomicU32 = AtomicU32::new(0);

fn new_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    unique_name(nanos, std::process::id(), count)
}

/// What one hook did, for the tests. The command itself ignores it.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    InJob,
    NoNotice,
    Spool(Written),
    Failed,
}

/// `name` comes from the start of the hook. Claude runs its hooks async, so a later
/// time can sort the file of `Stop` after the file of `SessionEnd`.
pub fn run_hook(source: Source, in_job: bool, data: &Path, input: &[u8], name: &str) -> Outcome {
    if in_job {
        return Outcome::InJob;
    }
    let Some(file) = hook_file(source, input) else {
        return Outcome::NoNotice;
    };
    match write_file(&spool_dir(data), name, &file.to_bytes()) {
        Ok(written) => Outcome::Spool(written),
        Err(_) => Outcome::Failed,
    }
}

fn read_input() -> Vec<u8> {
    let mut input = Vec::new();
    let _ = std::io::stdin().take(MAX_INPUT).read_to_end(&mut input);
    input
}

/// The command. A timer ends the process after `TIME_LIMIT`, also when the agent never
/// closes stdin.
pub fn main(agent: &str) -> ! {
    std::thread::spawn(|| {
        std::thread::sleep(TIME_LIMIT);
        std::process::exit(0);
    });
    let name = new_name();
    let in_job = std::env::var_os(JOB_VAR).is_some();
    if let (Some(source), Ok(dirs)) = (source_of(agent), Dirs::from_env()) {
        let input = if in_job { Vec::new() } else { read_input() };
        run_hook(source, in_job, &dirs.data, &input, &name);
    }
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spool::{SpoolEvent, open_spool, take_files};

    const STOP: &str = r#"{"session_id":"s1","cwd":"/nowhere/app","hook_event_name":"Stop","last_assistant_message":"Done."}"#;

    fn data_with_spool() -> tempfile::TempDir {
        let data = tempfile::tempdir().unwrap();
        open_spool(&spool_dir(data.path())).unwrap();
        data
    }

    #[test]
    fn the_spool_file_takes_the_name_from_the_start_of_the_hook() {
        let data = data_with_spool();
        let name = unique_name(1, 2, 3);

        run_hook(Source::Claude, false, data.path(), STOP.as_bytes(), &name);

        assert!(spool_dir(data.path()).join(format!("{name}.json")).exists());
    }

    #[test]
    fn a_hook_writes_one_spool_file_with_the_repo_name_only() {
        let data = data_with_spool();

        let outcome = run_hook(
            Source::Claude,
            false,
            data.path(),
            STOP.as_bytes(),
            &new_name(),
        );

        assert_eq!(outcome, Outcome::Spool(Written::Yes));
        let taken = take_files(&spool_dir(data.path()), SystemTime::now());
        assert_eq!(
            taken.files,
            [SpoolFile {
                v: 1,
                source: Source::Claude,
                event: SpoolEvent::Finished,
                session: "s1".into(),
                repo: "app".into(),
                text: "Done.".into(),
            }]
        );
    }

    #[test]
    fn a_hook_in_a_bridge_job_writes_nothing() {
        let data = data_with_spool();
        assert_eq!(
            run_hook(
                Source::Claude,
                true,
                data.path(),
                STOP.as_bytes(),
                &new_name()
            ),
            Outcome::InJob
        );
        assert_eq!(
            std::fs::read_dir(spool_dir(data.path())).unwrap().count(),
            0
        );
    }

    #[test]
    fn a_hook_with_no_bridge_writes_nothing() {
        let data = tempfile::tempdir().unwrap();
        assert_eq!(
            run_hook(
                Source::Codex,
                false,
                data.path(),
                STOP.as_bytes(),
                &new_name()
            ),
            Outcome::Spool(Written::NoFolder)
        );
    }

    #[test]
    fn an_event_with_no_notification_writes_nothing() {
        let data = data_with_spool();
        let input = STOP.replace("\"Stop\"", "\"PreToolUse\"");
        assert_eq!(
            run_hook(
                Source::Claude,
                false,
                data.path(),
                input.as_bytes(),
                &new_name()
            ),
            Outcome::NoNotice
        );
    }

    #[test]
    fn the_repo_is_the_git_top_folder_or_else_the_folder() {
        let home = tempfile::tempdir().unwrap();
        let deep = home.path().join("lighthouse").join("src").join("bin");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(repo_name(&deep), "bin");
        std::fs::create_dir(home.path().join("lighthouse").join(".git")).unwrap();
        assert_eq!(repo_name(&deep), "lighthouse");
    }

    #[test]
    fn a_long_text_is_cut_between_characters_and_loses_control_characters() {
        assert_eq!(cut_text("a\nb\tc", 600), "a b c");
        assert_eq!(cut_text("aé€", 4), "aé");
        let long = "\"\\".repeat(2000);
        let file = SpoolFile {
            v: 1,
            source: Source::Codex,
            event: SpoolEvent::Waiting,
            session: "s".repeat(128),
            repo: cut_text(&long, MAX_REPO),
            text: cut_text(&long, MAX_TEXT),
        };
        assert!(file.to_bytes().len() <= 4096);
    }

    #[test]
    fn a_huge_input_gives_no_notice_and_no_error() {
        let data = data_with_spool();
        let huge = vec![b'['; 1 << 20];
        assert_eq!(
            run_hook(Source::Claude, false, data.path(), &huge, &new_name()),
            Outcome::NoNotice
        );
    }

    #[test]
    fn only_claude_and_codex_are_hook_sources() {
        assert_eq!(source_of("claude"), Some(Source::Claude));
        assert_eq!(source_of("codex"), Some(Source::Codex));
        assert_eq!(source_of("gemini"), None);
    }
}
