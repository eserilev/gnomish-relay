//! The Gnomish Relay bridge. See `SPEC.md` section 8.
//!
//! Step 5 of the build order: publish a fixed reply into the slots.

mod fs_safe;
mod slots;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use protocol::slot::{Reply, Status, prepare_replies, slot_body};

const USAGE: &str = "\
usage:
  gnomish-relay install          make the slot addons (game closed)
  gnomish-relay say <text>       publish one reply into every slot

The AddOns folder comes from GNOMISH_ADDONS.";

fn addons_dir() -> Result<PathBuf> {
    let dir = std::env::var_os("GNOMISH_ADDONS")
        .context("set GNOMISH_ADDONS to the Interface/AddOns folder of WoW")?;
    Ok(PathBuf::from(dir))
}

#[allow(clippy::cast_possible_truncation)] // u32 seconds last until 2106
fn now() -> Result<u32> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as u32)
}

fn body(replies: &[Reply]) -> Result<Vec<u8>> {
    Ok(slot_body(now()?, &prepare_replies(replies)))
}

fn say(text: &str) -> Result<()> {
    let reply = Reply {
        chat: b"relay".to_vec(),
        id: now()?,
        status: Status::Done,
        text: text.as_bytes().to_vec(),
    };
    slots::publish(&addons_dir()?, &body(&[reply])?)?;
    println!("published to {} slots", protocol::slot::SLOTS);
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["install"] => {
            let dir = addons_dir()?;
            slots::install(&dir, &body(&[])?)?;
            println!("made {} slots in {}", protocol::slot::SLOTS, dir.display());
            Ok(())
        }
        ["say", text] => say(text),
        _ => bail!("{USAGE}"),
    }
}
