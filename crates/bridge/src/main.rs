//! The `gnomish-relay` command.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use bridge::agent::Echo;
use bridge::receive::StripKey;
use bridge::relay::Folders;
use bridge::run::{Paths, now, run};
use bridge::slots;
use protocol::slot::{Reply, Status, prepare_replies, slot_body};

const USAGE: &str = "\
usage:
  gnomish-relay install          make the slot addons (game closed)
  gnomish-relay run              read strips, answer with the echo agent, publish
  gnomish-relay say <chat> <id> <text>
                                 publish a reply to message <id> (from `/relay diag`)

The AddOns folder comes from GNOMISH_ADDONS. Agents work inside the folders of
GNOMISH_ROOTS (a path list, like PATH), or inside $HOME. The first one is the default.";

fn addons_dir() -> Result<PathBuf> {
    let dir = std::env::var_os("GNOMISH_ADDONS")
        .context("set GNOMISH_ADDONS to the Interface/AddOns folder of WoW")?;
    Ok(PathBuf::from(dir))
}

/// `Interface/AddOns` is two levels below the folder that holds `Screenshots`.
fn screenshots_dir(addons: &std::path::Path) -> Result<PathBuf> {
    let game = addons
        .parent()
        .and_then(std::path::Path::parent)
        .context("GNOMISH_ADDONS has no game folder")?;
    Ok(game.join("Screenshots"))
}

fn folders() -> Result<Folders> {
    let roots: Vec<PathBuf> = match std::env::var_os("GNOMISH_ROOTS") {
        Some(list) => std::env::split_paths(&list).collect(),
        None => vec![PathBuf::from(
            std::env::var_os("HOME").context("HOME is not set")?,
        )],
    };
    let roots: Vec<Vec<u8>> = roots
        .iter()
        .map(|r| r.to_string_lossy().as_bytes().to_vec())
        .collect();
    let base = roots.first().context("GNOMISH_ROOTS is empty")?.clone();
    Ok(Folders { roots, base })
}

fn key_path() -> Result<PathBuf> {
    let config = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".config"),
    };
    Ok(config.join("gnomish-relay").join("strip.key"))
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
    slots::publish(&addons_dir()?, &body(&[reply]), next)?;
    println!(
        "published to {} slots from slot {next}",
        protocol::slot::SLOT_WINDOW
    );
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["install"] => {
            let dir = addons_dir()?;
            slots::install(&dir, &body(&[]))?;
            println!("made {} slots in {}", protocol::slot::SLOTS, dir.display());
            Ok(())
        }
        ["run"] => {
            let addons = addons_dir()?;
            let paths = Paths {
                screenshots: screenshots_dir(&addons)?,
                addons,
            };
            run(
                paths,
                folders()?,
                StripKey::load(&key_path()?)?,
                Arc::new(Echo),
            )
        }
        ["say", chat, id, text] => say(chat, id, text),
        _ => bail!("{USAGE}"),
    }
}
