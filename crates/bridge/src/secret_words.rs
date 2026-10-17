//! The words of a command that name a secret path (SPEC.md 6.6.4). The sandbox hides
//! such a path, but at `auto-edit` the player still sees the question.

use protocol::shell::Script;

use crate::action_input::DESKTOP_PATHS;

/// `deny` holds the resolved `deny` folders of the policy.
pub fn names_secret(script: &Script, deny: &[Vec<u8>]) -> bool {
    let patterns = secret_patterns(deny);
    let words = script.simples.iter().flat_map(|simple| &simple.words);
    let targets = script.redirects.iter().map(|redirect| &redirect.target);
    words
        .chain(targets)
        .any(|word| word_names_any(word, &patterns))
}

/// The `desktop` paths, and the last two parts of each `deny` folder: a word such as
/// `~/.config/gnomish-relay` names the config folder with no home folder in it.
fn secret_patterns(deny: &[Vec<u8>]) -> Vec<Vec<String>> {
    let mut patterns: Vec<Vec<String>> = DESKTOP_PATHS
        .iter()
        .map(|pattern| parts(pattern.as_bytes()))
        .collect();
    for folder in deny {
        let all = parts(after_last_colon(folder));
        let start = all.len().saturating_sub(2);
        patterns.push(all[start..].to_vec());
    }
    patterns
}

/// `-v ~/.ssh:/x`, `--file=~/.ssh/id`, and `--mount source=~/.ssh,target=/x` each hold a
/// path after a `:`, a `=`, or a `,`.
fn word_names_any(word: &[u8], patterns: &[Vec<String>]) -> bool {
    word.split(|b| matches!(b, b'=' | b':' | b','))
        .any(|piece| {
            let parts = parts(piece);
            patterns.iter().any(|pattern| pattern_in(pattern, &parts))
        })
}

/// `word_names_any` cuts each word at `:`, so a part such as `C:` never matches. The
/// drive and a `\\?\` prefix go with it.
fn after_last_colon(folder: &[u8]) -> &[u8] {
    folder.rsplit(|b| *b == b':').next().unwrap_or(folder)
}

/// The named parts of a path in lower case, with no empty part and no `.`.
fn parts(path: &[u8]) -> Vec<String> {
    path.split(|b| *b == b'/' || *b == b'\\')
        .filter(|part| !part.is_empty() && *part != b".")
        .map(|part| String::from_utf8_lossy(part).to_ascii_lowercase())
        .collect()
}

/// Every part of the pattern, in a row: `config.json` alone is not `.docker/config.json`.
fn pattern_in(pattern: &[String], parts: &[String]) -> bool {
    if pattern.is_empty() || pattern.len() > parts.len() {
        return false;
    }
    parts.windows(pattern.len()).any(|run| {
        run.iter()
            .zip(pattern)
            .all(|(part, want)| part_matches(want, part))
    })
}

/// A last `*` matches the rest of a part, as in the classifier.
fn part_matches(pattern: &str, part: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(start) => part.starts_with(start),
        None => part == pattern,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::shell::split;

    fn named(raw: &str) -> bool {
        let deny = [b"/home/x/.config/gnomish-relay".to_vec()];
        names_secret(&split(raw.as_bytes()).unwrap(), &deny)
    }

    #[test]
    fn a_word_that_names_a_secret_path_is_found() {
        for raw in [
            "cat ~/.ssh/id_ed25519",
            "cat ~/.SSH/config",
            "cat .env",
            "cat .env.local",
            "cat ~/.docker/config.json",
            "ls -v ~/.ssh:/root/.ssh",
            "tool --file=~/.aws/credentials",
            "cat < ~/.netrc",
            "cat ~/.config/gnomish-relay/strip.key",
            "docker run --mount type=bind,source=~/.ssh,target=/x img",
        ] {
            assert!(named(raw), "{raw}");
        }
    }

    #[test]
    fn a_common_command_names_no_secret() {
        for raw in [
            "gh pr list",
            "pip install requests",
            "cat config.json",
            "cat .github/workflows/ci.yml",
            "cargo test -p gnomish-relay",
            "git config user.name",
        ] {
            assert!(!named(raw), "{raw}");
        }
    }

    /// A folder named `.env` matches `.env` too, so a virtual environment asks.
    #[test]
    fn a_virtual_environment_named_env_counts_as_a_secret() {
        assert!(named("python -m venv .env"));
        assert!(named("ls .env/bin"));
    }

    fn named_with_deny(raw: &str, deny: &[u8]) -> bool {
        names_secret(&split(raw.as_bytes()).unwrap(), &[deny.to_vec()])
    }

    /// Unquoted, bash reads each `\` as an escape, so only a quoted path keeps its parts.
    #[test]
    fn a_quoted_windows_path_names_the_config_folder() {
        let deny = br"\\?\C:\Users\x\.config\gnomish-relay";
        let raw = r"cat '\\?\C:\Users\x\.config\gnomish-relay\strip.key'";

        assert!(named_with_deny(raw, deny));
    }

    #[test]
    fn a_deny_folder_next_to_the_drive_names_a_secret() {
        for deny in [&br"/C:/secrets"[..], br"\\?\C:\secrets"] {
            assert!(named_with_deny(r"cat 'C:\secrets\key'", deny));
            assert!(named_with_deny("cat C:/secrets/key", deny));
        }
    }

    /// The classifier treats `.env.*` as a secret, so a template is one too.
    #[test]
    fn an_env_example_counts_as_a_secret() {
        assert!(named("cat .env.example"));
    }
}
