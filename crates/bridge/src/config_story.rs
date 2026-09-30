//! The changes that setup makes to keys of `[story]` that exist: `program` and
//! `lore_pack` after it installs the Timeways programs and builds the lore pack
//! (SPEC.md 11.4), and the model after it finds or installs one (11.6). Every other
//! line stays.

use std::path::Path;

use crate::config_text::model_lines;
use crate::model_setup::FoundModel;

/// The comment above the two keys in a config from setup (`config_text::story_table`).
pub const STORY_PATHS_NOTE: [&str; 2] = [
    "# The story program of Timeways and its lore pack: absolute paths, or ones that",
    "# start with ~/. Setup sets both when it installs Timeways (SPEC.md 11.4).",
];

/// The comment in a `[story]` of a setup that found no model.
pub const NO_MODEL_NOTE: &str =
    "# No model found. Install claude, or start Ollama or LM Studio, then set:";

const PATH_KEYS: [&str; 2] = ["program", "lore_pack"];
const MODEL_KEYS: [&str; 4] = ["model", "claude_model", "local_url", "local_model"];

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

/// `key = ...` or `# key = ...`, for each key of `keys`.
fn is_key_line(line: &str, keys: &[&str]) -> bool {
    let bare = line.trim().trim_start_matches('#').trim_start();
    keys.iter().any(|key| {
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
    with_story_keys(text, &PATH_KEYS, &keys, &STORY_PATHS_NOTE)
}

/// The text with the keys of `model` in `[story]`, in place of the keys of an old
/// model. The caller checks the result with the config loader.
pub fn with_story_model(text: &str, model: &FoundModel) -> String {
    with_story_keys(text, &MODEL_KEYS, &model_lines(model, ""), &[NO_MODEL_NOTE])
}

/// `new_lines` go right below the `[story]` header. The old lines of `keys` and the
/// `notes` inside `[story]` go away.
fn with_story_keys(text: &str, keys: &[&str], new_lines: &str, notes: &[&str]) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let Some(start) = lines.iter().position(|line| is_story_header(line)) else {
        let end = if text.is_empty() || text.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        return format!("{text}{end}\n[story]\n{new_lines}");
    };
    let mut out = String::with_capacity(text.len() + new_lines.len());
    let mut in_story = false;
    for (i, line) in lines.iter().enumerate() {
        if is_header(line) {
            in_story = i == start;
        }
        let old = is_key_line(line, keys) || notes.contains(&line.trim_end());
        if in_story && old {
            continue;
        }
        out.push_str(line);
        if i == start {
            if !line.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(new_lines);
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

    fn ollama(model: &str) -> FoundModel {
        FoundModel::Local {
            url: "http://127.0.0.1:11434".into(),
            model: model.into(),
        }
    }

    #[test]
    fn a_story_with_no_model_gets_the_local_model_and_loads() {
        let home = tempfile::tempdir().unwrap();
        let text = crate::config_text::timeways_config(&home.path().join("wow"), &[]);

        let new = with_story_model(&text, &ollama("llama3.2:3b"));

        assert!(!new.contains(NO_MODEL_NOTE), "{new}");
        assert!(!new.contains("# model = \"claude\""), "{new}");
        let config = crate::config::parse(&new, home.path()).unwrap();
        let story = config.story.unwrap();
        assert_eq!(
            story.model.choice,
            crate::model::ModelChoice::Local(crate::model_local::LocalModel {
                url: "http://127.0.0.1:11434".into(),
                model: "llama3.2:3b".into(),
            })
        );
    }

    #[test]
    fn a_new_model_replaces_the_keys_of_the_old_one_and_other_keys_stay() {
        let text = "[story]\nmodel = \"claude\"\nclaude_model = \"haiku\"\n\
                    model_timeout_seconds = 30\n[wow]\nmodel = \"not the story\"\n";

        let new = with_story_model(text, &ollama("qwen2.5:3b"));

        assert_eq!(
            new,
            "[story]\nmodel = \"local\"\nlocal_url = \"http://127.0.0.1:11434\"\n\
             local_model = \"qwen2.5:3b\"\nmodel_timeout_seconds = 30\n\
             [wow]\nmodel = \"not the story\"\n"
        );
    }

    #[test]
    fn a_config_with_no_story_table_gets_one_with_the_model() {
        let new = with_story_model("[wow]\npath = \"~/wow\"\n", &FoundModel::Claude);

        assert_eq!(
            new,
            "[wow]\npath = \"~/wow\"\n\n[story]\nmodel = \"claude\"\nclaude_model = \"haiku\"\n"
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
