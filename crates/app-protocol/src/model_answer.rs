//! The answer of a model call before it goes to the story program (SPEC.md 9.8).

/// The text of one answer for the story program. Its longest reply text is 8 KiB.
pub const MAX_ANSWER: usize = 16 * 1024;

/// An answer is hostile text. Control characters other than a newline and a tab go,
/// and a long answer is cut.
#[must_use]
pub fn clean_answer(text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    clean[..clean.floor_char_boundary(MAX_ANSWER)].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hostile_answer_loses_its_control_characters_and_is_cut() {
        assert_eq!(clean_answer("a\u{7}b\r\nc\td\u{1b}[31m"), "ab\nc\td[31m");
        let long = "é".repeat(MAX_ANSWER);
        let cut = clean_answer(&long);
        assert_eq!(cut.len(), MAX_ANSWER);
        assert!(cut.chars().all(|c| c == 'é'));
    }
}
