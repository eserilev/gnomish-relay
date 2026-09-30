//! The blocks of the bridge in a reply (SPEC.md 7.3.1, 9.10). Their kinds are upper-case
//! letters, which the renderer never writes, so no agent text can make one.

use protocol::markdown::render_markdown;

use crate::chat_branch::{BranchInfo, Own};

const MARKER: &str = "\x1bM1";
const US: char = '\x1f';

/// What the bridge adds under a reply of a run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunBlocks {
    pub branch: Option<BranchInfo>,
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

/// The blocks, each with its `\n` in front, in the order of the addon.
pub fn blocks(run: &RunBlocks) -> String {
    let mut text = String::new();
    if let Some(branch) = &run.branch {
        text.push_str(&branch_block(branch));
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

    fn own_branch() -> RunBlocks {
        RunBlocks {
            branch: Some(BranchInfo {
                branch: "gnomish/x|y".into(),
                own: Own::Yes,
                start: "main".into(),
            }),
        }
    }

    #[test]
    fn the_blocks_go_right_after_the_marker() {
        let text = with_blocks("\x1bM1\np\x1fDone.\n", &blocks(&own_branch()));

        assert_eq!(text, "\x1bM1\nB\x1fgnomish/x||y\x1f1\x1fmain\np\x1fDone.\n");
    }

    #[test]
    fn an_empty_reply_with_blocks_is_the_marker_and_the_blocks() {
        let text = with_blocks("", &blocks(&own_branch()));

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
        let text = "\x1bM1\nB\x1fx\x1f0\x1f\np\x1fDone.\n";

        assert_eq!(without_blocks(text), "\x1bM1\np\x1fDone.\n");
        assert_eq!(without_blocks("\x1bM1\nB\x1fx\x1f0\x1f\n"), "");
        assert_eq!(without_blocks("plain"), "plain");
    }

    #[test]
    fn no_blocks_leave_the_reply_as_it_is() {
        assert_eq!(with_blocks("\x1bM1\np\x1fx\n", ""), "\x1bM1\np\x1fx\n");
        assert_eq!(blocks(&RunBlocks::default()), "");
    }
}
