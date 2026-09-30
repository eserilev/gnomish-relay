//! The `gnomish-relay` command.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bridge::agent;
use bridge::agent::Agents;
#[cfg(unix)]
use bridge::agent_wall;
use bridge::always_rules::AlwaysRules;
use bridge::app_files::private_game_paths;
use bridge::check_agent;
use bridge::command_sandbox;
use bridge::config::{self, Config, Policy, RelayConfig, StoryConfig};
use bridge::desktop::{self, Approvals, Prompt};
use bridge::dirs::Dirs;
#[cfg(unix)]
use bridge::forward;
use bridge::gate::{Gate, Places};
use bridge::install;
use bridge::lock;
use bridge::raise::Raiser;
use bridge::receive::{KeySet, RELAY_KEY_FILE};
use bridge::run::{Paths, now, run};
use bridge::selftest;
use bridge::service;
use bridge::settings_list::BridgeSettings;
use bridge::setup;
use bridge::setup_command;
use bridge::slots::{self, Files};
use bridge::status;
use bridge::story::StorySpec;
use bridge::update;
use protocol::apps::App;
use protocol::slot::{Reply, Status, prepare_replies, slot_body};

const USAGE: &str = "\
usage:
  gnomish-relay setup [folder] [--roots a,b] [--relay] [--new-key] [--autostart]
                                     install the addons, the keys, the config, and the slots
  gnomish-relay install              make the slot addons (game closed)
  gnomish-relay run                  read strips, run the agents, publish the replies
  gnomish-relay restart              stop the bridge and start it again, for example after a config edit
  gnomish-relay status               show whether the bridge runs, the config, the sandbox, and the agent
  gnomish-relay update               install the latest release and restart the bridge
  gnomish-relay check-agent <name>   start an agent of the config and show what it offers
  gnomish-relay approve [id]         list the tool calls that wait for the desktop, or allow one
  gnomish-relay deny <id>            refuse a tool call that waits for the desktop
  gnomish-relay rules                list the Always allow rules from the game
  gnomish-relay rules remove <id>    remove one Always allow rule
  gnomish-relay say <chat> <id> <text>
                                     publish a reply to message <id> (from `/relay diag`)
  gnomish-relay selftest collect [folder] [--out <repo>]
                                     copy the results of the self-test addon into the repo (developers)";

fn load_config(dirs: &Dirs) -> Result<Config> {
    config::load(&dirs.config, &dirs.home)
}

fn addons_dir(wow: &Path) -> PathBuf {
    install::addons_dir(wow)
}

fn body(replies: &[Reply]) -> Vec<u8> {
    slot_body(App::Relay, now(), &prepare_replies(replies))
}

fn say(dirs: &Dirs, chat: &str, id: &str, text: &str) -> Result<()> {
    let reply = Reply {
        chat: chat.as_bytes().to_vec(),
        id: id.parse().context("the message id is a number")?,
        status: Status::Done,
        text: text.as_bytes().to_vec(),
    };
    // `say` has no strip to learn the slot from, so the user gives it: `/relay diag` shows it.
    let next = match std::env::var("GNOMISH_NEXT_SLOT") {
        Ok(n) => n
            .parse()
            .context("GNOMISH_NEXT_SLOT is not a slot number")?,
        Err(_) => 1,
    };
    let config = load_config(dirs)?;
    config.require_relay()?;
    let addons = addons_dir(&config.wow);
    let files = Files {
        body: body(&[reply]),
        ..Files::empty(App::Relay, now())
    };
    slots::publish(&addons, App::Relay, &files, next)?;
    println!(
        "published to {} slots from slot {next}",
        protocol::slot::SLOT_WINDOW
    );
    Ok(())
}

/// `selftest collect [folder] [--out <repo>]`. It needs no config and no key, so it
/// finds the game as setup does.
fn selftest_collect(dirs: &Dirs, args: &[&str]) -> Result<()> {
    let (game, out) = match args {
        [] => (None, None),
        ["--out", out] => (None, Some(*out)),
        [game] => (Some(*game), None),
        [game, "--out", out] => (Some(*game), Some(*out)),
        _ => bail!("{USAGE}"),
    };
    let repo = match out {
        Some(out) => PathBuf::from(out),
        None => std::env::current_dir()?,
    };
    if !repo.join("addon").join("GnomishRelaySelfTest").is_dir() {
        bail!("run this in the gnomish-relay repo, or give --out <repo>");
    }
    let collected = selftest::collect(&setup_command::pick_game(dirs, game)?, &repo)?;
    println!("wrote {}", collected.fixture.display());
    println!(
        "wrote {} golden vectors to tests/vectors/{}",
        collected.vectors, collected.build
    );
    for name in &collected.missing {
        println!("no screenshot of {name}");
    }
    println!("capture: {:?}", collected.capture);
    Ok(())
}

fn install(dirs: &Dirs) -> Result<()> {
    let config = load_config(dirs)?;
    let dir = addons_dir(&config.wow);
    let relay = match config.relay {
        Some(_) => setup::Relay::On,
        None => setup::Relay::Off,
    };
    for app in setup::install_all_slots(&dir, relay)? {
        println!(
            "made {} slots of {app:?} in {}",
            protocol::slot::SLOTS,
            dir.display()
        );
    }
    Ok(())
}

fn start(dirs: &Dirs) -> Result<()> {
    let config = load_config(dirs)?;
    let state = dirs.data.clone();
    bridge::fs_safe::make_private_dir(&state)?;
    let _lock = lock::take(&state)?;
    let paths = Paths {
        state,
        config: dirs.config.clone(),
        screenshots: config.wow.join("Screenshots"),
        accounts: config.wow.join("WTF").join("Account"),
        addons: addons_dir(&config.wow),
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
fn start_relay(
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

fn print_status(dirs: &Dirs) -> Result<()> {
    let places = Places {
        config_dir: &dirs.config,
        data_dir: &dirs.data,
        home: &dirs.home,
    };
    std::fs::create_dir_all(places.data_dir)?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    for line in status::status_lines(&places, &path, now()) {
        println!("{line}");
    }
    Ok(())
}

fn approvals(dirs: &Dirs) -> Approvals {
    Approvals::new(&dirs.data, Prompt::Off)
}

/// Lists the tool calls that wait for the desktop (SPEC.md 6.6.3).
fn list_approvals(dirs: &Dirs) {
    let pending = approvals(dirs).list();
    if pending.is_empty() {
        println!("no tool call waits for the desktop");
    }
    for line in pending.iter().flat_map(|p| p.lines(now())) {
        println!("{line}");
    }
}

fn answer_approval(dirs: &Dirs, id: &str, verdict: desktop::Verdict) -> Result<()> {
    approvals(dirs).answer(id, verdict)?;
    println!("answered {id}");
    Ok(())
}

/// Lists the "Always allow" rules from the game (SPEC.md 6.6.5).
fn list_rules(dirs: &Dirs) {
    let rules = AlwaysRules::new(&dirs.data).list(now());
    if rules.is_empty() {
        println!("no Always allow rules");
    }
    for rule in rules {
        println!("{}", rule.line(now()));
    }
}

fn remove_rule(dirs: &Dirs, id: &str) -> Result<()> {
    if !AlwaysRules::new(&dirs.data).remove(id, now())? {
        bail!("no rule has the id {id}. Run: gnomish-relay rules");
    }
    println!("removed {id}");
    Ok(())
}

/// The forwarder inside a sandbox: it relays the ports, runs `child`, and exits with its
/// status (SPEC.md 6.6.4).
#[cfg(unix)]
fn forward_then(socket: &str, ports: &str, child: std::process::Command) -> Result<()> {
    let local_ports = forward::parse_ports(ports).map_err(anyhow::Error::msg)?;
    let forward = forward::Forward {
        socket: Path::new(socket),
        port: forward::INNER_PORT,
        local_ports: &local_ports,
    };
    std::process::exit(forward::run_forwarder(&forward, child))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["help" | "--help" | "-h"] => {
            println!("{USAGE}");
            Ok(())
        }
        ["setup", ref rest @ ..] => setup_command::setup(&Dirs::from_env()?, rest),
        ["install"] => install(&Dirs::from_env()?),
        ["run"] => start(&Dirs::from_env()?),
        ["run", "--background"] => {
            let log = service::start_background(&Dirs::from_env()?, &std::env::current_exe()?)?;
            println!("the bridge runs, and logs to {}", log.display());
            Ok(())
        }
        ["restart"] => service::restart(&Dirs::from_env()?, &std::env::current_exe()?),
        ["status"] => print_status(&Dirs::from_env()?),
        ["update"] => update::self_update(&Dirs::from_env()?),
        ["check-agent", name] => {
            check_agent::check_agent(&Dirs::from_env()?, name, &mut std::io::stdout())
        }
        ["approve"] => {
            list_approvals(&Dirs::from_env()?);
            Ok(())
        }
        ["approve", id] => answer_approval(&Dirs::from_env()?, id, desktop::Verdict::Approve),
        ["deny", id] => answer_approval(&Dirs::from_env()?, id, desktop::Verdict::Deny),
        ["rules"] => {
            list_rules(&Dirs::from_env()?);
            Ok(())
        }
        ["rules", "remove", id] => remove_rule(&Dirs::from_env()?, id),
        ["say", chat, id, text] => say(&Dirs::from_env()?, chat, id, text),
        ["selftest", "collect", ref rest @ ..] => selftest_collect(&Dirs::from_env()?, rest),
        [command_sandbox::RUN_FLAG, command] => {
            std::process::exit(command_sandbox::run_wrapped(command))
        }
        #[cfg(unix)]
        [bridge::holder::HOLD_FLAG, launch, proxy, ports] => {
            std::process::exit(bridge::holder::run_holder(Path::new(launch), proxy, ports))
        }
        #[cfg(unix)]
        [
            forward::FORWARD_FLAG,
            socket,
            ports,
            agent_wall::EXEC_FLAG,
            program,
            ref rest @ ..,
        ] => {
            let args: Vec<String> = rest.iter().map(|a| (*a).to_owned()).collect();
            let child = forward::exec_command(Path::new(program), &args);
            forward_then(socket, ports, child)
        }
        #[cfg(unix)]
        [forward::FORWARD_FLAG, socket, ports, shell, "-c", command] => forward_then(
            socket,
            ports,
            forward::shell_command(Path::new(shell), command),
        ),
        _ => bail!("{USAGE}"),
    }
}
