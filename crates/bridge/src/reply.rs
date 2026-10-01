//! The text of a finished reply for the game window (SPEC.md 7.3.1).

use protocol::markdown::render_markdown;

use crate::relay::Work;
use crate::usage::Usage;

const MARKER: &str = "\x1bM1\n";

/// An empty answer stays empty, so the game shows no empty reply block.
fn render(markdown: &str) -> String {
    if markdown.is_empty() {
        return String::new();
    }
    // The renderer adds ASCII only at ASCII bytes, so valid UTF-8 stays valid.
    String::from_utf8_lossy(&render_markdown(markdown.as_bytes())).into_owned()
}

/// An attach reply is the last prompt on one line and the answer below it. The
/// prompt is the user's own text, so it stays plain.
fn render_exchange(text: &str) -> String {
    let Some((prompt, answer)) = text.split_once('\n') else {
        return text.to_owned();
    };
    format!("{prompt}\n{}", render(answer))
}

/// A done reply of an agent, as blocks.
pub fn render_reply(work: &Work, text: &str) -> String {
    match work {
        Work::Attach { .. } => render_exchange(text),
        Work::Prompt
        | Work::ListSessions
        | Work::ListFolders
        | Work::ListSubfolders
        | Work::ListSettings => render(text),
        // The reply of a git action is text of the bridge, with its blocks already.
        Work::Git(_) => text.to_owned(),
    }
}

/// The usage line goes first, so a cut of a long reply never drops it (SPEC.md 9.10).
/// Only a reply of blocks, or an empty one, takes it.
pub fn with_usage(rendered: &str, usage: &Usage) -> String {
    let blocks = match rendered.strip_prefix(MARKER) {
        Some(blocks) => blocks,
        None if rendered.is_empty() => "",
        None => return rendered.to_owned(),
    };
    format!("{MARKER}u\x1f{}\n{blocks}", usage.line())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage() -> Usage {
        Usage {
            input: 1234,
            cached: 0,
            output: 350,
            cost_usd: Some(0.04),
        }
    }

    #[test]
    fn the_usage_line_comes_first_after_the_marker() {
        let rendered = render_reply(&Work::Prompt, "Done.");

        assert_eq!(
            with_usage(&rendered, &usage()),
            "\x1bM1\nu\x1f1.2k in · 350 out · $0.04\np\x1fDone.\n"
        );
    }

    #[test]
    fn an_empty_reply_with_usage_shows_only_the_usage_line() {
        assert_eq!(
            with_usage("", &usage()),
            "\x1bM1\nu\x1f1.2k in · 350 out · $0.04\n"
        );
    }

    #[test]
    fn a_reply_that_is_not_blocks_takes_no_usage_line() {
        assert_eq!(with_usage("fix it\n", &usage()), "fix it\n");
    }

    #[test]
    fn a_prompt_reply_becomes_blocks() {
        assert_eq!(
            render_reply(&Work::Prompt, "# Hi\n**x**"),
            "\x1bM1\nh\x1f1\x1fHi\np\x1f|cffffd100x|r\n"
        );
    }

    #[test]
    fn an_empty_reply_stays_empty() {
        assert_eq!(render_reply(&Work::Prompt, ""), "");
    }

    #[test]
    fn an_attach_reply_renders_only_the_answer() {
        let work = Work::Attach {
            session: "s1".into(),
            open: crate::relay::Open::Same,
        };
        assert_eq!(
            render_reply(&work, "fix **it**\nDone."),
            "fix **it**\n\x1bM1\np\x1fDone.\n"
        );
        assert_eq!(render_reply(&work, "just a prompt\n"), "just a prompt\n");
        assert_eq!(render_reply(&work, ""), "");
    }

    #[test]
    fn utf8_stays_whole() {
        assert_eq!(render_reply(&Work::Prompt, "é|"), "\x1bM1\np\x1fé||\n");
    }
}
