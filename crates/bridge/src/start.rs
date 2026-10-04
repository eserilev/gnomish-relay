//! `gnomish-relay run`: the config, the keys, a repair of the key addons, and the
//! agents, then the run loop.

use std::path::PathBuf;

use anyhow::Result;

use crate::agent;
use crate::app_files::private_game_paths;
use crate::auto_update::{AutoUpdater, Parts};
use crate::background::Background;
use crate::build_kind::BuildKind;
use crate::config::{self, FullAuto, RelayConfig, StoryConfig};
use crate::desktop::Prompt;
use crate::dirs::Dirs;
use crate::folder_path::real_path;
use crate::fs_safe::make_private_dir;
use crate::full_auto::FullAutoAsker;
use crate::game_choice::NO_WOW;
use crate::game_folders::GameFolders;
use crate::game_watch::GameWatch;
use crate::gate::{Gate, Places};
use crate::hooks_install::files_for_bridge;
use crate::install;
use crate::lock;
use crate::logging;
use crate::lore_job::{LoreJob, LoreParts};
use crate::raise::Raiser;
use crate::receive::{KeySet, RELAY_KEY_FILE};
use crate::run::{Paths, RelayParts, run};
use crate::settings_list::BridgeSettings;
use crate::setup;
use crate::story::StorySpec;
use crate::trust::Truster;
use crate::wow_client::served_games;

/// Holds the lock of the bridge until the run loop ends. With no game yet, it says so
/// and ends with success, so the login service does not start it again and again.
pub fn start(dirs: &Dirs) -> Result<()> {
    let config = config::load(&dirs.config, &dirs.home)?;
    let games = config.games();
    if games.is_empty() {
        println!("{NO_WOW}");
        return Ok(());
    }
    let state = dirs.data.clone();
    make_private_dir(&state)?;
    let _lock = lock::take(&state)?;
    logging::start(&state);
    let paths = Paths {
        state,
        config: dirs.config.clone(),
        games,
    };
    // Equal keys, or a `timeways.key` that does not load, stop the bridge here.
    let keys = KeySet::load(&dirs.config)?;
    // Never a file of the Timeways addon but an old `Key.lua` (SPEC.md 9.7, decision 15).
    for game in &paths.games {
        if let Some(changed) = setup::repair_timeways_key(&dirs.config, &game.addons)? {
            print_changed("Timeways", changed);
        }
    }
    // A game installed after setup has no slots yet (SPEC.md 7.9).
    let products = setup::products_of(&config, &dirs.config);
    for game in &paths.games {
        setup::install_missing_slots(&game.addons, &products)?;
    }
    let auto_update = auto_updater(dirs, &config, BuildKind::THIS)?;
    let lore = lore_job(dirs, &config);
    let relay = match config.relay {
        Some(relay) => Some(start_relay(dirs, relay, config.story.as_ref(), &paths)?),
        None => None,
    };
    let story = match &config.story {
        Some(story) => story_spec(dirs, story, &paths)?,
        None => None,
    };
    let game_watch = config
        .wow
        .as_deref()
        .map(|wow| GameWatch::new(wow, served_games(wow)));
    let background = Background::new(auto_update, game_watch, lore);
    run(paths, relay, keys, story, background)
}

/// The private files of every served game: no agent and no command reads them.
fn private_paths(games: &[GameFolders]) -> Vec<PathBuf> {
    games
        .iter()
        .flat_map(|g| private_game_paths(&g.addons, &g.accounts, &g.screenshots))
        .collect()
}

/// The lore build of Timeways, with a story program in the config (SPEC.md 11.4).
fn lore_job(dirs: &Dirs, config: &config::Config) -> Option<LoreJob> {
    let story = config.story.as_ref()?.program.as_ref()?;
    Some(LoreJob::new(LoreParts::of(
        &story.program,
        &story.lore_pack,
        &dirs.data,
    )))
}

/// A build from source never updates itself (SPEC.md 11.3).
fn auto_updater(
    dirs: &Dirs,
    config: &config::Config,
    build: BuildKind,
) -> Result<Option<AutoUpdater>> {
    if !build.manages_itself() {
        crate::run::log("auto-update is off: this is a build from source");
        return Ok(None);
    }
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
    for game in &paths.games {
        print_changed(
            "Gnomish Relay",
            install::write_relay_keys(&game.addons, hex.trim())?,
        );
    }
    let places = Places {
        config_dir: &dirs.config,
        data_dir: &paths.state,
        home: &dirs.home,
    };
    let mut gate = Gate::new(&relay, &places, Prompt::Dialog);
    gate.sandbox = gate.sandbox.with_game(private_paths(&paths.games));
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
    use std::path::Path;

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
        let forever = GameFolders::of(&home.path().join("wow/_classic_beta_"));
        let anniversary = GameFolders::of(&home.path().join("wow/_anniversary_"));
        let paths = Paths {
            games: vec![forever, anniversary],
            state: dirs.data.clone(),
            config: dirs.config.clone(),
        };

        let parts = start_relay(&dirs, config.relay.unwrap(), None, &paths).unwrap();

        for game in &paths.games {
            let key_addon = game.addons.join("GnomishRelay_Key");
            assert!(key_addon.join(install::KEY_FILE).is_file());
            assert!(!game.addons.join(install::ADDON).exists());
        }
        assert_eq!(parts.policy.default_agent, "echo");
        assert!(parts.raiser.free_commands.is_empty());
        assert_eq!(parts.max_parallel_runs, 3);
        let full_auto = parts.full_auto.unwrap();
        assert!(full_auto.agents.is_empty(), "echo has no command sandbox");
    }

    #[test]
    fn the_private_files_of_every_served_game_are_hidden() {
        let forever = GameFolders::of(Path::new("/wow/_classic_beta_"));
        let anniversary = GameFolders::of(Path::new("/wow/_anniversary_"));

        let private = private_paths(&[forever.clone(), anniversary.clone()]);

        for game in [&forever, &anniversary] {
            assert!(private.contains(&game.addons.join("GnomishRelay_Key")));
            assert!(private.contains(&game.accounts));
            assert!(private.contains(&game.screenshots));
        }
    }

    #[test]
    fn a_build_from_source_never_updates_itself() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("Code")).unwrap();
        let dirs = Dirs {
            home: home.path().to_owned(),
            config: home.path().join("config"),
            data: home.path().join("data"),
        };
        let config = setup::write_config(&dirs.config, CONFIG, &dirs.home).unwrap();

        let source = auto_updater(&dirs, &config, BuildKind::Source).unwrap();
        let release = auto_updater(&dirs, &config, BuildKind::Release).unwrap();

        assert!(source.is_none());
        assert!(release.is_some());
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
