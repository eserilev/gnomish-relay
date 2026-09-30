//! The log line of each game question, and the summary of a tool call in it (SPEC.md
//! 6.6.3, "The log of requests"). The summary comes from the classifier input, never
//! from the popup text, so no argument, file content, or agent text reaches the log.

use std::path::Path;

use protocol::action::ToolCall;
use protocol::always::is_plain_word;
use protocol::shell::split;

use crate::action_input::resolved_bytes;

const MAX_NAMES: usize = 4;
const MAX_PATHS: usize = 3;
const UNKNOWN: &str = "?";

pub fn game_line(agent: &str, folder: &str, summary: &str) -> String {
    format!("game request: {agent} in {folder}: {summary}")
}

/// `chat` is resolved. `title` gives only the tool name, when it starts with a plain word.
pub fn summary(tool: &ToolCall, title: &str, chat: &Path) -> String {
    match tool {
        ToolCall::Command { raw, .. } => command_summary(raw),
        ToolCall::Files { reads, writes } if writes.is_empty() => {
            files_summary("read", reads, chat)
        }
        ToolCall::Files { writes, .. } => files_summary("write", writes, chat),
        ToolCall::Unknown => tool_name(title).to_owned(),
    }
}

fn command_summary(raw: &[u8]) -> String {
    let Some(script) = split(raw) else {
        return format!("command {UNKNOWN}");
    };
    let names: Vec<&str> = script
        .simples
        .iter()
        .take(MAX_NAMES)
        .map(|simple| name_of(simple.words.first()))
        .collect();
    if names.is_empty() {
        return format!("command {UNKNOWN}");
    }
    format!("command {}", names.join(", "))
}

/// The last part of the first word. `TOKEN=x` and other words that are not plain are `?`.
fn name_of(word: Option<&Vec<u8>>) -> &str {
    let Some(word) = word else {
        return UNKNOWN;
    };
    let last = word
        .rsplit(|b| *b == b'/' || *b == b'\\')
        .next()
        .unwrap_or(word);
    if !is_plain_word(last) {
        return UNKNOWN;
    }
    std::str::from_utf8(last).unwrap_or(UNKNOWN)
}

fn files_summary(verb: &str, paths: &[Vec<u8>], chat: &Path) -> String {
    let chat = resolved_bytes(chat);
    let mut parts: Vec<String> = paths
        .iter()
        .take(MAX_PATHS)
        .map(|path| String::from_utf8_lossy(relative_to(path, &chat)).into_owned())
        .collect();
    let more = paths.len().saturating_sub(MAX_PATHS);
    if more > 0 {
        parts.push(format!("and {more} more"));
    }
    parts.insert(0, verb.to_owned());
    parts.join(" ")
}

/// A path inside the chat folder, from the chat folder. Any other path stays whole.
fn relative_to<'a>(path: &'a [u8], chat: &[u8]) -> &'a [u8] {
    if path == chat {
        return b".";
    }
    match path.strip_prefix(chat) {
        Some([b'/', rest @ ..]) => rest,
        _ => path,
    }
}

/// Claude's title is `<tool>: <reason>`. Other agents write free text there.
fn tool_name(title: &str) -> &str {
    let head = title.split(':').next().unwrap_or("");
    if is_plain_word(head.as_bytes()) {
        head
    } else {
        "a tool"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_input::{command_call, file_call};
    use std::path::PathBuf;

    const CHAT: &str = "/home/x/Code/app";

    fn command(raw: &str) -> String {
        summary(&command_call(raw, Path::new(CHAT)), "Bash", Path::new(CHAT))
    }

    #[test]
    fn a_command_logs_only_the_names_of_its_commands() {
        assert_eq!(
            command("cargo test -q --secret-flag abc && tail -5 log.txt"),
            "command cargo, tail"
        );
        assert_eq!(command("/usr/bin/ls -la"), "command ls");
    }

    #[test]
    fn a_variable_before_a_command_and_a_command_that_does_not_parse_log_a_question_mark() {
        assert_eq!(command("TOKEN=ghp_secret make"), "command ?");
        assert_eq!(command("echo \"$SECRET\""), "command ?");
    }

    #[test]
    fn a_command_logs_at_most_four_names() {
        assert_eq!(command("a; b; c; d; e"), "command a, b, c, d");
    }

    #[test]
    fn a_file_call_logs_its_paths_from_the_chat_folder() {
        let chat = PathBuf::from(CHAT);
        let read = file_call(&[chat.join("src/main.rs")], &[]);
        let write = file_call(&[], &[PathBuf::from("/etc/hosts")]);

        assert_eq!(summary(&read, "Read", &chat), "read src/main.rs");
        assert_eq!(summary(&write, "Write", &chat), "write /etc/hosts");
    }

    #[test]
    fn a_file_call_logs_at_most_three_paths() {
        let chat = PathBuf::from(CHAT);
        let paths: Vec<PathBuf> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|name| chat.join(name))
            .collect();

        let read = file_call(&paths, &[]);

        assert_eq!(summary(&read, "Grep", &chat), "read a b c and 2 more");
    }

    #[test]
    fn another_tool_logs_its_name_and_never_the_words_of_the_agent() {
        let chat = PathBuf::from(CHAT);

        let named = summary(&ToolCall::Unknown, "WebFetch: send my key", &chat);
        let free = summary(&ToolCall::Unknown, "Fetch the page with my key", &chat);

        assert_eq!((named.as_str(), free.as_str()), ("WebFetch", "a tool"));
    }

    #[test]
    fn the_game_line_names_the_agent_the_folder_and_the_summary() {
        assert_eq!(
            game_line("claude", CHAT, "command ls"),
            "game request: claude in /home/x/Code/app: command ls"
        );
    }
}
