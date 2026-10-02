//! `gnomish-relay selftest collect`: copies the results of the self-test addon into the
//! repo (SPEC.md 14.3). It reads only the saved variables of the self-test and its own
//! screenshots. It never reads the key or the config of the relay.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::calibration::{self, Verdict};
use crate::dirs::Dirs;
use crate::fixture::{self, PLACEHOLDER};
use crate::game_choice;
use crate::ids::hex;
use crate::line::{CELLS_PER_ROW, Mode};
use crate::line_choice::{self, LineChoice};
use crate::saved;
use crate::vectors::{self, MANIFEST, Manifest, Shot, TEST_KEY, Vector};
use crate::wow_client::WowClient;

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
    }))
}

/// The PNG files from the time of the run, by name.
fn pngs_of_the_run(dir: &Path, shots: &[Shot]) -> Result<Vec<PathBuf>> {
    let window = window(shots)?;
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_png(path) && in_window(path, window))
        .collect();
    paths.sort();
    Ok(paths)
}

/// The screenshots of the self-test, one per shot, by the frame that each one holds. The
/// file names in the saved variables are never used: that file is untrusted text.
pub fn find(dir: &Path, shots: &[Shot]) -> Result<Vec<Found>> {
    let paths = pngs_of_the_run(dir, shots)?;
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
        let corner = vectors::strip_corner(&vectors::read_png(&one.path)?)?;
        fs::write(dir.join(&file), corner.to_png()?)?;
        let (width, height) = corner.size();
        vectors.push(Vector {
            file,
            shot: one.shot.clone(),
            width,
            height,
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

/// The best verdict of each line mode over the screenshots of the run (SPEC.md 14.3.1).
/// A screenshot is judged by the marker that it shows, so a line that reads wrong still
/// gets a verdict.
pub fn judge_lines(dir: &Path, shots: &[Shot]) -> Result<Vec<(Mode, Verdict)>> {
    let mut lines = Vec::new();
    for shot in shots.iter().filter(|s| s.kind == "line") {
        let mode = shot
            .mode
            .and_then(Mode::from_id)
            .context("a line shot has no mode")?;
        lines.push((mode, vectors::frame_of(shot)?, Verdict::NotFound));
    }
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    for path in pngs_of_the_run(dir, shots)? {
        let Ok(image) = vectors::read_png(&path) else {
            continue;
        };
        for (mode, frame, verdict) in &mut lines {
            *verdict = verdict.better(calibration::judge(&image, *mode, frame));
        }
    }
    Ok(lines.into_iter().map(|(mode, _, v)| (mode, v)).collect())
}

/// The physical screen size that the self-test measured.
fn screen(results: &Value) -> Option<(u32, u32)> {
    let side = |i: usize| {
        let value = results
            .pointer(&format!("/screen/physical/{i}"))?
            .as_u64()?;
        u32::try_from(value).ok()
    };
    Some((side(0)?, side(1)?))
}

/// The smallest clean mode, or mode 0 with a reason, for the screen of the run.
fn line_choice(lines: &[(Mode, Verdict)], screen: Option<(u32, u32)>) -> Option<LineChoice> {
    let (width, height) = screen?;
    if lines.is_empty() {
        return None;
    }
    Some(LineChoice::from_verdicts(lines, width, height))
}

pub struct Collected {
    pub build: String,
    pub fixture: PathBuf,
    pub vectors: usize,
    /// Shots with no screenshot in the Screenshots folder. The line shots have verdicts instead.
    pub missing: Vec<String>,
    pub capture: fixture::Capture,
    /// Empty when the run drew no line.
    pub lines: Vec<(Mode, Verdict)>,
    /// The physical screen size of the run, which the line fits.
    pub screen: Option<(u32, u32)>,
    pub line: Option<LineChoice>,
}

impl Collected {
    /// The report of the line modes, for the player (SPEC.md 14.3.1).
    pub fn line_report(&self) -> Vec<String> {
        if self.lines.is_empty() {
            return vec![
                "The self-test drew no strip line. Link the new self-test and run it again.".into(),
            ];
        }
        let screen = self
            .screen
            .map_or("an unknown screen".to_owned(), |(w, h)| {
                format!("a screen of {w}x{h}")
            });
        let mut report = vec![format!("Strip line modes, for {screen}:")];
        for (mode, verdict) in &self.lines {
            report.push(format!(
                "  mode {} ({}): {}",
                mode.id(),
                mode.name(),
                verdict.describe()
            ));
        }
        report.push(chosen_line(self.line));
        report
    }

    /// Writes the result for the bridge, as the line test of the addon does. A run with
    /// no line changes nothing.
    pub fn save_line(&self, data: &Path) -> Result<()> {
        match self.line {
            Some(line) => line_choice::remember(data, line),
            None => Ok(()),
        }
    }
}

fn chosen_line(line: Option<LineChoice>) -> String {
    let Some(mode) = line.and_then(|l| Mode::from_id(l.mode)) else {
        return "No mode reads exactly, so the addons keep the old strip. Fix the cause above, then run the self-test again.".into();
    };
    let p = mode.pixels();
    format!(
        "Chosen: mode {} ({}). The strip is now a line {p} px tall and {} px wide for most messages.",
        mode.id(),
        mode.name(),
        CELLS_PER_ROW * p
    )
}

/// Writes `fixture` under the name of its client (SPEC.md 7.9). Only Forever has a
/// placeholder, so only a Forever fixture deletes it.
fn write_fixture(fixtures: &Path, fixture: &fixture::Fixture) -> Result<PathBuf> {
    fs::create_dir_all(fixtures)?;
    let client = fixture::client(fixture)?;
    let path = fixtures.join(fixture::file_name(client, &fixture.build));
    fs::write(&path, serde_json::to_string_pretty(fixture)? + "\n")?;
    if client == WowClient::Forever {
        let _ = fs::remove_file(fixtures.join(PLACEHOLDER));
    }
    Ok(path)
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
    let path = write_fixture(&repo.join("tests").join("fixtures"), &fixture)?;
    let dir = repo.join("tests").join("vectors").join(&fixture.build);
    let vectors = write_vectors(&dir, &fixture.build, &found, &saved_file)?;
    let lines = judge_lines(&game.join("Screenshots"), &shots)?;
    let missing = shots
        .iter()
        .filter(|s| s.kind != "hide_after_call" && s.kind != "line")
        .filter(|s| !found.iter().any(|f| f.shot.frame_id == s.frame_id))
        .map(|s| s.name.clone())
        .collect();
    Ok(Collected {
        build: fixture.build,
        fixture: path,
        vectors,
        missing,
        capture: fixture.fake.capture,
        line: line_choice(&lines, screen(&parts.results)),
        lines,
        screen: screen(&parts.results),
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
    let collected = collect(&game_choice::require(dirs, game)?, &repo)?;
    println!("wrote {}", collected.fixture.display());
    println!(
        "wrote {} golden vectors to tests/vectors/{}",
        collected.vectors, collected.build
    );
    for name in &collected.missing {
        println!("no screenshot of {name}");
    }
    println!("capture: {:?}", collected.capture);
    for line in collected.line_report() {
        println!("{line}");
    }
    collected.save_line(&dirs.data)?;
    if collected.line.is_some() {
        println!(
            "wrote {}. The bridge sends it to the addons at its next publish.",
            dirs.data.join(line_choice::FILE).display()
        );
    }
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

    const PLACEHOLDER_JSON: &str = include_str!("../../../tests/fixtures/forever-placeholder.json");

    /// The placeholder fixture as if a client with `interface` measured it.
    fn fixture_of(interface: u32) -> fixture::Fixture {
        let mut fixture: fixture::Fixture = serde_json::from_str(PLACEHOLDER_JSON).unwrap();
        fixture.fake.build_info.interface = interface;
        fixture.build = "2.5.6.69795".into();
        fixture
    }

    #[test]
    fn an_anniversary_fixture_is_named_after_its_client_and_keeps_the_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(PLACEHOLDER), "{}").unwrap();

        let path = write_fixture(dir.path(), &fixture_of(20506)).unwrap();

        assert_eq!(path, dir.path().join("anniversary-2.5.6.69795.json"));
        assert!(dir.path().join(PLACEHOLDER).exists());
    }

    #[test]
    fn a_fixture_of_an_unknown_client_is_refused() {
        let dir = tempfile::tempdir().unwrap();

        let error = write_fixture(dir.path(), &fixture_of(120_001)).unwrap_err();

        assert!(format!("{error:#}").contains("120001"), "{error:#}");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

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
            mode: None,
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
