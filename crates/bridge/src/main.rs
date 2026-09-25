//! The `gnomish-relay` command.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bridge::acp::AcpAgent;
use bridge::agent;
use bridge::config::{self, Config, Kind};
use bridge::fs_safe::write_atomic;
use bridge::install;
use bridge::receive::StripKey;
use bridge::run::{Paths, now, run};
use bridge::slots::{self, Files};
use protocol::slot::{Reply, Status, prepare_replies, slot_body};

const USAGE: &str = "\
usage:
  gnomish-relay setup [folder] [--roots a,b] [--new-key] [--autostart]
                                     install the addon, the key, the config, and the slots
  gnomish-relay install              make the slot addons (game closed)
  gnomish-relay run                  read strips, run the agents, publish the replies
  gnomish-relay check-agent <name>   start an agent of the config and show what it offers
  gnomish-relay say <chat> <id> <text>
                                     publish a reply to message <id> (from `/relay diag`)";

const APP: &str = "gnomish-relay";
const KEY_FILE: &str = "strip.key";

fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn home_dir() -> Result<PathBuf> {
    var("HOME")
        .or_else(|| var("USERPROFILE"))
        .context("HOME is not set")
}

/// The config folder of the OS. It holds `config.toml` and `strip.key`.
fn config_dir() -> Result<PathBuf> {
    let dir = if cfg!(windows) {
        var("APPDATA").context("APPDATA is not set")?
    } else if cfg!(target_os = "macos") {
        home_dir()?.join("Library").join("Application Support")
    } else {
        match var("XDG_CONFIG_HOME") {
            Some(dir) => dir,
            None => home_dir()?.join(".config"),
        }
    };
    Ok(dir.join(APP))
}

/// The data folder of the OS, for `state.json` (SPEC.md 8.3).
fn data_dir() -> Result<PathBuf> {
    let dir = if cfg!(windows) {
        var("LOCALAPPDATA").context("LOCALAPPDATA is not set")?
    } else if cfg!(target_os = "macos") {
        home_dir()?.join("Library").join("Application Support")
    } else {
        match var("XDG_DATA_HOME") {
            Some(dir) => dir,
            None => home_dir()?.join(".local").join("share"),
        }
    };
    Ok(dir.join(APP))
}

fn load_config() -> Result<Config> {
    config::load(&config_dir()?, &home_dir()?)
}

fn addons_dir(wow: &Path) -> PathBuf {
    wow.join("Interface").join("AddOns")
}

/// Mode 0600: the key signs strips, and the config sets the ceiling of every game message.
fn write_private(dir: &Path, name: &str, text: &str) -> Result<()> {
    write_atomic(dir, name, text.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.join(name), std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn pick_game(given: Option<&str>) -> Result<PathBuf> {
    if let Some(folder) = given {
        return Ok(PathBuf::from(folder));
    }
    let games = install::find_games(&home_dir()?);
    match games.as_slice() {
        [game] => Ok(game.clone()),
        [] => bail!(
            "found no WoW Forever folder. Start WoW once, or give the folder: gnomish-relay setup <_classic_beta_ folder>"
        ),
        more => bail!(
            "found more than one WoW Forever folder. Give one: gnomish-relay setup <folder>\n{}",
            more.iter()
                .map(|g| g.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        ),
    }
}

/// The key of this computer, made once. `--new-key` replaces it.
fn strip_key(dir: &Path, new: bool) -> Result<String> {
    let path = dir.join(KEY_FILE);
    if !new && let Ok(hex) = std::fs::read_to_string(&path) {
        return Ok(hex.trim().to_owned());
    }
    let hex = install::new_key()?;
    write_private(dir, KEY_FILE, &hex)?;
    println!("made a new strip key");
    Ok(hex)
}

fn command(program: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("cannot run {program}"))?;
    if !status.success() {
        bail!("{program} {} failed", args.join(" "));
    }
    Ok(())
}

/// Starts the bridge at each login, and now (SPEC.md 11.3).
fn autostart() -> Result<()> {
    let exe = std::env::current_exe()?;
    let path_var = std::env::var("PATH").unwrap_or_default();
    if cfg!(windows) {
        let run = format!("\"{}\" run", exe.display());
        command(
            "schtasks",
            &[
                "/Create",
                "/F",
                "/SC",
                "ONLOGON",
                "/TN",
                "Gnomish Relay",
                "/TR",
                &run,
            ],
        )?;
        command("schtasks", &["/Run", "/TN", "Gnomish Relay"])?;
    } else if cfg!(target_os = "macos") {
        let dir = home_dir()?.join("Library").join("LaunchAgents");
        std::fs::create_dir_all(&dir)?;
        let name = format!("{}.plist", install::LAUNCHD_LABEL);
        write_atomic(
            &dir,
            &name,
            install::launchd_plist(&exe, &path_var).as_bytes(),
        )?;
        let uid = String::from_utf8(std::process::Command::new("id").arg("-u").output()?.stdout)?;
        let plist = dir.join(&name).to_string_lossy().into_owned();
        let _ = command(
            "launchctl",
            &["bootout", &format!("gui/{}", uid.trim()), &plist],
        );
        command(
            "launchctl",
            &["bootstrap", &format!("gui/{}", uid.trim()), &plist],
        )?;
    } else {
        let dir = config_dir()?
            .parent()
            .context("no config folder")?
            .join("systemd")
            .join("user");
        std::fs::create_dir_all(&dir)?;
        write_atomic(
            &dir,
            "gnomish-relay.service",
            install::systemd_unit(&exe, &path_var).as_bytes(),
        )?;
        command("systemctl", &["--user", "daemon-reload"])?;
        command(
            "systemctl",
            &["--user", "enable", "--now", "gnomish-relay.service"],
        )?;
        println!("logs: journalctl --user -u gnomish-relay");
    }
    println!("the bridge starts at each login");
    Ok(())
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
    let default = if found.is_empty() {
        "~".to_owned()
    } else {
        found.join(", ")
    };
    let answer = match given {
        Some(list) => list.to_owned(),
        None => ask(
            "Folders the agents can work in, divided by commas",
            &default,
        )?,
    };
    let roots: Vec<String> = answer
        .split(',')
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_owned)
        .collect();
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
fn setup(args: &[&str]) -> Result<()> {
    let new_key = args.contains(&"--new-key");
    let roots_given = option(args, "--roots");
    let folder = args
        .iter()
        .copied()
        .find(|a| !a.starts_with("--") && Some(*a) != roots_given);
    let wow = pick_game(folder)?;
    let addons = addons_dir(&wow);
    if !addons.is_dir() {
        bail!(
            "{} has no Interface/AddOns folder. Start WoW once, then give the _classic_beta_ folder.",
            wow.display()
        );
    }
    println!("game: {}", wow.display());
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot make {}", dir.display()))?;
    let key = strip_key(&dir, new_key)?;
    let restart = match install::install_addon(&addons, &key)? {
        install::Installed::New => true,
        install::Installed::Updated => {
            println!("updated the addon: type /reload in the game");
            false
        }
        install::Installed::Unchanged => false,
    };
    if !dir.join(config::FILE).exists() {
        let agents = install::find_agents(&std::env::var_os("PATH").unwrap_or_default());
        let roots = choose_roots(&home_dir()?, roots_given)?;
        write_private(
            &dir,
            config::FILE,
            &config::default_text(&wow, &agents, &roots),
        )?;
        let names: Vec<&str> = agents.iter().map(|(name, _)| *name).collect();
        println!(
            "wrote {} with agents: {}",
            dir.join(config::FILE).display(),
            if names.is_empty() {
                "none, so echo".into()
            } else {
                names.join(", ")
            }
        );
    }
    if args.contains(&"--autostart") {
        autostart()?;
    }
    let slots_new = !addons.join(slots::slot_name(1)).is_dir();
    slots::install(&addons, &Files::empty(now()))?;
    if restart || slots_new {
        println!("restart WoW: it finds new addons only at launch");
    }
    println!("ok");
    Ok(())
}

fn body(replies: &[Reply]) -> Vec<u8> {
    slot_body(now(), &prepare_replies(replies))
}

fn say(chat: &str, id: &str, text: &str) -> Result<()> {
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
    let addons = addons_dir(&load_config()?.wow);
    let files = Files {
        body: body(&[reply]),
        ..Files::empty(now())
    };
    slots::publish(&addons, &files, next)?;
    println!(
        "published to {} slots from slot {next}",
        protocol::slot::SLOT_WINDOW
    );
    Ok(())
}

fn install() -> Result<()> {
    let dir = addons_dir(&load_config()?.wow);
    slots::install(&dir, &Files::empty(now()))?;
    println!("made {} slots in {}", protocol::slot::SLOTS, dir.display());
    Ok(())
}

fn start() -> Result<()> {
    let config = load_config()?;
    let state = data_dir()?;
    std::fs::create_dir_all(&state).with_context(|| format!("cannot make {}", state.display()))?;
    let paths = Paths {
        state,
        screenshots: config.wow.join("Screenshots"),
        accounts: config.wow.join("WTF").join("Account"),
        addons: addons_dir(&config.wow),
    };
    let key_path = config_dir()?.join(KEY_FILE);
    let key = StripKey::load(&key_path)?;
    // An addon app can replace the addon folder and drop the key (SPEC.md 11.3).
    let hex = std::fs::read_to_string(&key_path)?;
    if install::install_addon(&paths.addons, hex.trim())? != install::Installed::Unchanged {
        println!("wrote the addon files again: type /reload in the game");
    }
    let agents = agent::from_config(&config);
    run(paths, config.policy, key, agents)
}

/// Starts one agent of the config and opens a session in the default folder, with
/// no prompt. It shows that a new `[agents.<name>]` entry works.
fn check_agent(name: &str) -> Result<()> {
    let config = load_config()?;
    let spec = config
        .agents
        .get(name)
        .with_context(|| format!("the config has no [agents.{name}]"))?;
    if spec.kind != Kind::Acp {
        bail!("[agents.{name}] is not an ACP agent");
    }
    let agent = AcpAgent {
        command: spec.command.clone(),
        env: spec.env.clone(),
        modes: spec.modes.clone(),
        timeout: std::time::Duration::from_mins(1),
        permission_timeout: std::time::Duration::from_mins(1),
    };
    let cwd = String::from_utf8_lossy(&config.policy.folders.base).into_owned();
    let report = agent.check(&cwd).map_err(anyhow::Error::msg)?;
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
    for (level, mode) in &spec.modes {
        if !report.modes.contains(mode) {
            bail!("the agent has no mode {mode:?}, which the config names for {level:?}");
        }
    }
    println!("ok");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["setup", ref rest @ ..] => setup(rest),
        ["install"] => install(),
        ["run"] => start(),
        ["check-agent", name] => check_agent(name),
        ["say", chat, id, text] => say(chat, id, text),
        _ => bail!("{USAGE}"),
    }
}
