//! `gnomish-relay run`: the config, the keys, a repair of the key addons, and the
//! agents, then the run loop.

use anyhow::Result;

use crate::agent;
use crate::app_files::private_game_paths;
use crate::auto_update::{AutoUpdater, Parts};
use crate::config::{self, FullAuto, RelayConfig, StoryConfig};
use crate::desktop::Prompt;
use crate::dirs::Dirs;
use crate::folder_path::real_path;
use crate::fs_safe::make_private_dir;
use crate::full_auto::FullAutoAsker;
use crate::game_choice::NO_WOW;
use crate::gate::{Gate, Places};
use crate::hooks_install::files_for_bridge;
use crate::install;
use crate::lock;
use crate::logging;
use crate::raise::Raiser;
use crate::receive::{KeySet, RELAY_KEY_FILE};
use crate::run::{Paths, RelayParts, run};
use crate::settings_list::BridgeSettings;
use crate::setup;
use crate::story::StorySpec;
use crate::trust::Truster;

/// Holds the lock of the bridge until the run loop ends. With no game yet, it says so
/// and ends with success, so the login service does not start it again and again.
pub fn start(dirs: &Dirs) -> Result<()> {
    let config = config::load(&dirs.config, &dirs.home)?;
    let Some(wow) = config.wow.as_deref() else {
        println!("{NO_WOW}");
        return Ok(());
    };
    let state = dirs.data.clone();
    make_private_dir(&state)?;
    let _lock = lock::take(&state)?;
    logging::start(&state);
    let paths = Paths {
        state,
        config: dirs.config.clone(),
        screenshots: wow.join("Screenshots"),
        accounts: wow.join("WTF").join("Account"),
        addons: install::addons_dir(wow),
    };
    // Equal keys, or a `timeways.key` that does not load, stop the bridge here.
    let keys = KeySet::load(&dirs.config)?;
    // Never a file of the Timeways addon but an old `Key.lua` (SPEC.md 9.7, decision 15).
    if let Some(changed) = setup::repair_timeways_key(&dirs.config, &paths.addons)? {
        print_changed("Timeways", changed);
    }
    let auto_update = auto_updater(dirs, &config)?;
    let relay = match config.relay {
        Some(relay) => Some(start_relay(dirs, relay, config.story.as_ref(), &paths)?),
        None => None,
    };
    let story = match &config.story {
        Some(story) => story_spec(dirs, story, &paths)?,
        None => None,
    };
    run(paths, relay, keys, story, auto_update)
}

fn auto_updater(dirs: &Dirs, config: &config::Config) -> Result<Option<AutoUpdater>> {
    let Some(parts) = Parts::of(dirs, config) else {
        return Ok(None);
    };
    Ok(Some(AutoUpdater::new(parts, std::env::current_exe()?)))
}

/// The key addon comes back when it is missing. `CurseForge` owns the relay addon (SPEC.md 11.3).
pub fn start_relay(
    dirs: &Dirs,
    relay: RelayConfig,
    story: Option<&StoryConfig>,
    paths: &Paths,
) -> Result<RelayParts> {
    let hex = std::fs::read_to_string(dirs.config.join(RELAY_KEY_FILE))?;
    print_changed(
        "Gnomish Relay",
        install::write_relay_keys(&paths.addons, hex.trim())?,
    );
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
    println!("Commands from WoW run in: {sandbox}");
    println!("Agents from WoW run in: {}", gate.wall.summary());
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
    let full_auto = full_auto_asker(&relay, &gate);
    let truster = Truster {
        approvals: gate.approvals.clone(),
        config_dir: dirs.config.clone(),
        home: real_path(&dirs.home).unwrap_or_else(|_| dirs.home.clone()),
        permission_timeout: relay.permission_timeout,
        roots: gate.roots.clone(),
    };
    let mut settings = BridgeSettings::from_config(&relay, story, Some(&dirs.home), sandbox);
    settings.rules.store = gate.always.clone();
    let var = |name: &str| std::env::var_os(name).map(std::path::PathBuf::from);
    settings.hooks = files_for_bridge(&dirs.home, &dirs.data, &var);
    Ok(RelayParts {
        policy: relay.policy,
        agents,
        raiser,
        full_auto,
        truster,
        settings,
        max_parallel_runs: relay.max_parallel_runs,
        daily_cost_cap_usd: relay.daily_cost_cap_usd,
    })
}

/// Full-auto keeps "It stays in the sandbox" only for Claude with a command sandbox
/// (SPEC.md 9.3). `allow_full_auto = false` gives no asker at all.
fn full_auto_asker(relay: &RelayConfig, gate: &Gate) -> Option<FullAutoAsker> {
    if relay.full_auto == FullAuto::Off {
        return None;
    }
    let agents = if gate.sandbox.is_on() {
        relay
            .agents
            .iter()
            .filter(|(_, spec)| spec.kind == config::Kind::Claude)
            .map(|(name, _)| name.clone())
            .collect()
    } else {
        Vec::new()
    };
    Some(FullAutoAsker {
        approvals: gate.approvals.clone(),
        permission_timeout: relay.permission_timeout,
        agents,
    })
}

/// WoW finds a new addon folder only at launch (SPEC.md 7.2, rule 1).
fn print_changed(title: &str, changed: install::Installed) {
    match changed {
        install::Installed::New => println!("Updated {title}. Restart WoW to load it"),
        install::Installed::Updated => println!("Updated {title}. Type /reload in WoW"),
        install::Installed::Unchanged => {}
    }
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
    fn a_relay_start_writes_the_key_addon_and_no_relay_addon_and_keeps_the_policy_of_the_config() {
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

        let parts = start_relay(&dirs, config.relay.unwrap(), None, &paths).unwrap();

        let key_addon = paths.addons.join("GnomishRelay_Key");
        assert!(key_addon.join(install::KEY_FILE).is_file());
        assert!(!paths.addons.join(install::ADDON).exists());
        assert_eq!(parts.policy.default_agent, "echo");
        assert!(parts.raiser.free_commands.is_empty());
        assert_eq!(parts.max_parallel_runs, 3);
        let full_auto = parts.full_auto.unwrap();
        assert!(full_auto.agents.is_empty(), "echo has no command sandbox");
    }

    #[test]
    fn allow_full_auto_false_gives_no_full_auto_at_all() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("Code")).unwrap();
        let text = CONFIG.replace("default_agent", "allow_full_auto = false\ndefault_agent");
        let config = setup::write_config(&home.path().join("config"), &text, home.path()).unwrap();
        let relay = config.relay.unwrap();
        let gate = Gate::bare(Vec::new(), home.path().join("c"), home.path().join("d"));

        assert!(full_auto_asker(&relay, &gate).is_none());
    }
}
