//! The one change that setup makes to keys that exist: `program` and `lore_pack` of
//! `[story]`, after it installs the Timeways programs and builds the lore pack
//! (SPEC.md 11.4). Every other line stays.

use std::path::Path;

/// The comment above the two keys in a config from setup (`config_text::story_table`).
pub const STORY_PATHS_NOTE: [&str; 2] = [
    "# The story program of Timeways and its lore pack: absolute paths, or ones that",
    "# start with ~/. Setup sets both when it installs Timeways (SPEC.md 11.4).",
];

const KEYS: [&str; 2] = ["program", "lore_pack"];

/// A path in the home folder reads better with `~/`, and the config takes it.
pub fn config_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy().replace('\\', "/")),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `program = ...` or `# program = ...`, and the same for `lore_pack`.
fn is_story_path_line(line: &str) -> bool {
    let bare = line.trim().trim_start_matches('#').trim_start();
    KEYS.iter().any(|key| {
        bare.strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

fn is_header(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

fn is_story_header(line: &str) -> bool {
    line.split('#').next().unwrap_or_default().trim() == "[story]"
}

/// The text with `program` and `lore_pack` in `[story]`. A config with no `[story]`
/// gets one at its end. The caller checks the result with the config loader.
pub fn with_story_paths(text: &str, program: &str, lore_pack: &str) -> String {
    let keys = format!(
        "program = {}\nlore_pack = {}\n",
        quote(program),
        quote(lore_pack)
    );
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let Some(start) = lines.iter().position(|line| is_story_header(line)) else {
        let end = if text.is_empty() || text.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        return format!("{text}{end}\n[story]\n{keys}");
    };
    let mut out = String::with_capacity(text.len() + keys.len());
    let mut in_story = false;
    for (i, line) in lines.iter().enumerate() {
        if is_header(line) {
            in_story = i == start;
        }
        let old = is_story_path_line(line) || STORY_PATHS_NOTE.contains(&line.trim_end());
        if in_story && old {
            continue;
        }
        out.push_str(line);
        if i == start {
            if !line.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&keys);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_commented_keys_of_setup_become_real_keys() {
        let text = format!(
            "[wow]\npath = \"~/wow\"\n\n[story]\n{}\n{}\n# program = \"~/x\"\n\
             # lore_pack = \"~/y\"\nmodel = \"claude\"\n",
            STORY_PATHS_NOTE[0], STORY_PATHS_NOTE[1]
        );

        let new = with_story_paths(&text, "~/.local/bin/timeways-story", "~/lore.sqlite");

        assert_eq!(
            new,
            "[wow]\npath = \"~/wow\"\n\n[story]\nprogram = \"~/.local/bin/timeways-story\"\n\
             lore_pack = \"~/lore.sqlite\"\nmodel = \"claude\"\n"
        );
    }

    #[test]
    fn old_keys_are_replaced_and_other_tables_stay() {
        let text = "[story]\nprogram = \"~/old\"\nlore_pack=\"~/old.sqlite\"\n\
                    [sandbox]\n# program = \"a note of the user\"\n";

        let new = with_story_paths(text, "/opt/story", "/opt/lore");

        assert_eq!(
            new,
            "[story]\nprogram = \"/opt/story\"\nlore_pack = \"/opt/lore\"\n\
             [sandbox]\n# program = \"a note of the user\"\n"
        );
    }

    #[test]
    fn a_config_with_no_story_table_gets_one_at_its_end() {
        let new = with_story_paths("[wow]\npath = \"~/wow\"", "/s", "/l");

        assert_eq!(
            new,
            "[wow]\npath = \"~/wow\"\n\n[story]\nprogram = \"/s\"\nlore_pack = \"/l\"\n"
        );
    }

    #[test]
    fn a_windows_path_is_quoted_for_toml() {
        let new = with_story_paths("", "C:\\Users\\x\\story.exe", "/l");
        assert!(
            new.contains("program = \"C:\\\\Users\\\\x\\\\story.exe\"\n"),
            "{new}"
        );
    }

    #[test]
    fn a_path_in_the_home_folder_starts_with_a_tilde() {
        let home = Path::new("/home/x");
        assert_eq!(
            config_path(&home.join(".local/bin/timeways-story"), home),
            "~/.local/bin/timeways-story"
        );
        assert_eq!(config_path(Path::new("/opt/s"), home), "/opt/s");
    }
}
