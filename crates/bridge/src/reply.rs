//! The text of a finished reply for the game window (SPEC.md 7.3.1).

use protocol::markdown::render_markdown;

use crate::relay::Work;

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
        Work::Prompt | Work::ListSessions => render(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            fork: false,
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
