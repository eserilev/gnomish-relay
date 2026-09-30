//! The reply to a settings list: what the bridge allows, for the Settings and Diag tabs
//! of the game (SPEC.md 13.1). The game only reads it, and never writes the config.
//!
//! One line per value: `key \t value`. A value can hold more tabs, so a reader splits a
//! line at its first tab only. A last line `+` says that the list is cut. The reply
//! never holds a key, an `env` entry, or the command line of an agent.

use std::path::Path;

use protocol::lua::lua_string;
use protocol::slot::MAX_TEXT;

use crate::always_rules::{RuleLine, RuleList};
use crate::ci_checks::CiChecks;
use crate::config::{Policy, RelayConfig, StoryConfig};
use crate::folder_list::CUT;
use crate::folder_path::{is_inside_folder, native_folder, path_bytes, path_parts};
use crate::hooks_install::{self, HookFiles, HookState};
use crate::hooks_merge::agent_word;
use crate::model::ModelChoice;

/// The two quotes of the Lua literal, and the cut line after a newline.
const RESERVED: usize = 2 + 4 + CUT.len();

/// The values that do not change while the bridge runs. The levels of the agents come
/// from the policy at each reply, because a raise on the desktop changes them.
#[derive(Clone, Debug, Default)]
pub struct BridgeSettings {
    pub version: String,
    pub sandbox: String,
    pub default_cwd: String,
    pub roots: Vec<String>,
    /// The kind of each agent, by name.
    pub kinds: Vec<(String, String)>,
    pub timeout_minutes: u64,
    pub permission_timeout_minutes: u64,
    pub story: Option<StorySettings>,
    pub allow: Vec<String>,
    /// A folder of `[allow.folders]` and one of its patterns.
    pub allow_folders: Vec<(String, String)>,
    /// The "Always allow" rules, read at each reply (SPEC.md 6.6.5).
    pub rules: RuleList,
    /// The settings files of the agents that can hold our hooks, read at each reply,
    /// because `hooks install` runs while the bridge runs (SPEC.md 10.5).
    pub hooks: Vec<HookFiles>,
    /// `[git] ci_checks` (SPEC.md 9.10).
    pub ci_checks: CiChecks,
}

/// The state of our hooks for one agent: a `hook` line of the list.
pub type HookLine = (&'static str, HookState);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorySettings {
    pub model: String,
    pub budget_window_minutes: u32,
}

/// `~/Code` for a path in the home folder, as the player reads it. Both paths are in the
/// form of `path_bytes`, so a `\\?\` prefix on Windows makes no difference.
fn shown(path: &[u8], home: Option<&[u8]>) -> String {
    let Some(home) = home.filter(|h| is_inside_folder(path, h)) else {
        return String::from_utf8_lossy(path).into_owned();
    };
    let mut parts = vec![b"~".as_slice()];
    parts.extend(&path_parts(path)[path_parts(home).len()..]);
    String::from_utf8_lossy(&parts.join(&b'/')).into_owned()
}

/// A folder of the policy, in the form of the resolver.
fn shown_folder(resolved: &[u8], home: Option<&[u8]>) -> String {
    shown(&native_folder(resolved.to_vec(), cfg!(windows)), home)
}

/// The model name only: the address of a local model stays on the desktop.
fn model_word(choice: &ModelChoice) -> String {
    match choice {
        ModelChoice::None => "none".into(),
        ModelChoice::Claude { model: None, .. } => "claude".into(),
        ModelChoice::Claude {
            model: Some(model), ..
        } => format!("claude {model}"),
        ModelChoice::Local(local) => format!("local {}", local.model),
    }
}

impl BridgeSettings {
    pub fn from_config(
        relay: &RelayConfig,
        story: Option<&StoryConfig>,
        home: Option<&Path>,
        sandbox: String,
    ) -> BridgeSettings {
        let shown_home = home;
        let folders = &relay.policy.folders;
        let (allow, folder_rules) = relay.allow.patterns();
        let home = home.map(path_bytes);
        let home = home.as_deref();
        BridgeSettings {
            version: env!("CARGO_PKG_VERSION").into(),
            sandbox,
            default_cwd: shown_folder(&folders.base, home),
            roots: folders
                .roots
                .iter()
                .map(|r| shown_folder(r, home))
                .collect(),
            kinds: relay
                .agents
                .iter()
                .map(|(name, spec)| (name.clone(), spec.kind.word().to_owned()))
                .collect(),
            timeout_minutes: relay.timeout.as_secs() / 60,
            permission_timeout_minutes: relay.permission_timeout.as_secs() / 60,
            story: story.map(|s| StorySettings {
                model: model_word(&s.model.choice),
                budget_window_minutes: s.model.budget_window_minutes,
            }),
            allow,
            allow_folders: folder_rules
                .into_iter()
                .map(|(folder, rule)| (shown(&path_bytes(&folder), home), rule))
                .collect(),
            rules: RuleList {
                roots: folders
                    .roots
                    .iter()
                    .map(|r| std::path::PathBuf::from(String::from_utf8_lossy(r).into_owned()))
                    .collect(),
                home: shown_home.map(Path::to_owned).unwrap_or_default(),
                ..RuleList::default()
            },
            hooks: Vec::new(),
            ci_checks: relay.ci_checks.clone(),
        }
    }

    pub fn hook_lines(&self) -> Vec<HookLine> {
        let state = |f: &HookFiles| (agent_word(f.source), hooks_install::state(f));
        self.hooks.iter().map(state).collect()
    }

    /// Each line is a key and its fields. The reply joins the fields with tabs.
    fn lines(
        &self,
        policy: &Policy,
        rules: &[RuleLine],
        hooks: &[HookLine],
    ) -> Vec<(&'static str, Vec<String>)> {
        let one = |key, value: &str| (key, vec![value.to_owned()]);
        let mut lines = vec![
            one("version", &self.version),
            one("sandbox", &self.sandbox),
            one("default_cwd", &self.default_cwd),
        ];
        lines.extend(self.roots.iter().map(|r| one("allowed_root", r)));
        lines.push(one("default_agent", &policy.default_agent));
        for (name, level) in &policy.agents {
            let fields = vec![name.clone(), self.kind_of(name), level.word().to_owned()];
            lines.push(("agent", fields));
        }
        lines.push(one("timeout_minutes", &self.timeout_minutes.to_string()));
        let permission = self.permission_timeout_minutes.to_string();
        lines.push(one("permission_timeout_minutes", &permission));
        if let Some(story) = &self.story {
            lines.push(one("story_model", &story.model));
            let window = story.budget_window_minutes.to_string();
            lines.push(one("story_budget_window_minutes", &window));
        }
        for (agent, state) in hooks {
            lines.push(("hook", vec![(*agent).to_owned(), state.word().to_owned()]));
        }
        for rule in rules {
            let fields = vec![
                rule.id.clone(),
                rule.folder.clone(),
                rule.pattern.clone(),
                rule.days.to_string(),
            ];
            lines.push(("rule", fields));
        }
        lines.extend(self.allow.iter().map(|p| one("allow", p)));
        for (folder, pattern) in &self.allow_folders {
            lines.push(("allow_folder", vec![folder.clone(), pattern.clone()]));
        }
        lines
    }

    fn kind_of(&self, name: &str) -> String {
        let kind = self.kinds.iter().find(|(n, _)| n == name);
        kind.map_or_else(String::new, |(_, k)| k.clone())
    }
}

/// A tab or a line break inside a field would break the lines of the list.
fn field(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The size of a line in the slot file, with the newline before it.
fn cost(line: &str) -> usize {
    lua_string(format!("\n{line}").as_bytes()).len() - 2
}

/// Keeps the first lines that fit in one reply record (S12). The "Always allow" rules and
/// then the allow table come last, because only they can be long. A cut then drops allow
/// patterns first.
pub fn settings_reply(
    settings: &BridgeSettings,
    policy: &Policy,
    rules: &[RuleLine],
    hooks: &[HookLine],
) -> String {
    let mut kept: Vec<String> = Vec::new();
    let mut size = RESERVED;
    for (key, fields) in settings.lines(policy, rules, hooks) {
        let fields: Vec<String> = fields.iter().map(|f| field(f)).collect();
        let line = format!("{key}\t{}", fields.join("\t"));
        if size + cost(&line) > MAX_TEXT {
            kept.push(CUT.into());
            break;
        }
        size += cost(&line);
        kept.push(line);
    }
    kept.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Permission;
    use crate::relay::Folders;

    fn policy() -> Policy {
        Policy {
            folders: Folders {
                roots: vec![b"/home/x/Code".to_vec()],
                base: b"/home/x/Code".to_vec(),
            },
            agents: [
                ("claude".to_owned(), Permission::AutoEdit),
                ("codex".to_owned(), Permission::Ask),
            ]
            .into(),
            default_agent: "claude".into(),
        }
    }

    fn settings() -> BridgeSettings {
        BridgeSettings {
            version: "0.1.0".into(),
            sandbox: "bwrap".into(),
            default_cwd: "~/Code".into(),
            roots: vec!["~/Code".into()],
            kinds: vec![("claude".into(), "claude".into())],
            timeout_minutes: 30,
            permission_timeout_minutes: 10,
            story: None,
            allow: vec!["cargo test".into()],
            allow_folders: vec![("~/Code/app".into(), "npm test".into())],
            rules: RuleList::default(),
            hooks: Vec::new(),
            ci_checks: CiChecks::Off,
        }
    }

    #[test]
    fn the_hook_lines_come_before_the_rules_with_the_state_of_each_agent() {
        let hooks = [("claude", HookState::On), ("codex", HookState::Moved)];
        let reply = settings_reply(&settings(), &policy(), &[], &hooks);
        assert!(
            reply.contains(
                "permission_timeout_minutes\t10\nhook\tclaude\ton\nhook\tcodex\tmoved\nallow\t"
            ),
            "{reply}"
        );
    }

    #[test]
    fn the_hook_lines_read_the_settings_of_the_agents_at_each_reply() {
        let home = tempfile::tempdir().unwrap();
        let no_env = |_: &str| None;
        let mut with_hooks = settings();
        with_hooks.hooks = HookFiles::both(home.path(), &no_env);
        assert_eq!(
            with_hooks.hook_lines(),
            [("claude", HookState::Off), ("codex", HookState::Off)]
        );
        let program = home.path().join("gnomish-relay");
        std::fs::write(&program, b"").unwrap();
        hooks_install::install(&with_hooks.hooks[0], &program).unwrap();
        assert_eq!(with_hooks.hook_lines()[0], ("claude", HookState::On));
    }

    #[test]
    fn each_value_is_one_line_of_key_and_value() {
        let reply = settings_reply(&settings(), &policy(), &[], &[]);
        assert_eq!(
            reply,
            "version\t0.1.0\nsandbox\tbwrap\ndefault_cwd\t~/Code\nallowed_root\t~/Code\n\
             default_agent\tclaude\nagent\tclaude\tclaude\tauto-edit\nagent\tcodex\t\task\n\
             timeout_minutes\t30\npermission_timeout_minutes\t10\nallow\tcargo test\n\
             allow_folder\t~/Code/app\tnpm test"
        );
    }

    #[test]
    fn the_always_rules_come_right_before_the_allow_table() {
        let rules = [RuleLine {
            id: "a1b2".into(),
            folder: "Code/app".into(),
            pattern: "cargo test *".into(),
            days: 3,
        }];
        let reply = settings_reply(&settings(), &policy(), &rules, &[]);
        assert!(
            reply.contains("permission_timeout_minutes\t10\nrule\ta1b2\tCode/app\tcargo test *\t3\nallow\tcargo test"),
            "{reply}"
        );
    }

    #[test]
    fn the_rule_list_reads_the_file_at_each_reply() {
        let dir = tempfile::tempdir().unwrap();
        let list = RuleList {
            store: crate::always_rules::AlwaysRules::new(dir.path()),
            roots: vec![std::env::temp_dir().join("Code")],
            home: std::env::temp_dir(),
        };
        assert!(list.lines(1_790_000_000).is_empty());
        let rule = [vec!["make".to_owned()]];
        let folder = &std::env::temp_dir().join("Code").join("app");
        list.store
            .grant(
                folder,
                crate::always_rules::Scope::Tree,
                &rule,
                1_790_000_000,
            )
            .unwrap();
        let lines = list.lines(1_790_000_000);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].folder, "Code/app");
        assert_eq!(lines[0].pattern, "make *");
        assert_eq!(lines[0].days, 0);
    }

    #[test]
    fn the_story_lines_come_only_with_a_story_section() {
        let mut with_story = settings();
        with_story.story = Some(StorySettings {
            model: "claude haiku".into(),
            budget_window_minutes: 20,
        });
        let reply = settings_reply(&with_story, &policy(), &[], &[]);
        assert!(reply.contains("\nstory_model\tclaude haiku\nstory_budget_window_minutes\t20\n"));
        assert!(!settings_reply(&settings(), &policy(), &[], &[]).contains("story"));
    }

    #[test]
    fn a_raised_level_shows_in_the_next_reply() {
        let mut policy = policy();
        policy.agents.insert("codex".into(), Permission::AutoEdit);
        assert!(
            settings_reply(&settings(), &policy, &[], &[]).contains("agent\tcodex\t\tauto-edit")
        );
    }

    #[test]
    fn a_control_character_in_a_value_becomes_a_space() {
        let mut odd = settings();
        odd.roots = vec!["~/a\nb\tc".into()];
        let reply = settings_reply(&odd, &policy(), &[], &[]);
        assert!(reply.contains("\nallowed_root\t~/a b c\n"), "{reply}");
        let mut folder = settings();
        folder.allow_folders = vec![("~/a\tb".into(), "make".into())];
        let reply = settings_reply(&folder, &policy(), &[], &[]);
        assert!(reply.ends_with("\nallow_folder\t~/a b\tmake"), "{reply}");
    }

    #[test]
    fn a_long_allow_table_is_cut_to_fit_one_record_with_a_mark() {
        let mut long = settings();
        long.allow = (0..5000).map(|n| format!("tool{n} run")).collect();
        let reply = settings_reply(&long, &policy(), &[], &[]);
        assert!(reply.ends_with("\n+"));
        assert!(lua_string(reply.as_bytes()).len() <= MAX_TEXT);
        assert!(reply.starts_with("version\t0.1.0\n"));
    }

    #[test]
    fn a_path_in_the_home_folder_starts_with_a_tilde() {
        let home = Some(b"/home/x".as_slice());
        assert_eq!(shown(b"/home/x/Code", home), "~/Code");
        assert_eq!(shown(b"/home/x", home), "~");
        assert_eq!(shown(b"/home/xy", home), "/home/xy");
        assert_eq!(shown(b"/srv/work", home), "/srv/work");
        assert_eq!(shown(b"/srv/work", None), "/srv/work");
        let drive = Some(b"C:/Users/x".as_slice());
        assert_eq!(shown(b"C:/Users/x/Code", drive), "~/Code");
    }

    #[test]
    fn the_model_shows_its_name_but_never_the_address_of_a_local_model() {
        let local = ModelChoice::Local(crate::model_local::LocalModel {
            url: "http://127.0.0.1:11434".into(),
            model: "llama3.2".into(),
        });
        assert_eq!(model_word(&local), "local llama3.2");
        assert_eq!(model_word(&ModelChoice::None), "none");
        let claude = ModelChoice::Claude {
            command: vec!["claude".into()],
            model: None,
        };
        assert_eq!(model_word(&claude), "claude");
    }

    #[test]
    fn a_config_gives_its_values_but_no_command_line_and_no_env() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("code")).unwrap();
        let text = r#"
            allowed_roots = ["~/code"]
            default_agent = "claude"
            [wow]
            path = "~/wow"
            [agents.claude]
            kind = "claude"
            command = ["claude", "--secret-flag"]
            env = ["ANTHROPIC_API_KEY"]
            permission = "ask"
            [allow]
            commands = ["cargo test *"]
            [story]
            model = "claude"
        "#;
        let config = crate::config::parse(text, home.path()).unwrap();
        let relay = config.require_relay().unwrap();
        let home_path = home.path().canonicalize().unwrap();
        let settings = BridgeSettings::from_config(
            relay,
            config.story.as_ref(),
            Some(&home_path),
            "none".into(),
        );
        let reply = settings_reply(&settings, &relay.policy, &[], &[]);
        assert!(reply.contains("\nallowed_root\t~/code\n"), "{reply}");
        assert!(reply.contains("\nagent\tclaude\tclaude\task\n"), "{reply}");
        assert!(reply.ends_with("\nallow\tcargo test"), "{reply}");
        assert!(reply.contains("\nstory_model\tclaude\n"), "{reply}");
        assert!(!reply.contains("secret-flag"), "{reply}");
        assert!(!reply.contains("ANTHROPIC"), "{reply}");
    }
}
