//! The fixture of one client build: what the self-test measured, and the behavior of
//! the fake game that follows from it (SPEC.md 14.3). The tests of the addon read the
//! newest fixture, so the fake game acts as the real game did.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::vectors::Shot;
use crate::wow_client::WowClient;

pub const FORMAT: u32 = 1;
/// The guesses for Forever from before the self-test. No other client has one.
pub const PLACEHOLDER: &str = "forever-placeholder.json";

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Fixture {
    pub format: u32,
    pub placeholder: bool,
    pub build: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub fake: Fake,
    pub measured: Value,
}

/// Each field is one behavior of the fake game in `addon/tests/wow.lua`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Fake {
    pub build_info: BuildInfo,
    pub physical_screen: [u32; 2],
    /// Seconds from `Screenshot()` to its event: the slowest shot, since the addon must survive it.
    pub shot_delay: f64,
    pub shot_event: String,
    pub capture: Capture,
    pub status_shown: StatusShown,
    pub load_addon: LoadReturns,
    pub set_font: SetFontReturns,
    /// False when `GetContentHeight` is 0 right after `SetText`.
    pub content_height_at_once: bool,
    pub saved_variables: SavedVariables,
    pub login_events: Vec<String>,
    pub timers_due_together: TimerOrder,
    pub bit: BitResults,
    pub hook_missing_global: HookMissing,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct BuildInfo {
    pub version: String,
    pub build: String,
    pub date: String,
    pub interface: u32,
}

/// When the picture of a screenshot is taken.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Capture {
    /// In the `Screenshot()` call: a strip hidden right after the call is in the picture.
    Call,
    /// After the handler that called `Screenshot()`.
    AfterHandler,
}

/// When the "Screen captured" text shows through `ActionStatus`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum StatusShown {
    Never,
    BeforeEvent,
    AfterEvent,
}

/// What `C_AddOns.LoadAddOn` returns. `None` is nil.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Load {
    pub loaded: Option<bool>,
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct LoadReturns {
    pub present: Load,
    pub again: Load,
    pub missing: Load,
    pub disabled: Load,
    /// A load right after `EnableAddOn` of a disabled addon, as `Slots.lua` does it.
    pub enabled_then_loaded: Load,
    pub out_of_date: Load,
}

/// What `FontString:SetFont` returns. `None` is nil.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SetFontReturns {
    pub present: Option<bool>,
    pub missing: MissingFont,
}

/// What `FontString:SetFont` does with a file that WoW did not find at launch.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum MissingFont {
    /// Forever. `None` is nil.
    Returns(Option<bool>),
    /// TBC Anniversary: "Invalid font asset".
    Raises,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum SavedVariables {
    BeforeFiles,
    AfterFiles,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum TimerOrder {
    /// Timers that are due at the same time run in the order of their start.
    Fifo,
    Other,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum BitResults {
    Unsigned,
    Signed,
}

/// What `hooksecurefunc` does with the name of a global that does not exist.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum HookMissing {
    MakesGlobal,
    Ignores,
    Error,
}

/// The parts of the results that the fake game needs. The addon writes them.
#[derive(Deserialize)]
struct Results {
    format: u32,
    client: BuildInfo,
    screen: Screen,
    lua: LuaFacts,
    addons: AddOns,
    fonts: Fonts,
    timing: Timing,
    shots: Vec<TimedShot>,
}

#[derive(Deserialize)]
struct Screen {
    physical: [u32; 2],
}

#[derive(Deserialize)]
struct LuaFacts {
    bit_lshift_1_31: f64,
    hooksecurefunc_missing: HookProbe,
}

#[derive(Deserialize)]
struct HookProbe {
    returns: Vec<Value>,
    global_after: String,
}

#[derive(Deserialize)]
struct LoadProbe {
    returns: Vec<Value>,
}

#[derive(Deserialize)]
struct Present {
    first: LoadProbe,
    again: LoadProbe,
}

#[derive(Deserialize)]
struct Disabled {
    disabled: LoadProbe,
    enabled_then_loaded: LoadProbe,
}

#[derive(Deserialize)]
struct AddOns {
    present: Present,
    missing: LoadProbe,
    disabled: Disabled,
    out_of_date: LoadProbe,
}

#[derive(Deserialize)]
struct Fonts {
    font_string_set_font: SetFontProbe,
    two_blocks: Heights,
}

#[derive(Deserialize)]
struct SetFontProbe {
    present: Vec<Value>,
    missing: Vec<Value>,
}

#[derive(Deserialize)]
struct Heights {
    at_once: f64,
}

#[derive(Deserialize)]
struct Timing {
    same_time_order: Vec<String>,
}

#[derive(Deserialize)]
pub struct TimedShot {
    #[serde(flatten)]
    pub shot: Shot,
    pub ok: Option<bool>,
    pub events: Vec<ShotEvent>,
    pub status_shown_ms: Option<i64>,
}

#[derive(Deserialize)]
pub struct ShotEvent {
    pub event: String,
    pub ms: i64,
}

#[derive(Deserialize)]
struct LoadPart {
    sessions: std::collections::BTreeMap<String, Session>,
}

#[derive(Deserialize)]
struct Session {
    saved_at_file_load: String,
    saved_at_addon_loaded: Option<String>,
    events: Vec<SessionEvent>,
}

#[derive(Deserialize)]
struct SessionEvent {
    event: String,
}

/// The values after the `ok` of `pcall`. An error in the call has no values.
fn returned<'a>(list: &'a [Value], what: &str) -> Result<&'a [Value]> {
    match list.split_first() {
        Some((Value::Bool(true), rest)) => Ok(rest),
        _ => bail!("{what} raised an error in the game: {list:?}"),
    }
}

fn load(probe: &LoadProbe, what: &str) -> Result<Load> {
    let values = returned(&probe.returns, what)?;
    Ok(Load {
        loaded: values.first().and_then(Value::as_bool),
        reason: values.get(1).and_then(Value::as_str).map(str::to_owned),
    })
}

fn load_returns(addons: &AddOns) -> Result<LoadReturns> {
    Ok(LoadReturns {
        present: load(&addons.present.first, "LoadAddOn of a present addon")?,
        again: load(&addons.present.again, "a second LoadAddOn")?,
        missing: load(&addons.missing, "LoadAddOn of a missing addon")?,
        disabled: load(&addons.disabled.disabled, "LoadAddOn of a disabled addon")?,
        enabled_then_loaded: load(
            &addons.disabled.enabled_then_loaded,
            "LoadAddOn after EnableAddOn",
        )?,
        out_of_date: load(&addons.out_of_date, "LoadAddOn of an out-of-date addon")?,
    })
}

fn set_font(probe: &SetFontProbe) -> Result<SetFontReturns> {
    let present = returned(&probe.present, "SetFont with a present file")?;
    Ok(SetFontReturns {
        present: present.first().and_then(Value::as_bool),
        missing: missing_font(&probe.missing)?,
    })
}

/// The `pcall` of `SetFont` with a missing file: its returns, or an error in the call.
fn missing_font(pcall: &[Value]) -> Result<MissingFont> {
    match pcall.split_first() {
        Some((Value::Bool(true), rest)) => {
            Ok(MissingFont::Returns(rest.first().and_then(Value::as_bool)))
        }
        Some((Value::Bool(false), _)) => Ok(MissingFont::Raises),
        _ => bail!("SetFont with a missing file has no pcall result: {pcall:?}"),
    }
}

fn golden(shots: &[TimedShot]) -> impl Iterator<Item = &TimedShot> {
    shots
        .iter()
        .filter(|s| s.shot.kind == "golden" && s.ok == Some(true))
}

fn shot_delay(shots: &[TimedShot]) -> Result<f64> {
    let slowest = golden(shots)
        .filter_map(|s| s.events.first().map(|e| e.ms))
        .max()
        .context("no golden shot got a screenshot event")?;
    #[allow(clippy::cast_precision_loss)] // milliseconds of one shot
    Ok(slowest as f64 / 1000.0)
}

fn shot_event(shots: &[TimedShot]) -> Result<String> {
    golden(shots)
        .find_map(|s| s.events.first().map(|e| e.event.clone()))
        .context("no golden shot got a screenshot event")
}

fn status_shown(shots: &[TimedShot]) -> StatusShown {
    let first = golden(shots).find_map(|s| Some((s.status_shown_ms?, s.events.first()?.ms)));
    match first {
        None => StatusShown::Never,
        Some((status, event)) if status <= event => StatusShown::BeforeEvent,
        Some(_) => StatusShown::AfterEvent,
    }
}

fn saved_variables(part: &LoadPart) -> Result<SavedVariables> {
    let known = part
        .sessions
        .values()
        .find(|s| s.saved_at_addon_loaded.as_deref() == Some("table"))
        .context("no session had a saved variables file yet. Type /reload in the game once more, then collect again")?;
    Ok(if known.saved_at_file_load == "table" {
        SavedVariables::BeforeFiles
    } else {
        SavedVariables::AfterFiles
    })
}

fn login_events(part: &LoadPart) -> Result<Vec<String>> {
    let session = part
        .sessions
        .get("login")
        .or_else(|| part.sessions.get("reload"))
        .context("no session recorded its login events")?;
    Ok(session.events.iter().map(|e| e.event.clone()).collect())
}

fn timer_order(timing: &Timing) -> TimerOrder {
    if timing.same_time_order == ["first", "second", "third"] {
        TimerOrder::Fifo
    } else {
        TimerOrder::Other
    }
}

fn bit_results(lua: &LuaFacts) -> Result<BitResults> {
    match lua.bit_lshift_1_31 {
        x if (x - 2_147_483_648.0).abs() < 0.5 => Ok(BitResults::Unsigned),
        x if (x + 2_147_483_648.0).abs() < 0.5 => Ok(BitResults::Signed),
        x => bail!("bit.lshift(1, 31) gave {x}"),
    }
}

fn hook_missing(probe: &HookProbe) -> HookMissing {
    match probe.returns.first() {
        Some(Value::Bool(true)) if probe.global_after == "function" => HookMissing::MakesGlobal,
        Some(Value::Bool(true)) => HookMissing::Ignores,
        _ => HookMissing::Error,
    }
}

fn fake(results: &Results, part: &LoadPart, probe_in_picture: bool) -> Result<Fake> {
    Ok(Fake {
        build_info: results.client.clone(),
        physical_screen: results.screen.physical,
        shot_delay: shot_delay(&results.shots)?,
        shot_event: shot_event(&results.shots)?,
        capture: if probe_in_picture {
            Capture::Call
        } else {
            Capture::AfterHandler
        },
        status_shown: status_shown(&results.shots),
        load_addon: load_returns(&results.addons)?,
        set_font: set_font(&results.fonts.font_string_set_font)?,
        content_height_at_once: results.fonts.two_blocks.at_once > 0.0,
        saved_variables: saved_variables(part)?,
        login_events: login_events(part)?,
        timers_due_together: timer_order(&results.timing),
        bit: bit_results(&results.lua)?,
        hook_missing_global: hook_missing(&results.lua.hooksecurefunc_missing),
    })
}

/// The path and the text of each `error` field. The addon writes one for a part that failed.
fn errors(value: &Value, path: &str) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(Value::String(text)) = value.get("error") {
        found.push(format!("{path}: {text}"));
    }
    if let Value::Object(fields) = value {
        for (key, field) in fields {
            found.extend(errors(field, &format!("{path}.{key}")));
        }
    }
    found
}

/// Only digits and dots, so a build name is safe in a file name.
pub fn build_name(info: &BuildInfo) -> Result<String> {
    let name = format!("{}.{}", info.version, info.build);
    let digits = name
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if !digits {
        bail!("the client build {name:?} is not digits and dots");
    }
    Ok(name)
}

/// The shots of the results, for the golden vectors.
pub fn shots(results: &Value) -> Result<Vec<TimedShot>> {
    let shots = results.get("shots").context("the results have no shots")?;
    Ok(serde_json::from_value(shots.clone())?)
}

/// The results without the payloads, which go into the manifest of the vectors.
fn measured(results: &Value, load: &Value, combat: Option<&Value>) -> Value {
    let mut measured = results.clone();
    if let Some(Value::Array(shots)) = measured.get_mut("shots") {
        for shot in shots.iter_mut().filter_map(Value::as_object_mut) {
            shot.remove("payload");
        }
    }
    if let Some(object) = measured.as_object_mut() {
        object.insert("load".into(), load.clone());
        object.insert("combat".into(), combat.cloned().unwrap_or(Value::Null));
    }
    measured
}

/// The fixture of the self-test results. `probe_in_picture` says whether the strip that
/// hid right after its `Screenshot()` call is in a picture.
pub fn build(
    results: &Value,
    load: &Value,
    combat: Option<&Value>,
    probe_in_picture: bool,
) -> Result<Fixture> {
    let typed: Results = serde_json::from_value(results.clone()).with_context(|| {
        format!(
            "the results have a wrong shape. Errors in the game: {:?}",
            errors(results, "")
        )
    })?;
    if typed.format != FORMAT {
        bail!("the results have format {}, not {FORMAT}", typed.format);
    }
    let part: LoadPart =
        serde_json::from_value(load.clone()).context("the load order has a wrong shape")?;
    Ok(Fixture {
        format: FORMAT,
        placeholder: false,
        build: build_name(&typed.client)?,
        note: None,
        fake: fake(&typed, &part, probe_in_picture)?,
        measured: measured(results, load, combat),
    })
}

fn version_parts(build: &str) -> Option<Vec<u64>> {
    build.split('.').map(|part| part.parse().ok()).collect()
}

/// The file name of a fixture, such as `anniversary-2.5.6.69795.json` (SPEC.md 7.9).
pub fn file_name(client: WowClient, build: &str) -> String {
    format!("{}-{build}.json", client.name())
}

/// The client of a fixture, from the interface number of its build.
pub fn client(fixture: &Fixture) -> Result<WowClient> {
    let interface = fixture.fake.build_info.interface;
    WowClient::of_interface(interface)
        .with_context(|| format!("no supported client has the interface {interface}"))
}

/// Each real fixture of `client` in `dir` with its version numbers, oldest first. The
/// placeholder is not one.
pub fn real_fixtures(dir: &Path, client: WowClient) -> Result<Vec<(Vec<u64>, PathBuf)>> {
    let prefix = format!("{}-", client.name());
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let build = name
            .strip_prefix(&prefix)
            .and_then(|n| n.strip_suffix(".json"));
        if let Some(version) = build.and_then(version_parts) {
            found.push((version, entry.path()));
        }
    }
    found.sort();
    Ok(found)
}

/// The fixture of the newest Forever build in `dir`, or the placeholder while there is
/// none. The fake game of the tests is Forever, the first client.
pub fn newest(dir: &Path) -> Result<PathBuf> {
    let newest = real_fixtures(dir, WowClient::Forever)?.pop();
    Ok(newest.map_or_else(|| dir.join(PLACEHOLDER), |(_, path)| path))
}

pub fn read(path: &Path) -> Result<Fixture> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} has a wrong shape", path.display()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn shot(kind: &str, event_ms: Option<i64>, status_ms: Option<i64>) -> TimedShot {
        let events = event_ms.map_or(
            json!([]),
            |ms| json!([{"event": "SCREENSHOT_SUCCEEDED", "ms": ms}]),
        );
        serde_json::from_value(json!({
            "kind": kind, "name": "n", "frame_id": 1, "time": 1, "payload": "",
            "unix": 1, "ok": true, "events": events, "status_shown_ms": status_ms,
        }))
        .unwrap()
    }

    #[test]
    fn the_shot_delay_is_the_slowest_golden_shot() {
        let shots = [
            shot("golden", Some(300), None),
            shot("golden", Some(1200), None),
            shot("combat", Some(5000), None),
        ];
        assert!((shot_delay(&shots).unwrap() - 1.2).abs() < 1e-9);
    }

    #[test]
    fn with_no_screenshot_event_there_is_no_delay() {
        assert!(shot_delay(&[shot("golden", None, None)]).is_err());
        assert!(shot_event(&[shot("golden", None, None)]).is_err());
    }

    #[test]
    fn the_screen_captured_text_shows_before_or_after_the_event_or_never() {
        let never = [shot("golden", Some(400), None)];
        let before = [shot("golden", Some(400), Some(10))];
        let after = [shot("golden", Some(400), Some(900))];
        assert_eq!(status_shown(&never), StatusShown::Never);
        assert_eq!(status_shown(&before), StatusShown::BeforeEvent);
        assert_eq!(status_shown(&after), StatusShown::AfterEvent);
    }

    #[test]
    fn a_call_that_raised_an_error_has_no_return_values() {
        assert!(returned(&[json!(false), json!("boom")], "x").is_err());
        assert!(returned(&[], "x").is_err());
        assert_eq!(returned(&[json!(true), json!(1)], "x").unwrap(), [json!(1)]);
    }

    fn facts(lshift: f64, returns: Vec<Value>, after: &str) -> LuaFacts {
        LuaFacts {
            bit_lshift_1_31: lshift,
            hooksecurefunc_missing: HookProbe {
                returns,
                global_after: after.into(),
            },
        }
    }

    #[test]
    fn bit_results_are_unsigned_or_signed_or_an_error() {
        let unsigned = facts(2_147_483_648.0, vec![], "nil");
        let signed = facts(-2_147_483_648.0, vec![], "nil");
        assert_eq!(bit_results(&unsigned).unwrap(), BitResults::Unsigned);
        assert_eq!(bit_results(&signed).unwrap(), BitResults::Signed);
        assert!(bit_results(&facts(0.0, vec![], "nil")).is_err());
    }

    #[test]
    fn a_set_font_that_raises_for_a_missing_file_is_kept_as_raises() {
        let raised = [json!(false), json!("Invalid font asset")];
        assert_eq!(missing_font(&raised).unwrap(), MissingFont::Raises);
        assert_eq!(
            missing_font(&[json!(true), json!(false)]).unwrap(),
            MissingFont::Returns(Some(false))
        );
        assert_eq!(
            missing_font(&[json!(true)]).unwrap(),
            MissingFont::Returns(None)
        );
    }

    #[test]
    fn a_set_font_probe_with_no_pcall_result_is_refused() {
        assert!(missing_font(&[]).is_err());
        assert!(missing_font(&[json!("garbled")]).is_err());
    }

    #[test]
    fn a_hook_on_a_missing_global_makes_it_or_ignores_it_or_fails() {
        let makes = facts(0.0, vec![json!(true)], "function");
        let ignores = facts(0.0, vec![json!(true)], "nil");
        let fails = facts(0.0, vec![json!(false), json!("no")], "nil");
        assert_eq!(
            hook_missing(&makes.hooksecurefunc_missing),
            HookMissing::MakesGlobal
        );
        assert_eq!(
            hook_missing(&ignores.hooksecurefunc_missing),
            HookMissing::Ignores
        );
        assert_eq!(
            hook_missing(&fails.hooksecurefunc_missing),
            HookMissing::Error
        );
    }

    #[test]
    fn timers_run_in_start_order_only_when_the_measured_order_says_so() {
        let order = |names: &[&str]| Timing {
            same_time_order: names.iter().map(|n| (*n).to_owned()).collect(),
        };
        let fifo = order(&["first", "second", "third"]);
        let other = order(&["second", "first", "third"]);
        assert_eq!(timer_order(&fifo), TimerOrder::Fifo);
        assert_eq!(timer_order(&other), TimerOrder::Other);
    }

    fn load_part(file: &str) -> LoadPart {
        serde_json::from_value(json!({"sessions": {"reload": {
            "saved_at_file_load": file,
            "saved_at_addon_loaded": "table",
            "events": [{"event": "ADDON_LOADED"}],
        }}}))
        .unwrap()
    }

    #[test]
    fn saved_variables_before_the_files_show_as_a_table_at_file_load() {
        assert_eq!(
            saved_variables(&load_part("table")).unwrap(),
            SavedVariables::BeforeFiles
        );
        assert_eq!(
            saved_variables(&load_part("nil")).unwrap(),
            SavedVariables::AfterFiles
        );
        assert_eq!(login_events(&load_part("nil")).unwrap(), ["ADDON_LOADED"]);
    }

    #[test]
    fn a_part_that_failed_in_the_game_is_named_in_the_error() {
        let results = json!({"format": 1, "addons": {"error": "LoadAddOn is nil"}});

        let error = build(&results, &json!({}), None, true).unwrap_err();

        assert!(format!("{error:#}").contains(".addons: LoadAddOn is nil"));
    }

    #[test]
    fn a_build_name_is_only_digits_and_dots() {
        let info = |version: &str, build: &str| BuildInfo {
            version: version.into(),
            build: build.into(),
            date: String::new(),
            interface: 0,
        };
        assert_eq!(
            build_name(&info("1.60.1", "70009")).unwrap(),
            "1.60.1.70009"
        );
        assert!(build_name(&info("../..", "1")).is_err());
        assert!(build_name(&info("1..2", "3")).is_err());
        assert!(build_name(&info("1.60/x", "3")).is_err());
    }

    #[test]
    fn the_newest_fixture_compares_versions_as_numbers_and_skips_the_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(newest(dir.path()).unwrap(), dir.path().join(PLACEHOLDER));
        let names = [
            PLACEHOLDER,
            "forever-1.60.9.1.json",
            "forever-1.60.10.1.json",
            "anniversary-2.5.7.1.json",
            "notes.json",
        ];
        for name in names {
            fs::write(dir.path().join(name), "{}").unwrap();
        }
        let newest = newest(dir.path()).unwrap();
        assert_eq!(newest, dir.path().join("forever-1.60.10.1.json"));
        assert_eq!(
            real_fixtures(dir.path(), WowClient::Forever).unwrap().len(),
            2
        );
        let anniversary = real_fixtures(dir.path(), WowClient::Anniversary).unwrap();
        assert_eq!(anniversary.len(), 1);
    }
}
