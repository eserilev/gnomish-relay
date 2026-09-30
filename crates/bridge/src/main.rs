//! The `gnomish-relay` command.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bridge::agent;
use bridge::agent::Agents;
#[cfg(unix)]
use bridge::agent_wall;
use bridge::always_rules::{self, AlwaysRules};
use bridge::app_files::private_game_paths;
use bridge::command_sandbox;
use bridge::config::{self, Config, Policy, RelayConfig, StoryConfig};
use bridge::config_text::RelayPart;
use bridge::desktop::{self, Approvals, Prompt};
use bridge::dirs::Dirs;
#[cfg(unix)]
use bridge::forward;
use bridge::gate::{Gate, Places};
use bridge::install;
use bridge::lock;
use bridge::model::ModelChoice;
use bridge::model_setup;
use bridge::raise::Raiser;
use bridge::receive::{KeySet, RELAY_KEY_FILE};
use bridge::run::{Paths, now, run};
use bridge::selftest;
use bridge::service;
use bridge::settings_list::BridgeSettings;
use bridge::setup::{self, KeyChoice};
use bridge::slots::{self, Files};
use bridge::status::{self, SandboxFound};
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

/// The game folder: the one given, the one found, or the answer to a question.
fn pick_game(dirs: &Dirs, given: Option<&str>) -> Result<PathBuf> {
    if let Some(folder) = given {
        return Ok(install::game_folder(folder));
    }
    let games = install::find_games(&dirs.home);
    if let [game] = games.as_slice() {
        return Ok(game.clone());
    }
    for (n, game) in games.iter().enumerate() {
        println!("{}. {}", n + 1, game.display());
    }
    let answer = ask("WoW folder", if games.is_empty() { "" } else { "1" })?;
    if answer.is_empty() {
        bail!("give the WoW folder: gnomish-relay setup <folder>");
    }
    let chosen = answer
        .parse::<usize>()
        .ok()
        .and_then(|n| games.get(n.checked_sub(1)?));
    Ok(chosen
        .cloned()
        .unwrap_or_else(|| install::game_folder(&answer)))
}

/// Reads one answer in a terminal. With no terminal, or an empty answer, the default.
fn ask(question: &str, default: &str) -> Result<String> {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return Ok(default.to_owned());
    }
    print!("{question} [{default}]: ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    let answer = answer.trim();
    Ok(if answer.is_empty() { default } else { answer }.to_owned())
}

/// `~/code` reads better in the config than the full path.
fn with_tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// The folders of code projects that setup finds, or the home folder.
fn choose_roots(home: &Path, given: Option<&str>) -> Result<Vec<String>> {
    let found: Vec<String> = install::suggest_roots(home)
        .iter()
        .map(|p| with_tilde(p, home))
        .collect();
    let answer = match given {
        Some(list) => list.to_owned(),
        // No default of the home folder: it holds ~/.ssh and the browser profiles.
        None => ask(
            "Folders the agents can work in, divided by commas",
            &found.join(", "),
        )?,
    };
    let roots: Vec<String> = answer
        .split(',')
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_owned)
        .collect();
    if roots.is_empty() {
        bail!("give the folders that the agents can work in: gnomish-relay setup --roots ~/code");
    }
    for root in &roots {
        if !config::expand(root, home)?.is_dir() {
            bail!("{root} is not a folder");
        }
    }
    Ok(roots)
}

fn option<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| *a == name)?;
    args.get(at + 1).copied()
}

/// Every step leaves alone what works, so a second run is safe (SPEC.md 11.3).
fn setup(dirs: &Dirs, args: &[&str]) -> Result<()> {
    let keys = if args.contains(&"--new-key") {
        KeyChoice::New
    } else {
        KeyChoice::Keep
    };
    let roots_given = option(args, "--roots");
    let folder = args
        .iter()
        .copied()
        .find(|a| !a.starts_with("--") && Some(*a) != roots_given);
    let wow = pick_game(dirs, folder)?;
    if !wow.is_dir() {
        bail!("{} is not a folder", wow.display());
    }
    // WoW makes Interface/AddOns at its first start. Setup makes it earlier.
    let addons = addons_dir(&wow);
    std::fs::create_dir_all(&addons)
        .with_context(|| format!("cannot make {}", addons.display()))?;
    println!("WoW: {}", wow.display());
    let existing = match std::fs::read_to_string(dirs.config.join(config::FILE)) {
        Ok(text) => Some((text, load_config(dirs)?)),
        Err(_) => None,
    };
    let timeways = install::timeways_dir(&addons).is_some();
    let found = setup::Found {
        relay_asked: args.contains(&"--relay") || roots_given.is_some(),
        config_has_relay: existing.as_ref().map(|(_, c)| c.relay.is_some()),
        relay_folder: addons.join(install::ADDON).exists(),
        timeways_folder: timeways,
    };
    let relay = match setup::relay_choice(&found) {
        setup::RelayChoice::Decided(relay) => relay,
        setup::RelayChoice::Ask => ask_relay()?,
    };
    let folders = setup::Folders {
        config: dirs.config.clone(),
        addons,
    };
    // The addon and the slots first: they need nothing else, and a later step can fail.
    let changed = setup::install_files(&folders, relay, keys)?;
    let config = setup_config(dirs, &wow, existing.as_ref(), relay, timeways, roots_given)?;
    print_setup(dirs, &config, relay, timeways);
    if args.contains(&"--autostart") {
        match service::autostart(dirs) {
            Ok(()) => println!("Bridge: on, starts at login"),
            Err(e) => println!("Bridge: not started at login ({e:#}). Run: gnomish-relay run"),
        }
    }
    println!("{}", last_line(&changed, relay, keys));
    Ok(())
}

/// A player who came for Timeways says no, so no is the answer with no terminal.
fn ask_relay() -> Result<setup::Relay> {
    let answer = ask(
        "Also set up Gnomish Relay, coding agents in the game? (y/N)",
        "n",
    )?;
    Ok(if answer.eq_ignore_ascii_case("y") {
        setup::Relay::On
    } else {
        setup::Relay::Off
    })
}

/// Writes the first config, or adds the part that it lacks: the relay with `--relay`,
/// and `[story]` when the Timeways addon is there.
fn setup_config(
    dirs: &Dirs,
    wow: &Path,
    existing: Option<&(String, Config)>,
    relay: setup::Relay,
    timeways: bool,
    roots_given: Option<&str>,
) -> Result<Config> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let lacks_relay = existing.is_none_or(|(_, c)| c.relay.is_none());
    let lacks_story = existing.is_none_or(|(_, c)| c.story.is_none());
    let agents = install::find_agents(&path_var);
    let roots = if relay == setup::Relay::On && lacks_relay {
        choose_roots(&dirs.home, roots_given)?
    } else {
        Vec::new()
    };
    let harnesses = if roots.is_empty() {
        Vec::new()
    } else {
        choose_harnesses(&path_var)?
    };
    let wants_story = timeways && lacks_story;
    // A local model is also for the agents: the relay part opens its port.
    let models = if wants_story || !roots.is_empty() {
        model_setup::find_models(&path_var)
    } else {
        Vec::new()
    };
    let local_ports = model_setup::local_ports(&models);
    let new_agents = setup::new_agents(&agents, existing.map(|(_, config)| config));
    let parts = setup::ConfigParts {
        wow,
        relay: (!roots.is_empty()).then_some(RelayPart {
            agents: &agents,
            harnesses: &harnesses,
            roots: &roots,
            local_ports: &local_ports,
        }),
        new_agents: &new_agents,
        story: wants_story.then_some(models.as_slice()),
    };
    let text = existing.map(|(text, _)| text.as_str());
    let config = match setup::config_text(text, &parts) {
        Some(new) => setup::write_config(&dirs.config, &new, &dirs.home)?,
        None => load_config(dirs)?,
    };
    let added = config.relay.as_ref().map(|relay| &relay.agents);
    for (name, _, _) in &new_agents {
        if added.is_some_and(|agents| agents.contains_key(*name)) {
            println!("Added agent: {name}. Pick it for a new chat in the game, in Settings");
        }
    }
    Ok(config)
}

/// A harness with no ACP mode runs its own commands, so setup adds it only on a yes.
fn choose_harnesses(path_var: &std::ffi::OsStr) -> Result<Vec<&'static str>> {
    let mut chosen = Vec::new();
    for name in install::find_harnesses(path_var) {
        let answer = ask(
            &format!(
                "Found {name}. Add it as an agent? It runs its own commands with no question, inside the sandbox. (y/N)"
            ),
            "n",
        )?;
        if answer.eq_ignore_ascii_case("y") {
            chosen.push(name);
        }
    }
    Ok(chosen)
}

fn print_setup(dirs: &Dirs, config: &Config, relay: setup::Relay, timeways: bool) {
    match &config.relay {
        Some(relay_config) => {
            for line in relay_lines(dirs, relay_config) {
                println!("{line}");
            }
            let config_file = dirs.config.join(config::FILE);
            println!("{}", setup::level_line(relay_config, &config_file));
        }
        None if relay == setup::Relay::Off => {
            println!("Gnomish Relay: off. To add coding agents: gnomish-relay setup --relay");
        }
        None => {}
    }
    if timeways {
        println!("{}", story_line(config));
    }
}

/// The agent and the sandbox, which setup checks by starting them.
fn relay_lines(dirs: &Dirs, config: &RelayConfig) -> Vec<String> {
    let gate = check_gate(dirs, config);
    let path = std::env::var_os("PATH").unwrap_or_default();
    let sandbox = SandboxFound::of(&gate.sandbox.tool, &path);
    vec![
        status::agent_line(config, &gate),
        status::sandbox_line(&sandbox),
    ]
}

fn story_line(config: &Config) -> String {
    let model = config.story.as_ref().map(|story| &story.model.choice);
    match model {
        Some(ModelChoice::Claude { model, .. }) => format!(
            "Story model: claude ({})",
            model.as_deref().unwrap_or("default")
        ),
        Some(ModelChoice::Local(local)) => format!("Story model: local {}", local.model),
        _ => {
            "Story model: none. Set model in [story] of the config, then run: gnomish-relay restart"
                .into()
        }
    }
}

/// WoW finds a new addon folder only at launch, and a new key only after a `/reload`.
fn last_line(changed: &setup::Changed, relay: setup::Relay, keys: KeyChoice) -> &'static str {
    let new_relay = changed.relay_addon == Some(install::Installed::New);
    if changed.new_slots || new_relay {
        return match relay {
            setup::Relay::On => "Restart WoW, then type /relay",
            setup::Relay::Off => "Restart WoW, then log in",
        };
    }
    let updated = [changed.relay_addon.as_ref(), changed.timeways_key.as_ref()]
        .contains(&Some(&install::Installed::Updated));
    if updated || keys == KeyChoice::New {
        return "Type /reload in WoW";
    }
    "Ready"
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
    let collected = selftest::collect(&pick_game(dirs, game)?, &repo)?;
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

/// A check sends no prompt, so no tool call reaches this gate.
fn check_gate(dirs: &Dirs, config: &RelayConfig) -> Gate {
    let places = Places {
        config_dir: &dirs.config,
        data_dir: &dirs.data,
        home: &dirs.home,
    };
    Gate::new(config, &places, Prompt::Off)
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
    for p in pending {
        let age = now().saturating_sub(p.created);
        let left = p
            .minutes_left(now())
            .map_or(String::new(), |minutes| format!("  {minutes} min left"));
        println!("{}  {age}s ago{left}  {} in {}", p.id, p.agent, p.folder);
        for line in p.text.lines() {
            println!("    {line}");
        }
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
    for r in rules {
        let scope = match r.scope {
            always_rules::Scope::Tree => "and the folders inside",
            always_rules::Scope::Exact => "only",
        };
        let days = r.days_unused(now());
        println!(
            "{}  {}  in {} ({scope}), last used {days} days ago",
            r.id,
            r.pattern(),
            r.folder.display()
        );
    }
}

fn remove_rule(dirs: &Dirs, id: &str) -> Result<()> {
    if !AlwaysRules::new(&dirs.data).remove(id, now())? {
        bail!("no rule has the id {id}. Run: gnomish-relay rules");
    }
    println!("removed {id}");
    Ok(())
}

/// Starts one agent of the config and opens a session in the default folder, with
/// no prompt. It shows that a new `[agents.<name>]` entry works.
fn check_agent(dirs: &Dirs, name: &str) -> Result<()> {
    let config = load_config(dirs)?;
    let config = config.require_relay()?;
    let spec = config
        .agents
        .get(name)
        .with_context(|| format!("the config has no [agents.{name}]"))?;
    let cwd = String::from_utf8_lossy(&config.policy.folders.base).into_owned();
    let report = agent::check(name, spec, &cwd, &check_gate(dirs, config))
        .with_context(|| format!("[agents.{name}] is the echo agent: it starts nothing"))?
        .map_err(anyhow::Error::msg)?;
    println!("{name}: {} {}", report.name, report.version);
    println!(
        "resumes sessions: {}",
        if report.load_session { "yes" } else { "no" }
    );
    println!(
        "modes: {}",
        if report.modes.is_empty() {
            "none".into()
        } else {
            report.modes.join(", ")
        }
    );
    for line in &report.details {
        println!("{line}");
    }
    let service = install::service_file(&dirs.config, &dirs.home)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| install::service_path_var(&text));
    let missing = spec
        .command
        .first()
        .and_then(|program| status::service_path_line(program, service.as_deref()));
    if let Some(line) = missing {
        println!("{line}");
    }
    for (level, mode) in &spec.modes {
        if !report.modes.contains(mode) {
            bail!("the agent has no mode {mode:?}, which the config names for {level:?}");
        }
    }
    println!("ok");
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
        ["setup", ref rest @ ..] => setup(&Dirs::from_env()?, rest),
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
        ["check-agent", name] => check_agent(&Dirs::from_env()?, name),
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
