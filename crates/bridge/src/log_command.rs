//! The `command` field of the log: the full command of a shell call, cut, with no
//! control characters, and with the values that can be secrets hidden (SPEC.md 8.5).

const MAX_CHARS: usize = 500;
const HIDDEN: &str = "***";
const SECRET_NAMES: [&str; 6] = ["key", "token", "secret", "password", "passwd", "auth"];
const TOKEN_STARTS: [&str; 9] = [
    "sk-",
    "ghp_",
    "gho_",
    "ghs_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "AKIA",
    "glpat-",
];

/// The command as one line of at most 500 characters, with its secrets as `***`.
pub fn command_field(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let one_line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let hidden = hide_secrets(&one_line);
    cut(&hidden)
}

/// Works on words split by spaces, so the rest of the command keeps its form.
fn hide_secrets(line: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut hide_next = false;
    for word in line.split(' ') {
        if word.is_empty() {
            words.push(String::new());
            continue;
        }
        let shown = if hide_next {
            HIDDEN.to_owned()
        } else {
            hidden_word(word)
        };
        hide_next = takes_a_secret(word);
        words.push(shown);
    }
    words.join(" ")
}

/// A word that starts like a known token, also after a quote.
pub fn looks_like_token(word: &str) -> bool {
    let bare = word.trim_start_matches(['"', '\'']);
    TOKEN_STARTS.iter().any(|start| bare.starts_with(start))
}

fn hidden_word(word: &str) -> String {
    if looks_like_token(word) {
        return HIDDEN.to_owned();
    }
    let Some((name, _)) = word.split_once('=') else {
        return word.to_owned();
    };
    let hides = if name.starts_with('-') {
        has_secret_name(name)
    } else {
        is_variable_name(name)
    };
    if hides {
        format!("{name}={HIDDEN}")
    } else {
        word.to_owned()
    }
}

/// `TOKEN` in `TOKEN=x`. Not `a` in `a==b`, and nothing in `=x`.
fn is_variable_name(name: &str) -> bool {
    let Some(first) = name.chars().next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn has_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SECRET_NAMES.iter().any(|secret| lower.contains(secret))
}

/// `--token` and `Bearer` come before a secret. `--token=x` holds its own.
fn takes_a_secret(word: &str) -> bool {
    if word.eq_ignore_ascii_case("bearer") {
        return true;
    }
    word.starts_with('-') && !word.contains('=') && has_secret_name(word)
}

fn cut(text: &str) -> String {
    match text.char_indices().nth(MAX_CHARS) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_hides_its_secret_values() {
        let raw = b"TOKEN=abc123 curl --password hunter2 -H 'Authorization: Bearer xyz' --api-key=k1 ghp_secret";

        let field = command_field(raw);

        assert_eq!(
            field,
            "TOKEN=*** curl --password *** -H 'Authorization: Bearer *** --api-key=*** ***"
        );
        for secret in ["abc123", "hunter2", "xyz", "k1", "ghp_secret"] {
            assert!(!field.contains(secret), "{secret} in {field}");
        }
    }

    #[test]
    fn a_plain_command_stays_whole() {
        assert_eq!(
            command_field(b"cargo test -p bridge -- --test-threads=2"),
            "cargo test -p bridge -- --test-threads=2"
        );
        assert_eq!(command_field(b"git status"), "git status");
    }

    #[test]
    fn a_command_loses_its_control_characters() {
        assert_eq!(
            command_field(b"echo a\nrm -rf x\x1b[2J"),
            "echo a rm -rf x [2J"
        );
    }

    #[test]
    fn a_long_command_is_cut_to_500_characters() {
        let field = command_field("é".repeat(600).as_bytes());

        assert_eq!(field.chars().count(), 501);
        assert!(field.ends_with('…'));
    }

    #[test]
    fn an_equals_sign_with_no_name_is_not_a_secret() {
        assert_eq!(command_field(b"test a == b"), "test a == b");
    }
}
