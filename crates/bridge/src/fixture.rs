//! The fixture of one client build: what the self-test measured, and the behavior of
//! the fake game that follows from it (SPEC.md 14.3). The tests of the addon read the
//! newest fixture, so the fake game acts as the real game did.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FORMAT: u32 = 1;
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
    pub missing: Option<bool>,
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

fn version_parts(build: &str) -> Option<Vec<u64>> {
    build.split('.').map(|part| part.parse().ok()).collect()
}

/// Each real fixture in `dir` with its version numbers, oldest first. The placeholder is not one.
pub fn real_fixtures(dir: &Path) -> Result<Vec<(Vec<u64>, PathBuf)>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let build = name
            .strip_prefix("forever-")
            .and_then(|n| n.strip_suffix(".json"));
        if let Some(version) = build.and_then(version_parts) {
            found.push((version, entry.path()));
        }
    }
    found.sort();
    Ok(found)
}

/// The fixture of the newest build in `dir`, or the placeholder while there is none.
pub fn newest(dir: &Path) -> Result<PathBuf> {
    let newest = real_fixtures(dir)?.pop();
    Ok(newest.map_or_else(|| dir.join(PLACEHOLDER), |(_, path)| path))
}

pub fn read(path: &Path) -> Result<Fixture> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} has a wrong shape", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_fixture_compares_versions_as_numbers_and_skips_the_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(newest(dir.path()).unwrap(), dir.path().join(PLACEHOLDER));
        let names = [
            PLACEHOLDER,
            "forever-1.60.9.1.json",
            "forever-1.60.10.1.json",
            "notes.json",
        ];
        for name in names {
            fs::write(dir.path().join(name), "{}").unwrap();
        }
        let newest = newest(dir.path()).unwrap();
        assert_eq!(newest, dir.path().join("forever-1.60.10.1.json"));
        assert_eq!(real_fixtures(dir.path()).unwrap().len(), 2);
    }
}
