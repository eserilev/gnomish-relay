//! `gnomish-relay selftest collect`: copies the results of the self-test addon into the
//! repo (SPEC.md 14.3). It reads only the saved variables of the self-test and its own
//! screenshots. It never reads the key or the config of the relay.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::dirs::Dirs;
use crate::fixture::{self, PLACEHOLDER};
use crate::ids::hex;
use crate::saved;
use crate::setup_command;
use crate::vectors::{self, MANIFEST, Manifest, Shot, TEST_KEY, Vector};

pub const SAVED_FILE: &str = "GnomishRelaySelfTest.lua";
/// A screenshot file gets its time a moment after the `Screenshot()` call.
const BEFORE_FIRST_SHOT: Duration = Duration::from_secs(10);
const AFTER_LAST_SHOT: Duration = Duration::from_mins(1);

/// The hex fields that the self-test addon writes into its saved variables.
pub struct Parts {
    pub results: Value,
    pub load: Value,
    pub combat: Option<Value>,
}

fn json_field(text: &str, name: &str) -> Result<Option<Value>> {
    let Some(bytes) = saved::hex_fields(text, name).into_iter().next() else {
        return Ok(None);
    };
    let value =
        serde_json::from_slice(&bytes).with_context(|| format!("the {name} field is not JSON"))?;
    Ok(Some(value))
}

impl Parts {
    pub fn read(text: &str) -> Result<Parts> {
        let results = json_field(text, "results")?.context(
            "the self-test has no results yet. Wait for \"done\" in the game, then type /reload",
        )?;
        let load = json_field(text, "load")?.context("the self-test has no load order")?;
        if results.get("key").and_then(Value::as_str) != Some(hex(TEST_KEY).as_str()) {
            bail!("the results name another key than the public test key");
        }
        Ok(Parts {
            results,
            load,
            combat: json_field(text, "combat")?,
        })
    }

    /// The shots of the normal run, then the shot in combat.
    pub fn shots(&self) -> Result<Vec<Shot>> {
        let mut shots: Vec<Shot> = fixture::shots(&self.results)?
            .into_iter()
            .map(|s| s.shot)
            .collect();
        if let Some(combat) = &self.combat {
            shots.extend(fixture::shots(combat)?.into_iter().map(|s| s.shot));
        }
        Ok(shots)
    }
}

/// A screenshot that holds the frame of one shot.
pub struct Found {
    pub shot: Shot,
    pub path: PathBuf,
    pub size: (usize, usize),
}

fn window(shots: &[Shot]) -> Result<(SystemTime, SystemTime)> {
    let times: Vec<u32> = shots.iter().filter_map(|s| s.unix).collect();
    let (Some(first), Some(last)) = (times.iter().min(), times.iter().max()) else {
        bail!("the self-test took no screenshot");
    };
    let at = |unix: u32| UNIX_EPOCH + Duration::from_secs(u64::from(unix));
    Ok((at(*first) - BEFORE_FIRST_SHOT, at(*last) + AFTER_LAST_SHOT))
}

fn is_png(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
}

fn in_window(path: &Path, (from, to): (SystemTime, SystemTime)) -> bool {
    fs::symlink_metadata(path)
        .and_then(|meta| meta.modified())
        .is_ok_and(|modified| from <= modified && modified <= to)
}

/// The shot whose frame the screenshot holds, if any. A screenshot of the player, or a
/// strip of the relay (another key), holds none.
fn match_file(path: &Path, shots: &[Shot]) -> Result<Option<Found>> {
    let Ok(image) = vectors::read_png(path) else {
        return Ok(None);
    };
    let Some(frame) = vectors::test_frame(&image)? else {
        return Ok(None);
    };
    Ok(vectors::shot_of(&frame, shots).map(|shot| Found {
        shot: shot.clone(),
        path: path.to_owned(),
        size: image.size(),
    }))
}

/// The screenshots of the self-test, one per shot, by the frame that each one holds. The
/// file names in the saved variables are never used: that file is untrusted text.
pub fn find(dir: &Path, shots: &[Shot]) -> Result<Vec<Found>> {
    let window = window(shots)?;
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_png(path) && in_window(path, window))
        .collect();
    paths.sort();
    let mut found: Vec<Found> = Vec::new();
    for path in paths {
        let Some(one) = match_file(&path, shots)? else {
            continue;
        };
        if !found.iter().any(|f| f.shot.frame_id == one.shot.frame_id) {
            found.push(one);
        }
    }
    Ok(found)
}

/// The name of the file in the repo. The shot name comes from the game, so only safe bytes stay.
fn file_name(shot: &Shot) -> String {
    let safe = !shot.name.is_empty()
        && shot
            .name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    let name = if safe { shot.name.as_str() } else { "shot" };
    format!("{:03}-{name}.png", shot.frame_id)
}

fn write_vectors(dir: &Path, build: &str, found: &[Found], saved_file: &Path) -> Result<usize> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir)?;
    let mut vectors = Vec::new();
    for one in found {
        let file = file_name(&one.shot);
        fs::copy(&one.path, dir.join(&file))?;
        vectors.push(Vector {
            file,
            shot: one.shot.clone(),
            width: one.size.0,
            height: one.size.1,
        });
    }
    let manifest = Manifest {
        build: build.to_owned(),
        key: hex(TEST_KEY),
        vectors,
    };
    fs::write(
        dir.join(MANIFEST),
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    // The raw bytes show how WoW writes saved variables, for the tests of saved.rs.
    fs::copy(saved_file, dir.join(SAVED_FILE))?;
    Ok(manifest.vectors.len())
}

pub struct Collected {
    pub build: String,
    pub fixture: PathBuf,
    pub vectors: usize,
    /// Shots with no screenshot in the Screenshots folder.
    pub missing: Vec<String>,
    pub capture: fixture::Capture,
}

/// Reads the results in `game` and writes `tests/fixtures` and `tests/vectors` in `repo`.
pub fn collect(game: &Path, repo: &Path) -> Result<Collected> {
    let accounts = game.join("WTF").join("Account");
    let (saved_file, text) = saved::newest(&accounts, SAVED_FILE).context(
        "the self-test has no saved variables. Link it with scripts/selftest-link.sh, log in, wait for \"done\", and type /reload",
    )?;
    let parts = Parts::read(&text)?;
    let shots = parts.shots()?;
    let found = find(&game.join("Screenshots"), &shots)?;
    let probe_in_picture = found.iter().any(|f| f.shot.kind == "hide_after_call");
    let fixture = fixture::build(
        &parts.results,
        &parts.load,
        parts.combat.as_ref(),
        probe_in_picture,
    )?;
    let fixtures = repo.join("tests").join("fixtures");
    fs::create_dir_all(&fixtures)?;
    let path = fixtures.join(format!("forever-{}.json", fixture.build));
    fs::write(&path, serde_json::to_string_pretty(&fixture)? + "\n")?;
    let _ = fs::remove_file(fixtures.join(PLACEHOLDER));
    let dir = repo.join("tests").join("vectors").join(&fixture.build);
    let vectors = write_vectors(&dir, &fixture.build, &found, &saved_file)?;
    let missing = shots
        .iter()
        .filter(|s| s.kind != "hide_after_call")
        .filter(|s| !found.iter().any(|f| f.shot.frame_id == s.frame_id))
        .map(|s| s.name.clone())
        .collect();
    Ok(Collected {
        build: fixture.build,
        fixture: path,
        vectors,
        missing,
        capture: fixture.fake.capture,
    })
}

/// `selftest collect [folder] [--out <repo>]`. It needs no config and no key, so it
/// finds the game as setup does.
pub fn collect_command(dirs: &Dirs, args: &[&str]) -> Result<()> {
    let Some((game, out)) = collect_args(args) else {
        bail!("usage: gnomish-relay selftest collect [folder] [--out <repo>]");
    };
    let repo = match out {
        Some(out) => PathBuf::from(out),
        None => std::env::current_dir()?,
    };
    if !repo.join("addon").join("GnomishRelaySelfTest").is_dir() {
        bail!("run this in the gnomish-relay repo, or give --out <repo>");
    }
    let collected = collect(&setup_command::pick_game(dirs, game)?, &repo)?;
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

/// The game folder and the repo, each one optional.
fn collect_args<'a>(args: &[&'a str]) -> Option<(Option<&'a str>, Option<&'a str>)> {
    match args {
        [] => Some((None, None)),
        ["--out", out] => Some((None, Some(*out))),
        [game] => Some((Some(*game), None)),
        [game, "--out", out] => Some((Some(*game), Some(*out))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_takes_a_game_folder_and_a_repo_in_that_order() {
        assert_eq!(collect_args(&[]), Some((None, None)));
        assert_eq!(collect_args(&["/wow"]), Some((Some("/wow"), None)));
        assert_eq!(collect_args(&["--out", "/r"]), Some((None, Some("/r"))));
        assert_eq!(
            collect_args(&["/wow", "--out", "/r"]),
            Some((Some("/wow"), Some("/r")))
        );
        assert_eq!(collect_args(&["/wow", "/r"]), None);
    }

    fn shot(frame_id: u16, name: &str) -> Shot {
        Shot {
            kind: "golden".into(),
            name: name.into(),
            frame_id,
            time: 1,
            payload: String::new(),
            unix: Some(1_790_300_000),
            ui_parent_scale: None,
            strip_effective_scale: None,
        }
    }

    #[test]
    fn a_file_name_keeps_only_safe_bytes_of_the_shot_name() {
        assert_eq!(file_name(&shot(3, "len-0062")), "003-len-0062.png");
        assert_eq!(file_name(&shot(4, "../../x")), "004-shot.png");
        assert_eq!(file_name(&shot(5, "")), "005-shot.png");
    }

    #[test]
    fn with_no_screenshot_call_there_is_no_window() {
        let mut never = shot(1, "a");
        never.unix = None;
        assert!(window(&[never]).is_err());
    }

    #[test]
    fn saved_variables_with_no_results_say_to_wait_for_the_run() {
        let error = Parts::read("GnomishRelaySelfTestDB = {}").err().unwrap();
        assert!(error.to_string().contains("Wait for"));
    }

    #[test]
    fn a_results_field_that_is_not_json_is_an_error() {
        let text = format!("[\"results\"] = \"{}\"", hex(b"not json"));
        assert!(Parts::read(&text).is_err());
    }
}
