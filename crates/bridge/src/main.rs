//! The `gnomish-relay` command.

#[cfg(unix)]
use std::path::Path;

use anyhow::{Context, Result, bail};
#[cfg(unix)]
use bridge::agent_wall;
use bridge::always_rules::AlwaysRules;
use bridge::check_agent;
use bridge::command_sandbox;
use bridge::config;
use bridge::desktop::{self, Approvals, Prompt};
use bridge::dirs::Dirs;
#[cfg(unix)]
use bridge::forward;
use bridge::gate::Places;
use bridge::hooks_install;
use bridge::install;
use bridge::run::now;
use bridge::selftest;
use bridge::service;
use bridge::setup_command;
use bridge::slots;
use bridge::start;
use bridge::status;
use bridge::update;
use protocol::slot::{Reply, Status};

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
  gnomish-relay hooks install [--claude] [--codex]
                                     show notifications from Claude Code and Codex in a terminal
  gnomish-relay hooks remove [--claude] [--codex]
                                     take the hooks of the notifications out again
  gnomish-relay hooks status         show the hooks of each agent
  gnomish-relay say <chat> <id> <text>
                                     publish a reply to message <id> (from `/relay diag`)
  gnomish-relay selftest collect [folder] [--out <repo>]
                                     copy the results of the self-test addon into the repo (developers)";

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
    let config = config::load(&dirs.config, &dirs.home)?;
    config.require_relay()?;
    slots::publish_reply(&install::addons_dir(&config.wow), reply, next, now())?;
    println!(
        "published to {} slots from slot {next}",
        protocol::slot::SLOT_WINDOW
    );
    Ok(())
}

fn print_status(dirs: &Dirs) -> Result<()> {
    let places = Places::of(dirs);
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
        ["install"] => setup_command::install_slots(&Dirs::from_env()?),
        ["run"] => start::start(&Dirs::from_env()?),
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
        ["selftest", "collect", ref rest @ ..] => {
            selftest::collect_command(&Dirs::from_env()?, rest)
        }
        ["hook", agent] => bridge::hook::main(agent),
        ["hooks", action, ref flags @ ..] => {
            hooks_install::command(&Dirs::from_env()?, action, flags, &mut std::io::stdout())
        }
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
