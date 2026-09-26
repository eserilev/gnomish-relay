//! A model call of the story program through `claude -p` with no tools (SPEC.md 9.7,
//! decision 10). Each call runs in a new empty private folder, and loads no settings,
//! no `CLAUDE.md`, no hooks, no skills, no plugins, and no MCP servers of the user.

use std::time::Duration;

use crate::agent::StopSignal;
use crate::claude;

/// Checked live on Claude Code 2.1.283: with these flags the `init` message lists no
/// tools, MCP servers, skills, slash commands, or plugins of the user, no hook of the
/// user runs, and a `CLAUDE.md` in a parent folder does not reach the model. `--bare`
/// keeps out the same, but it also skips the login of the user.
const NO_TOOLS_AND_NO_SETTINGS: [&str; 16] = [
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--permission-prompt-tool",
    "stdio",
    "--tools",
    "",
    "--strict-mcp-config",
    "--setting-sources",
    "",
    "--safe-mode",
    "--disable-slash-commands",
    "--no-session-persistence",
];

/// The prompt goes through stdin, never into the arguments.
pub fn args(model: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = NO_TOOLS_AND_NO_SETTINGS.map(str::to_owned).into();
    if let Some(model) = model {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    args
}

/// The folder has mode 0700 and goes away after the call.
pub fn ask(
    command: &[String],
    model: Option<&str>,
    prompt: &str,
    timeout: Duration,
    stop: StopSignal,
) -> Result<String, String> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("gnomish-relay-model-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let folder = builder
        .tempdir()
        .map_err(|e| format!("Cannot make a folder for the model: {e}"))?;
    let path = folder.path().to_string_lossy().into_owned();
    claude::answer_with_no_tools(command, &args(model), &path, prompt, timeout, stop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arguments_give_no_tools_and_load_no_settings_and_hold_no_prompt() {
        let args = args(Some("haiku"));
        let after = |flag: &str| {
            let at = args.iter().position(|a| a == flag).unwrap();
            args[at + 1].as_str()
        };
        assert_eq!(after("--tools"), "");
        assert_eq!(after("--setting-sources"), "");
        assert_eq!(after("--model"), "haiku");
        for flag in [
            "--strict-mcp-config",
            "--safe-mode",
            "--no-session-persistence",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag}");
        }
        assert!(
            !args.iter().any(|a| a == "--bare"),
            "--bare skips the login"
        );
    }

    #[test]
    fn with_no_model_the_default_model_of_claude_answers() {
        assert!(!args(None).iter().any(|a| a == "--model"));
    }
}
