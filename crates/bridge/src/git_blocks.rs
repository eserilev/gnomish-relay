//! The blocks of the bridge in a reply (SPEC.md 7.3.1, 9.11). Their kinds are upper-case
//! letters, which the renderer never writes, so no agent text can make one.

use protocol::markdown::render_markdown;

use crate::chat_branch::{BranchInfo, Own};
use crate::ci_checks::CiCounts;
use crate::run_changes::{ChangeKind, RunChanges};
use crate::test_summary::TestCounts;

const MARKER: &str = "\x1bM1";
const US: char = '\x1f';
/// More rows make the reply long, and the player opens the files anyway.
const MAX_FILES: usize = 12;

/// What the bridge adds under a reply of a run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunBlocks {
    pub branch: Option<BranchInfo>,
    pub changes: Option<RunChanges>,
    pub tests: Option<TestCounts>,
    pub ci: Option<CiCounts>,
}

/// A field with no control character and every `|` doubled (S10).
fn field(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .replace('|', "||")
}

fn block(kind: char, fields: &[String]) -> String {
    let mut line = String::from("\n");
    line.push(kind);
    for f in fields {
        line.push(US);
        line.push_str(&field(f));
    }
    line
}

fn branch_block(branch: &BranchInfo) -> String {
    let own = match branch.own {
        Own::Yes => "1",
        Own::No => "0",
    };
    block(
        'B',
        &[branch.branch.clone(), own.into(), branch.start.clone()],
    )
}

fn file_blocks(changes: &RunChanges) -> String {
    let mut text = String::new();
    for file in changes.files.iter().take(MAX_FILES) {
        let (added, removed) = file.lines.map_or((String::new(), String::new()), |(a, r)| {
            (a.to_string(), r.to_string())
        });
        let kind = match file.kind {
            ChangeKind::Added => "A",
            ChangeKind::Removed => "D",
            ChangeKind::Modified => "M",
        };
        text.push_str(&block(
            'F',
            &[file.path.clone(), added, removed, kind.into()],
        ));
    }
    let more = changes.files.len().saturating_sub(MAX_FILES);
    if more > 0 {
        text.push_str(&block('M', &[more.to_string()]));
    }
    text
}

fn changes_blocks(changes: &RunChanges) -> String {
    let (added, removed) = changes.totals();
    let summary = [
        changes.files.len().to_string(),
        added.to_string(),
        removed.to_string(),
    ];
    block('G', &summary) + &file_blocks(changes)
}

fn tests_block(tests: &TestCounts) -> String {
    let counts = [tests.passed, tests.failed, tests.skipped].map(|n| n.to_string());
    block('T', &counts)
}

pub fn ci_block(ci: &CiCounts) -> String {
    let counts = [ci.passed, ci.failed, ci.running].map(|n| n.to_string());
    let mut fields = counts.to_vec();
    fields.push(ci.failed_names.join(", "));
    block('C', &fields)
}

/// The blocks, each with its `\n` in front, in the order of the addon.
pub fn blocks(run: &RunBlocks) -> String {
    let mut text = String::new();
    if let Some(branch) = &run.branch {
        text.push_str(&branch_block(branch));
    }
    if let Some(changes) = &run.changes {
        text.push_str(&changes_blocks(changes));
    }
    if let Some(tests) = &run.tests {
        text.push_str(&tests_block(tests));
    }
    if let Some(ci) = &run.ci {
        text.push_str(&ci_block(ci));
    }
    text
}

/// A rendered reply with the blocks right after the marker, so a cut never takes them.
/// An empty reply becomes the marker and the blocks alone.
pub fn with_blocks(rendered: &str, blocks: &str) -> String {
    if blocks.is_empty() {
        return rendered.to_owned();
    }
    match rendered.strip_prefix(MARKER) {
        Some(rest) => format!("{MARKER}{blocks}{rest}"),
        None => format!("{MARKER}{blocks}\n"),
    }
}

/// An error with blocks goes through the renderer too, so its bytes get the escapes of
/// the agent text.
pub fn error_with_blocks(error: &str, blocks: &str) -> String {
    let rendered = String::from_utf8_lossy(&render_markdown(error.as_bytes())).into_owned();
    with_blocks(&rendered, blocks)
}

/// An error text can hold text of the agent. With no ESC, it never starts with the marker.
pub fn plain_error(error: &str) -> String {
    error.replace('\x1b', "")
}

/// The text without the blocks of the bridge, for the history of a restore.
pub fn without_blocks(text: &str) -> String {
    let Some(rest) = text.strip_prefix(MARKER) else {
        return text.to_owned();
    };
    let kept: Vec<&str> = rest
        .split('\n')
        .filter(|line| !line.starts_with(|c: char| c.is_ascii_uppercase()))
        .collect();
    let rest = kept.join("\n");
    if rest.trim_matches('\n').is_empty() {
        return String::new();
    }
    format!("{MARKER}{rest}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lane::{ChatId, MessageId};
    use crate::run_changes::{FileChange, Outcome, Snapshot};

    fn changes(files: usize) -> RunChanges {
        RunChanges {
            chat: ChatId::new("c"),
            id: MessageId(1),
            top: "/r".into(),
            start: Snapshot {
                tree: "a".into(),
                head: None,
            },
            end: Snapshot {
                tree: "b".into(),
                head: None,
            },
            files: (0..files)
                .map(|i| FileChange {
                    path: format!("f{i}|x\n.rs"),
                    lines: Some((2, 1)),
                    kind: ChangeKind::Modified,
                })
                .collect(),
            odd_names: false,
            shared: false,
            outcome: Outcome::Open,
        }
    }

    fn own_branch() -> BranchInfo {
        BranchInfo {
            branch: "gnomish/x|y".into(),
            own: Own::Yes,
            start: "main".into(),
        }
    }

    #[test]
    fn the_blocks_go_right_after_the_marker() {
        let run = RunBlocks {
            branch: Some(own_branch()),
            ..RunBlocks::default()
        };

        let text = with_blocks("\x1bM1\np\x1fDone.\n", &blocks(&run));

        assert_eq!(text, "\x1bM1\nB\x1fgnomish/x||y\x1f1\x1fmain\np\x1fDone.\n");
    }

    #[test]
    fn a_path_loses_its_control_characters_and_doubles_each_bar() {
        let text = blocks(&RunBlocks {
            changes: Some(changes(1)),
            ..RunBlocks::default()
        });

        assert_eq!(text, "\nG\x1f1\x1f2\x1f1\nF\x1ff0||x.rs\x1f2\x1f1\x1fM");
    }

    #[test]
    fn at_most_twelve_files_show_and_a_last_row_counts_the_rest() {
        let text = blocks(&RunBlocks {
            changes: Some(changes(15)),
            ..RunBlocks::default()
        });

        assert_eq!(text.matches("\nF\x1f").count(), 12);
        assert!(text.ends_with("\nM\x1f3"));
    }

    #[test]
    fn an_empty_reply_with_blocks_is_the_marker_and_the_blocks() {
        let run = RunBlocks {
            branch: Some(own_branch()),
            ..RunBlocks::default()
        };

        let text = with_blocks("", &blocks(&run));

        assert_eq!(text, "\x1bM1\nB\x1fgnomish/x||y\x1f1\x1fmain\n");
    }

    #[test]
    fn an_error_with_blocks_is_rendered_as_a_paragraph() {
        let text = error_with_blocks("Stopped.", "\nB\x1fmain\x1f0\x1f");

        assert_eq!(text, "\x1bM1\nB\x1fmain\x1f0\x1f\np\x1fStopped.\n");
    }

    #[test]
    fn a_plain_error_can_never_start_with_the_marker() {
        assert_eq!(plain_error("\x1bM1\nB\x1ffake"), "M1\nB\x1ffake");
    }

    #[test]
    fn the_history_keeps_the_reply_without_the_blocks() {
        let text = "\x1bM1\nB\x1fx\x1f0\x1f\nG\x1f1\x1f1\x1f0\np\x1fDone.\n";

        assert_eq!(without_blocks(text), "\x1bM1\np\x1fDone.\n");
        assert_eq!(without_blocks("\x1bM1\nG\x1f1\x1f1\x1f0\n"), "");
        assert_eq!(without_blocks("plain"), "plain");
    }

    #[test]
    fn the_test_line_follows_the_change_summary() {
        let run = RunBlocks {
            changes: Some(changes(1)),
            tests: Some(TestCounts {
                passed: 4,
                failed: 1,
                skipped: 0,
            }),
            ..RunBlocks::default()
        };

        let text = blocks(&run);

        assert!(text.ends_with("\x1fM\nT\x1f4\x1f1\x1f0"), "{text:?}");
    }

    #[test]
    fn the_ci_line_names_its_failed_checks() {
        let ci = CiCounts {
            passed: 5,
            failed: 2,
            running: 1,
            failed_names: vec!["lint".into(), "e2e|x".into()],
        };

        let text = with_blocks("", &ci_block(&ci));

        assert_eq!(text, "\x1bM1\nC\x1f5\x1f2\x1f1\x1flint, e2e||x\n");
    }

    #[test]
    fn no_blocks_leave_the_reply_as_it_is() {
        assert_eq!(with_blocks("\x1bM1\np\x1fx\n", ""), "\x1bM1\np\x1fx\n");
        assert_eq!(blocks(&RunBlocks::default()), "");
    }
}
