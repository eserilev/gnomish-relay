//! The text of `config.toml` that setup writes (SPEC.md 11.3 and 12). Setup never
//! changes a key that exists: it writes a first config, or adds a missing part.

use std::fmt::Write;
use std::path::Path;

use crate::config::Found;
use crate::config_story::STORY_PATHS_NOTE;
use crate::model_setup::{CLAUDE_MODEL, FoundModel};

/// The relay part of a config: the folders of the agents and the agents that setup found.
pub struct RelayPart<'a> {
    pub agents: &'a [Found<'a>],
    /// The presets of the command-line harnesses that the player chose in setup.
    pub harnesses: &'a [&'a str],
    pub roots: &'a [String],
    /// The ports of the local models that setup found (`model_setup::local_ports`).
    pub local_ports: &'a [u16],
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// TOML needs the top keys before the first table.
fn relay_keys(relay: &RelayPart) -> String {
    let roots: Vec<String> = relay.roots.iter().map(|r| quote(r)).collect();
    let default = relay
        .agents
        .first()
        .map(|(name, _, _)| *name)
        .or_else(|| relay.harnesses.first().copied())
        .unwrap_or("echo");
    format!(
        "allowed_roots = [{}]\ndefault_agent = {}\n",
        roots.join(", "),
        quote(default)
    )
}

fn wow_table(wow: &Path) -> String {
    format!("[wow]\npath = {}\n", quote(&wow.to_string_lossy()))
}

fn agent_tables(agents: &[Found]) -> String {
    let mut text = String::new();
    for (name, kind, command) in agents {
        let command: Vec<String> = command.iter().map(|word| quote(word)).collect();
        let _ = write!(
            text,
            "\n[agents.{name}]\nkind = \"{}\"\ncommand = [{}]\npermission = \"auto-edit\"\n",
            kind.word(),
            command.join(", ")
        );
    }
    text
}

/// Every agent edits the chat folder with no question, and asks before each command.
/// With no agent found, the echo agent.
fn relay_tables(relay: &RelayPart) -> String {
    let mut text = agent_tables(relay.agents);
    for name in relay.harnesses {
        let _ = write!(
            text,
            "\n# It runs its own commands with no question, inside the sandbox (SPEC.md 9.2).\n\
             [agents.{name}]\nkind = \"command\"\npreset = \"{name}\"\npermission = \"auto-edit\"\n"
        );
    }
    text.push_str(
        "\n# Commands that run from the game with no question, at auto-edit and full-auto.\n\
         # A pattern covers more words after it. It never allows a command that the\n\
         # classifier refuses or sends to the desktop (SPEC.md 6.6.3).\n\
         # [allow]\n# commands = [\"cargo test *\", \"cargo fmt --check\"]\n\
         # [allow.folders]\n# \"~/Code/lighthouse\" = [\"npm test *\"]\n",
    );
    if relay.agents.is_empty() && relay.harnesses.is_empty() {
        text.push_str("\n[agents.echo]\nkind = \"echo\"\npermission = \"auto-edit\"\n");
    }
    text.push_str(
        "\n# Any ACP agent is one entry. Run `gnomish-relay check-agent <name>` to test it.\n\
         # [agents.gemini]\n# kind = \"acp\"\n# command = [\"gemini\", \"--acp\"]\n\
         # permission = \"auto-edit\"\n# env = [\"GEMINI_API_KEY\"]\n\
         # A harness with only a command line runs inside the sandbox (SPEC.md 9.2).\n\
         # [agents.aider]\n# kind = \"command\"\n# preset = \"aider\"\n\
         # permission = \"auto-edit\"\n# env = [\"OPENAI_API_KEY\"]\n",
    );
    text.push_str(&sandbox_table(relay.local_ports));
    text
}

/// The agents reach any public host through their proxy, and this computer only on the
/// ports of `local_ports`: here the local models that setup found (SPEC.md 6.6.4).
fn sandbox_table(local_ports: &[u16]) -> String {
    let mut text = String::from(
        "\n# The ports of this computer that the agents and their commands reach.\n\
         # agent_network = \"strict\" limits the agents to their model hosts.\n",
    );
    if local_ports.is_empty() {
        text.push_str("# [sandbox]\n# local_ports = [5432, 3000]\n");
        return text;
    }
    let ports: Vec<String> = local_ports.iter().map(u16::to_string).collect();
    let _ = write!(
        text,
        "# A local model that setup found listens on these.\n[sandbox]\nlocal_ports = [{}]\n",
        ports.join(", ")
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
pub fn story_table(models: &[FoundModel]) -> String {
    let [note, rest] = STORY_PATHS_NOTE;
    let mut text = format!(
        "\n[story]\n{note}\n{rest}\n\
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

/// The agents that a later setup found go last. A config with an inline `agents` table
/// cannot take a new `[agents.<name>]` table, so it stays as it is.
pub fn with_agents(existing: &str, agents: &[Found]) -> String {
    let text = format!(
        "{existing}\n# Agents that a later setup found.{}",
        agent_tables(agents)
    );
    if text.parse::<toml::Table>().is_err() {
        return existing.to_owned();
    }
    text
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
    fn the_port_of_a_local_model_that_setup_found_goes_into_local_ports() {
        let home = home();
        let agents: [Found; 1] = [("claude", Kind::Claude, &["claude"])];
        let roots = roots();
        let with_model = RelayPart {
            agents: &agents,
            harnesses: &[],
            roots: &roots,
            local_ports: &[11434],
        };
        let without = RelayPart {
            local_ports: &[],
            ..with_model
        };

        let found = parsed(&relay_config(&home.path().join("wow"), &with_model), &home);
        let none = parsed(&relay_config(&home.path().join("wow"), &without), &home);

        assert_eq!(found.require_relay().unwrap().local_ports, vec![11434]);
        assert!(none.require_relay().unwrap().local_ports.is_empty());
    }

    #[test]
    fn the_relay_config_parses_and_every_agent_edits_the_chat_folder_with_no_question() {
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
            harnesses: &[],
            roots: &roots,
            local_ports: &[],
        };
        let config = parsed(&relay_config(Path::new(wow), &relay), &home);
        let relay = config.require_relay().unwrap();
        assert_eq!(relay.policy.agents["claude"], Permission::AutoEdit);
        assert_eq!(relay.policy.agents["gemini"], Permission::AutoEdit);
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
            harnesses: &[],
            roots: &roots,
            local_ports: &[],
        };
        let config = parsed(&relay_config(&home.path().join("wow"), &relay), &home);
        let relay = config.require_relay().unwrap();
        assert_eq!(relay.policy.default_agent, "echo");
        assert_eq!(relay.agents["echo"].kind, Kind::Echo);
    }

    #[test]
    fn a_harness_that_the_player_chose_gets_a_preset_entry() {
        let home = home();
        let roots = roots();
        let relay = RelayPart {
            agents: &[],
            harnesses: &["aider"],
            roots: &roots,
            local_ports: &[],
        };
        let config = parsed(&relay_config(&home.path().join("wow"), &relay), &home);
        let relay = config.require_relay().unwrap();
        assert_eq!(relay.policy.default_agent, "aider");
        assert_eq!(relay.agents["aider"].kind, Kind::Command);
        assert_eq!(relay.agents["aider"].command[0], "aider");
        assert_eq!(relay.policy.agents["aider"], Permission::AutoEdit);
        assert!(!relay.agents.contains_key("echo"));
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
            harnesses: &[],
            roots: &roots,
            local_ports: &[],
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
            harnesses: &[],
            roots: &roots,
            local_ports: &[],
        };
        let text = with_relay(&old, &relay);
        assert!(text.contains(&old));
        let config = parsed(&text, &home);
        assert_eq!(config.require_relay().unwrap().policy.default_agent, "echo");
        assert_ne!(config.story.unwrap().model.choice, ModelChoice::None);
    }

    #[test]
    fn a_config_with_an_inline_agents_table_gets_no_new_agent() {
        let old = "agents = { echo = { kind = \"echo\", permission = \"ask\" } }\n";
        let codex: [Found; 1] = [("codex", Kind::Codex, &["codex"])];

        assert_eq!(with_agents(old, &codex), old);
    }
}
