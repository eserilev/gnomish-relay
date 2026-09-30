//! `gnomish-relay run`: the config, the keys, a repair of the addon files, and the
//! agents, then the run loop.

use anyhow::Result;

use crate::agent::{self, Agents};
use crate::app_files::private_game_paths;
use crate::config::{self, Policy, RelayConfig, StoryConfig};
use crate::desktop::Prompt;
use crate::dirs::Dirs;
use crate::fs_safe::make_private_dir;
use crate::gate::{Gate, Places};
use crate::install;
use crate::lock;
use crate::raise::Raiser;
use crate::receive::{KeySet, RELAY_KEY_FILE};
use crate::run::{Paths, run};
use crate::settings_list::BridgeSettings;
use crate::setup;
use crate::story::StorySpec;

/// Holds the lock of the bridge until the run loop ends.
pub fn start(dirs: &Dirs) -> Result<()> {
    let config = config::load(&dirs.config, &dirs.home)?;
    let state = dirs.data.clone();
    make_private_dir(&state)?;
    let _lock = lock::take(&state)?;
    let paths = Paths {
        state,
        config: dirs.config.clone(),
        screenshots: config.wow.join("Screenshots"),
        accounts: config.wow.join("WTF").join("Account"),
        addons: install::addons_dir(&config.wow),
    };
    // Equal keys, or a `timeways.key` that does not load, stop the bridge here.
    let keys = KeySet::load(&dirs.config)?;
    // Only `Key.lua`, never another file of the Timeways addon (SPEC.md 9.7, decision 15).
    if setup::repair_timeways_key(&dirs.config, &paths.addons)? == Some(install::Installed::Updated)
    {
        println!("wrote the Timeways key again: type /reload in the game");
    }
    let relay = match config.relay {
        Some(relay) => Some(start_relay(dirs, relay, config.story.as_ref(), &paths)?),
        None => None,
    };
    let story = match &config.story {
        Some(story) => story_spec(dirs, story, &paths)?,
        None => None,
    };
    run(paths, relay, keys, story)
}

/// An addon app can replace the addon folder and drop the key (SPEC.md 11.3).
pub fn start_relay(
    dirs: &Dirs,
    relay: RelayConfig,
    story: Option<&StoryConfig>,
    paths: &Paths,
) -> Result<(Policy, Agents, Raiser, BridgeSettings)> {
    let hex = std::fs::read_to_string(dirs.config.join(RELAY_KEY_FILE))?;
    if install::install_addon(&paths.addons, hex.trim())? != install::Installed::Unchanged {
        println!("wrote the addon files again: type /reload in the game");
    }
    let places = Places {
        config_dir: &dirs.config,
        data_dir: &paths.state,
        home: &dirs.home,
    };
    let mut gate = Gate::new(&relay, &places, Prompt::Dialog);
    let private = private_game_paths(&paths.addons, &paths.accounts, &paths.screenshots);
    gate.sandbox = gate.sandbox.with_game(private);
    gate.approvals.clear();
    let sandbox = gate.sandbox.summary();
    println!("commands from the game run in: {sandbox}");
    println!("the agents of the game run in: {}", gate.wall.summary());
    let agents = agent::from_config(&relay, &gate);
    let raiser = Raiser {
        approvals: gate.approvals.clone(),
        config_dir: dirs.config.clone(),
        home: dirs.home.clone(),
        permission_timeout: relay.permission_timeout,
        free_commands: relay
            .agents
            .iter()
            .filter(|(_, spec)| spec.kind == config::Kind::Command)
            .map(|(name, _)| name.clone())
            .collect(),
    };
    let mut settings = BridgeSettings::from_config(&relay, story, Some(&dirs.home), sandbox);
    settings.rules.store = gate.always.clone();
    Ok((relay.policy, agents, raiser, settings))
}

fn story_spec(dirs: &Dirs, story: &StoryConfig, paths: &Paths) -> Result<Option<StorySpec>> {
    let spec = StorySpec::from_config(story, &dirs.config, &paths.state, &dirs.home)?;
    if spec.is_none() {
        eprintln!("timeways: [story] has no program, so the story program does not start");
    }
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "allowed_roots = [\"~/Code\"]\ndefault_agent = \"echo\"\n\
        [wow]\npath = \"~/wow\"\n[agents.echo]\nkind = \"echo\"\npermission = \"ask\"\n";

    #[test]
    fn a_relay_start_writes_the_addon_again_and_keeps_the_policy_of_the_config() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("Code")).unwrap();
        let dirs = Dirs {
            home: home.path().to_owned(),
            config: home.path().join("config"),
            data: home.path().join("data"),
        };
        let config = setup::write_config(&dirs.config, CONFIG, &dirs.home).unwrap();
        std::fs::write(dirs.config.join(RELAY_KEY_FILE), "ab".repeat(32)).unwrap();
        make_private_dir(&dirs.data).unwrap();
        let wow = home.path().join("wow");
        let paths = Paths {
            addons: install::addons_dir(&wow),
            screenshots: wow.join("Screenshots"),
            accounts: wow.join("WTF").join("Account"),
            state: dirs.data.clone(),
            config: dirs.config.clone(),
        };

        let (policy, _, raiser, _) =
            start_relay(&dirs, config.relay.unwrap(), None, &paths).unwrap();

        assert!(
            paths
                .addons
                .join(install::ADDON)
                .join(install::KEY_FILE)
                .is_file()
        );
        assert_eq!(policy.default_agent, "echo");
        assert!(raiser.free_commands.is_empty());
    }
}
