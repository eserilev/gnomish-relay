//! The `gnomish-relay` command.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bridge::acp::AcpAgent;
use bridge::agent;
use bridge::config::{self, Config, Kind};
use bridge::fs_safe::write_atomic;
use bridge::install;
use bridge::lock::{self, Bridge};
use bridge::receive::StripKey;
use bridge::run::{Paths, now, run};
use bridge::slots::{self, Files};
use bridge::update::{self, Replaced};
use protocol::slot::{Reply, Status, prepare_replies, slot_body};

const USAGE: &str = "\
usage:
  gnomish-relay setup [folder] [--roots a,b] [--new-key] [--autostart]
                                     install the addon, the key, the config, and the slots
  gnomish-relay install              make the slot addons (game closed)
  gnomish-relay run                  read strips, run the agents, publish the replies
  gnomish-relay restart              stop the bridge and start it again, for example after a config edit
  gnomish-relay update               install the latest release and restart the bridge
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
    install::addons_dir(wow)
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

/// The game folder: the one given, the one found, or the answer to a question.
fn pick_game(given: Option<&str>) -> Result<PathBuf> {
    if let Some(folder) = given {
        return Ok(install::game_folder(folder));
    }
    let games = install::find_games(&home_dir()?);
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

/// The key of this computer, made once. `--new-key` replaces it.
fn strip_key(dir: &Path, new: bool) -> Result<String> {
    let path = dir.join(KEY_FILE);
    if !new && let Ok(hex) = std::fs::read_to_string(&path) {
        return Ok(hex.trim().to_owned());
    }
    let hex = install::new_key()?;
    write_private(dir, KEY_FILE, &hex)?;
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
        // The Run key of the user needs no admin rights, unlike a scheduled task.
        let run = format!("\"{}\" run --background", exe.display());
        command(
            "reg",
            &[
                "add",
                RUN_KEY,
                "/v",
                "Gnomish Relay",
                "/t",
                "REG_SZ",
                "/d",
                &run,
                "/f",
            ],
        )?;
        restart_process(&exe)?;
    } else if cfg!(target_os = "macos") {
        let dir = launch_agents_dir()?;
        std::fs::create_dir_all(&dir)?;
        let name = format!("{}.plist", install::LAUNCHD_LABEL);
        let log = home_dir()?
            .join("Library")
            .join("Logs")
            .join("gnomish-relay.log");
        write_atomic(
            &dir,
            &name,
            install::launchd_plist(&exe, &path_var, &log).as_bytes(),
        )?;
        let domain = launchd_domain()?;
        let plist = dir.join(&name).to_string_lossy().into_owned();
        let _ = command("launchctl", &["bootout", &domain, &plist]);
        command("launchctl", &["bootstrap", &domain, &plist])?;
        println!("logs: {}", log.display());
    } else {
        let dir = systemd_dir()?;
        std::fs::create_dir_all(&dir)?;
        write_atomic(
            &dir,
            SYSTEMD_UNIT,
            install::systemd_unit(&exe, &path_var).as_bytes(),
        )?;
        command("systemctl", &["--user", "daemon-reload"])?;
        command("systemctl", &["--user", "enable", SYSTEMD_UNIT])?;
        command("systemctl", &["--user", "restart", SYSTEMD_UNIT])?;
        println!("logs: journalctl --user -u gnomish-relay");
    }
    Ok(())
}

const SYSTEMD_UNIT: &str = "gnomish-relay.service";

fn systemd_dir() -> Result<PathBuf> {
    Ok(config_dir()?
        .parent()
        .context("no config folder")?
        .join("systemd")
        .join("user"))
}

fn launch_agents_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join("Library").join("LaunchAgents"))
}

fn launchd_domain() -> Result<String> {
    let uid = std::process::Command::new("id").arg("-u").output()?.stdout;
    Ok(format!("gui/{}", String::from_utf8(uid)?.trim()))
}

/// Restarts the bridge through the login service of setup, or as a process with no
/// service. `exe` is the program to start: after an update, `current_exe` names the
/// old file.
fn restart(exe: &Path) -> Result<()> {
    if cfg!(target_os = "linux") && systemd_dir()?.join(SYSTEMD_UNIT).is_file() {
        return command("systemctl", &["--user", "restart", SYSTEMD_UNIT]);
    }
    let plist = launch_agents_dir()?.join(format!("{}.plist", install::LAUNCHD_LABEL));
    if cfg!(target_os = "macos") && plist.is_file() {
        let service = format!("{}/{}", launchd_domain()?, install::LAUNCHD_LABEL);
        return command("launchctl", &["kickstart", "-k", &service]);
    }
    restart_process(exe)
}

/// A bridge that runs with no service gets stopped, and then `exe` starts in the background.
fn restart_process(exe: &Path) -> Result<()> {
    let data = data_dir()?;
    std::fs::create_dir_all(&data)?;
    match lock::status(&data)? {
        Bridge::Stopped => {}
        Bridge::Runs(None) => {
            bail!("a bridge runs, but its process id is unknown. Stop it by hand")
        }
        Bridge::Runs(Some(pid)) => stop_process(pid)?,
    }
    if !lock::wait_until_stopped(&data, std::time::Duration::from_secs(10))? {
        bail!("the bridge does not stop");
    }
    let log = start_background(exe)?;
    println!("the bridge runs, and logs to {}", log.display());
    Ok(())
}

fn stop_process(pid: u32) -> Result<()> {
    let pid = pid.to_string();
    if cfg!(windows) {
        // `/T` also stops the agents that the bridge started.
        command("taskkill", &["/PID", &pid, "/T", "/F"])
    } else {
        command("kill", &[&pid])
    }
}

fn self_update() -> Result<()> {
    let name = update::archive_name().context("there is no release build for this OS and CPU")?;
    let base = std::env::var("GNOMISH_URL").unwrap_or_else(|_| update::RELEASES.to_owned());
    let exe = std::env::current_exe()?;
    let work = data_dir()?.join("update");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let replaced = update::fetch(&base, name, &work).and_then(|new| update::replace(&exe, &new));
    let _ = std::fs::remove_dir_all(&work);
    if replaced? == Replaced::Same {
        println!("gnomish-relay is the latest release");
        return Ok(());
    }
    println!("updated {}", exe.display());
    restart(&exe)?;
    println!("type /reload in the game");
    Ok(())
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
/// Big enough for weeks of normal logs. A bigger log starts again.
const MAX_LOG: u64 = 4 * 1024 * 1024;

/// Starts `run` as a new process with no console window, and its log in a file.
fn start_background(exe: &Path) -> Result<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    let log_path = dir.join("bridge.log");
    let too_big = std::fs::metadata(&log_path).is_ok_and(|m| m.len() > MAX_LOG);
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(&log_path)?;
    let mut child = std::process::Command::new(exe);
    child
        .arg("run")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        child.creation_flags(CREATE_NO_WINDOW);
    }
    child.spawn().context("cannot start the bridge")?;
    Ok(log_path)
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
fn setup(args: &[&str]) -> Result<()> {
    let new_key = args.contains(&"--new-key");
    let roots_given = option(args, "--roots");
    let folder = args
        .iter()
        .copied()
        .find(|a| !a.starts_with("--") && Some(*a) != roots_given);
    let wow = pick_game(folder)?;
    if !wow.is_dir() {
        bail!("{} is not a folder", wow.display());
    }
    // WoW makes Interface/AddOns at its first start. Setup makes it earlier.
    let addons = addons_dir(&wow);
    std::fs::create_dir_all(&addons)
        .with_context(|| format!("cannot make {}", addons.display()))?;
    println!("WoW: {}", wow.display());
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot make {}", dir.display()))?;
    let key = strip_key(&dir, new_key)?;
    // The addon and the slots first: they need nothing else, and a later step can fail.
    let addon = install::install_addon(&addons, &key)?;
    let slots_new = !addons.join(slots::slot_name(1)).is_dir();
    slots::install(&addons, &Files::empty(now()))?;
    // A first setup can stop at the folder question after the addon and the slots, so
    // the first config also means that WoW has not seen them yet.
    let first = !dir.join(config::FILE).exists();
    if first {
        let agents = install::find_agents(&std::env::var_os("PATH").unwrap_or_default());
        let roots = choose_roots(&home_dir()?, roots_given)?;
        write_private(
            &dir,
            config::FILE,
            &config::default_text(&wow, &agents, &roots),
        )?;
    }
    let config = load_config()?;
    println!("{}", agent_line(&config));
    if args.contains(&"--autostart") {
        match autostart() {
            Ok(()) => println!("Bridge: on, starts at login"),
            Err(e) => println!("Bridge: not started at login ({e:#}). Run: gnomish-relay run"),
        }
    }
    match (addon, slots_new || first) {
        (install::Installed::New, _) | (_, true) => println!("Restart WoW, then type /relay"),
        (install::Installed::Updated, false) => println!("Type /reload in WoW"),
        (install::Installed::Unchanged, false) => println!("Ready"),
    }
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
    let _lock = lock::take(&state)?;
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

/// The default agent, started once with no prompt, so a missing login shows here and
/// not as the first reply in the game.
fn agent_line(config: &Config) -> String {
    let name = &config.policy.default_agent;
    let Some(spec) = config.agents.get(name).filter(|s| s.kind == Kind::Acp) else {
        return "Agent: none. Replies repeat your message.".into();
    };
    let agent = AcpAgent {
        command: spec.command.clone(),
        env: spec.env.clone(),
        modes: spec.modes.clone(),
        timeout: std::time::Duration::from_mins(1),
        permission_timeout: std::time::Duration::from_mins(1),
    };
    let cwd = String::from_utf8_lossy(&config.policy.folders.base).into_owned();
    match agent.check(&cwd) {
        Ok(_) => format!("Agent: {name}"),
        Err(e) if install::needs_login(&e) => match install::login_command(name) {
            Some(login) => format!("Agent: {name} needs a login. Run: {login}"),
            None => format!("Agent: {name} needs a login."),
        },
        Err(e) => format!("Agent: {name} does not start: {e}"),
    }
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
        ["run", "--background"] => {
            let log = start_background(&std::env::current_exe()?)?;
            println!("the bridge runs, and logs to {}", log.display());
            Ok(())
        }
        ["restart"] => restart(&std::env::current_exe()?),
        ["update"] => self_update(),
        ["check-agent", name] => check_agent(name),
        ["say", chat, id, text] => say(chat, id, text),
        _ => bail!("{USAGE}"),
    }
}
