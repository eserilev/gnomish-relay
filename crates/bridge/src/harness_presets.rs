//! The arguments of well-known harnesses for a `command` agent (SPEC.md 9.2), so an entry
//! needs one line: `preset = "aider"`. Checked against the docs of each tool on
//! 2026-09-27; a live test with the real tool is still to do.

pub struct Preset {
    pub name: &'static str,
    pub program: &'static str,
    /// After the `command` of the entry.
    pub args: &'static [&'static str],
    /// At `ask`, where the chat folder is read-only.
    pub ask_args: &'static [&'static str],
    /// When the chat goes on. Empty: each message is a fresh run.
    pub resume: &'static [&'static str],
    /// The model hosts for `agent_network = "strict"`.
    pub hosts: &'static [&'static str],
}

pub const PRESETS: [Preset; 5] = [
    Preset {
        name: "aider",
        program: "aider",
        args: &[
            "--message-file={prompt_file}",
            "--yes-always",
            "--no-pretty",
            "--no-stream",
            "--no-fancy-input",
            "--no-check-update",
            "--no-show-model-warnings",
            "--analytics-disable",
        ],
        ask_args: &["--chat-mode=ask", "--dry-run", "--no-auto-commits"],
        // Aider keeps its history in the chat folder, which the sandbox keeps.
        resume: &["--restore-chat-history"],
        // The model decides the host, so the entry names it in `agent_hosts`.
        hosts: &[],
    },
    Preset {
        name: "gemini",
        program: "gemini",
        args: &["--prompt={prompt}", "--approval-mode=yolo"],
        ask_args: &[],
        resume: &[],
        hosts: &[
            "generativelanguage.googleapis.com",
            "cloudcode-pa.googleapis.com",
            "oauth2.googleapis.com",
        ],
    },
    Preset {
        name: "opencode",
        program: "opencode",
        args: &["run", "--auto", "{prompt}"],
        ask_args: &[],
        resume: &[],
        hosts: &[],
    },
    Preset {
        name: "goose",
        program: "goose",
        args: &["run", "-i", "-", "-q", "--no-session"],
        ask_args: &[],
        resume: &[],
        hosts: &[],
    },
    Preset {
        name: "llm",
        program: "llm",
        args: &["--no-log"],
        ask_args: &[],
        resume: &[],
        hosts: &["api.openai.com"],
    },
];

pub fn find(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name == name)
}

pub fn names() -> String {
    let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
    names.join(", ")
}

pub fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| (*w).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_args::{Input, check_template, input_of};

    #[test]
    fn every_preset_has_a_good_template() {
        for preset in &PRESETS {
            let mut template = vec![preset.program.to_owned()];
            template.extend(words(preset.args));
            template.extend(words(preset.ask_args));
            assert!(check_template(&template).is_ok(), "{}", preset.name);
            for host in preset.hosts {
                assert!(crate::allow_hosts::check_host_name(host).is_ok(), "{host}");
            }
        }
    }

    #[test]
    fn presets_pass_the_message_in_the_way_of_each_tool() {
        let input = |name| {
            let preset = find(name).unwrap();
            let mut template = vec![preset.program.to_owned()];
            template.extend(words(preset.args));
            input_of(&template)
        };
        assert_eq!(input("aider"), Input::File);
        assert_eq!(input("gemini"), Input::Argument);
        assert_eq!(input("opencode"), Input::Argument);
        assert_eq!(input("goose"), Input::Stdin);
        assert_eq!(input("llm"), Input::Stdin);
    }

    #[test]
    fn an_unknown_preset_is_none_and_the_names_list_every_preset() {
        assert!(find("vim").is_none());
        assert_eq!(names(), "aider, gemini, opencode, goose, llm");
    }
}
