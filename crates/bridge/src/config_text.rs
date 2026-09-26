//! The text of `config.toml` that setup writes (SPEC.md 11.3 and 12). Setup never
//! changes a key that exists: it writes a first config, or adds a missing part.

use std::fmt::Write;
use std::path::Path;

use crate::config::Found;
use crate::model_setup::{CLAUDE_MODEL, FoundModel};

/// The relay part of a config: the folders of the agents and the agents that setup found.
pub struct RelayPart<'a> {
    pub agents: &'a [Found<'a>],
    pub roots: &'a [String],
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// TOML needs the top keys before the first table.
fn relay_keys(relay: &RelayPart) -> String {
    let roots: Vec<String> = relay.roots.iter().map(|r| quote(r)).collect();
    let default = relay.agents.first().map_or("echo", |(name, _, _)| name);
    format!(
        "allowed_roots = [{}]\ndefault_agent = {}\n",
        roots.join(", "),
        quote(default)
    )
}

fn wow_table(wow: &Path) -> String {
    format!("[wow]\npath = {}\n", quote(&wow.to_string_lossy()))
}

/// Every agent asks. With no agent found, the echo agent.
fn relay_tables(relay: &RelayPart) -> String {
    let mut text = String::new();
    for (name, kind, command) in relay.agents {
        let command: Vec<String> = command.iter().map(|word| quote(word)).collect();
        let _ = write!(
            text,
            "\n[agents.{name}]\nkind = \"{}\"\ncommand = [{}]\npermission = \"ask\"\n",
            kind.word(),
            command.join(", ")
        );
    }
    text.push_str(
        "\n# Commands that run from the game with no question, at auto-edit and full-auto.\n\
         # A pattern covers more words after it. It never allows a command that the\n\
         # classifier refuses or sends to the desktop (SPEC.md 6.6.3).\n\
         # [allow]\n# commands = [\"cargo test *\", \"cargo fmt --check\"]\n\
         # [allow.folders]\n# \"~/Code/lighthouse\" = [\"npm test *\"]\n",
    );
    if relay.agents.is_empty() {
        text.push_str("\n[agents.echo]\nkind = \"echo\"\npermission = \"ask\"\n");
    }
    text.push_str(
        "\n# Any ACP agent is one entry. Run `gnomish-relay check-agent <name>` to test it.\n\
         # [agents.gemini]\n# kind = \"acp\"\n# command = [\"gemini\", \"--acp\"]\n\
         # permission = \"ask\"\n# env = [\"GEMINI_API_KEY\"]\n",
    );
    text
}

fn model_lines(model: &FoundModel, prefix: &str) -> String {
    match model {
        FoundModel::Claude => format!(
            "{prefix}model = \"claude\"\n{prefix}claude_model = {}\n",
            quote(CLAUDE_MODEL)
        ),
        FoundModel::Local { url, model } => format!(
            "{prefix}model = \"local\"\n{prefix}local_url = {}\n{prefix}local_model = {}\n",
            quote(url),
            quote(model)
        ),
    }
}

/// The first model that setup found runs, and the others wait as comments.
// TODO: find timeways-story when Timeways ships it.
pub fn story_table(models: &[FoundModel]) -> String {
    let mut text = String::from(
        "\n[story]\n\
         # The story program of Timeways and its lore pack: absolute paths, or ones that\n\
         # start with ~/. Set both when Timeways ships its program (SPEC.md 12).\n\
         # program = \"~/.local/bin/timeways-story\"\n\
         # lore_pack = \"~/.local/share/timeways/lore.sqlite\"\n",
    );
    let Some((first, others)) = models.split_first() else {
        text.push_str(
            "# No model found. Install claude, or start Ollama or LM Studio, then set:\n\
             # model = \"claude\"\n",
        );
        return text;
    };
    text.push_str(&model_lines(first, ""));
    for other in others {
        text.push_str("# Another model that setup found:\n");
        text.push_str(&model_lines(other, "# "));
    }
    text
}

/// The first config of a player with the relay.
pub fn relay_config(wow: &Path, relay: &RelayPart) -> String {
    format!(
        "{}\n{}{}",
        relay_keys(relay),
        wow_table(wow),
        relay_tables(relay)
    )
}

/// The first config of a player with only Timeways: no agents, no folders.
pub fn timeways_config(wow: &Path, models: &[FoundModel]) -> String {
    wow_table(wow) + &story_table(models)
}

/// A config with no relay part gets one: its keys first, its tables last.
pub fn with_relay(existing: &str, relay: &RelayPart) -> String {
    format!("{}\n{existing}{}", relay_keys(relay), relay_tables(relay))
}

pub fn with_story(existing: &str, models: &[FoundModel]) -> String {
    format!("{existing}{}", story_table(models))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Kind, Permission, parse};
    use crate::model::ModelChoice;
    use std::fs;
    use std::path::PathBuf;

    fn home() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join("Documents/Code")).unwrap();
        home
    }

    fn parsed(text: &str, home: &tempfile::TempDir) -> Config {
        parse(text, home.path()).unwrap_or_else(|e| panic!("{e:#}\n{text}"))
    }

    fn roots() -> Vec<String> {
        vec!["~/Documents/Code".to_owned()]
    }

    #[test]
    fn the_relay_config_parses_and_every_agent_asks() {
        let home = home();
        // A quote and a backslash in the folder name must not break the TOML string.
        let wow = if cfg!(windows) {
            r#"C:\Games\"wow""#
        } else {
            r#"/games/"wow"\x"#
        };
        let agents: [Found; 2] = [
            ("claude", Kind::Claude, &["claude"]),
            ("gemini", Kind::Acp, &["gemini", "--acp"]),
        ];
        let roots = roots();
        let relay = RelayPart {
            agents: &agents,
            roots: &roots,
        };
        let config = parsed(&relay_config(Path::new(wow), &relay), &home);
        let relay = config.require_relay().unwrap();
        assert_eq!(relay.policy.agents["claude"], Permission::Ask);
        assert_eq!(relay.policy.default_agent, "claude");
        assert_eq!(relay.agents["gemini"].command, ["gemini", "--acp"]);
        assert_eq!(relay.agents["claude"].kind, Kind::Claude);
        assert_eq!(config.wow, PathBuf::from(wow));
        assert!(config.story.is_none());
    }

    #[test]
    fn with_no_agent_found_the_relay_config_uses_echo() {
        let home = home();
        let roots = roots();
        let relay = RelayPart {
            agents: &[],
            roots: &roots,
        };
        let config = parsed(&relay_config(&home.path().join("wow"), &relay), &home);
        let relay = config.require_relay().unwrap();
        assert_eq!(relay.policy.default_agent, "echo");
        assert_eq!(relay.agents["echo"].kind, Kind::Echo);
    }

    #[test]
    fn the_timeways_config_has_no_relay_and_runs_the_first_model_found() {
        let home = home();
        let models = [
            FoundModel::Claude,
            FoundModel::Local {
                url: "http://127.0.0.1:11434".into(),
                model: "llama3.2".into(),
            },
        ];
        let text = timeways_config(&home.path().join("wow"), &models);
        let config = parsed(&text, &home);
        assert!(config.relay.is_none());
        let story = config.story.unwrap();
        assert_eq!(story.program, None);
        assert_eq!(
            story.model.choice,
            ModelChoice::Claude {
                command: vec!["claude".into()],
                model: Some("haiku".into()),
            }
        );
        assert!(text.contains("# local_model = \"llama3.2\""), "{text}");
    }

    #[test]
    fn a_local_model_found_alone_runs() {
        let home = home();
        let models = [FoundModel::Local {
            url: "http://127.0.0.1:1234".into(),
            model: "qwen3".into(),
        }];
        let config = parsed(&timeways_config(&home.path().join("wow"), &models), &home);
        let choice = config.story.unwrap().model.choice;
        assert!(matches!(choice, ModelChoice::Local(_)), "{choice:?}");
    }

    #[test]
    fn with_no_model_found_the_story_has_no_model() {
        let home = home();
        let text = timeways_config(&home.path().join("wow"), &[]);
        let config = parsed(&text, &home);
        assert_eq!(config.story.unwrap().model.choice, ModelChoice::None);
        assert!(text.contains("# model = \"claude\""));
    }

    #[test]
    fn a_relay_config_gets_a_story_and_keeps_every_key() {
        let home = home();
        let roots = roots();
        let relay = RelayPart {
            agents: &[],
            roots: &roots,
        };
        let old = relay_config(&home.path().join("wow"), &relay);
        let text = with_story(&old, &[FoundModel::Claude]);
        assert!(text.starts_with(&old));
        let config = parsed(&text, &home);
        assert!(config.relay.is_some());
        assert!(config.story.is_some());
    }

    #[test]
    fn a_timeways_config_gets_the_relay_and_keeps_its_story() {
        let home = home();
        let old = timeways_config(&home.path().join("wow"), &[FoundModel::Claude]);
        let roots = roots();
        let relay = RelayPart {
            agents: &[],
            roots: &roots,
        };
        let text = with_relay(&old, &relay);
        assert!(text.contains(&old));
        let config = parsed(&text, &home);
        assert_eq!(config.require_relay().unwrap().policy.default_agent, "echo");
        assert_ne!(config.story.unwrap().model.choice, ModelChoice::None);
    }
}
