//! `strip-line.json`: the result of the newest line tests, one for each screen size
//! (SPEC.md 7.1.3 and 7.1.4). The line test and `selftest collect` write it, and each
//! publish of the bridge reads it.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use protocol::apps::{App, push_slot_global};
use serde::{Deserialize, Serialize};

use crate::calibration::{self, Verdict};
use crate::fs_safe::{make_private_dir, read_at_most, write_private};
use crate::line::Mode;
use crate::run::log;

pub const FILE: &str = "strip-line.json";
const MAX_FILE: u64 = 4096;
const MAX_SIDE: u32 = 16384;
const MAX_SCREENS: usize = 8;

/// Why no mode reads exactly.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Blur,
    Scale,
    ColorShift,
    NotFound,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub struct LineChoice {
    /// 1 to 6, as in `line::MODES`. 0 when no mode reads exactly.
    pub mode: u8,
    /// The physical screen size of the test.
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
}

#[derive(Serialize, Deserialize, Default)]
struct Screens {
    /// The newest first.
    screens: Vec<LineChoice>,
}

impl LineChoice {
    /// The first clean mode in the order of preference. With none, mode 0 and the first
    /// failure that the test saw.
    pub fn from_verdicts(verdicts: &[(Mode, Verdict)], width: u32, height: u32) -> LineChoice {
        let (mode, reason) = match calibration::chosen(verdicts) {
            Some(mode) => (mode.id(), None),
            None => (0, Some(first_failure(verdicts))),
        };
        LineChoice {
            mode,
            width,
            height,
            reason,
        }
    }

    fn check(self) -> Result<LineChoice> {
        if self.mode != 0 && Mode::from_id(self.mode).is_none() {
            bail!("mode {} is not 0 to 6", self.mode);
        }
        let side_fits = |side: u32| (1..=MAX_SIDE).contains(&side);
        if !side_fits(self.width) || !side_fits(self.height) {
            bail!("the screen {}x{} is out of range", self.width, self.height);
        }
        Ok(self)
    }

    fn same_screen(self, other: LineChoice) -> bool {
        (self.width, self.height) == (other.width, other.height)
    }
}

/// A blur or a color shift comes from a marker at the right cell size.
fn measured_reason(verdict: Verdict) -> Option<Reason> {
    match verdict {
        Verdict::Blur { .. } => Some(Reason::Blur),
        Verdict::ColorShift { .. } => Some(Reason::ColorShift),
        Verdict::Clean { .. } | Verdict::Scale { .. } | Verdict::NotFound => None,
    }
}

/// A blur of 1-pixel cells can show the marker at another width by chance, so a scale
/// says less than a blur. "Not found" says the least.
fn first_failure(verdicts: &[(Mode, Verdict)]) -> Reason {
    if let Some(reason) = verdicts.iter().find_map(|&(_, v)| measured_reason(v)) {
        return reason;
    }
    let scaled = verdicts
        .iter()
        .any(|(_, v)| matches!(v, Verdict::Scale { .. }));
    if scaled {
        return Reason::Scale;
    }
    Reason::NotFound
}

/// The newest first. With no file, no result.
pub fn load(data: &Path) -> Result<Vec<LineChoice>> {
    let path = data.join(FILE);
    let bytes = match read_at_most(&path, MAX_FILE) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        Ok(None) => bail!("{} is over {MAX_FILE} bytes", path.display()),
        Ok(Some(bytes)) => bytes,
    };
    let file: Screens = serde_json::from_slice(&bytes)
        .with_context(|| format!("{} is not a list of line tests", path.display()))?;
    file.screens.into_iter().map(LineChoice::check).collect()
}

/// Puts `choice` first, in place of an older result of its screen size. A damaged file
/// starts over.
pub fn remember(data: &Path, choice: LineChoice) -> Result<()> {
    let choice = choice.check()?;
    let mut screens = load(data).unwrap_or_default();
    screens.retain(|old| !old.same_screen(choice));
    screens.insert(0, choice);
    screens.truncate(MAX_SCREENS);
    make_private_dir(data)?;
    let text = serde_json::to_string(&Screens { screens })? + "\n";
    write_private(data, FILE, &text)
}

/// The result that each publish sends: the newest one. A bad file counts as none, and
/// each new error goes into the log once.
pub struct LineFile {
    data: PathBuf,
    error: Option<String>,
}

impl LineFile {
    pub fn new(data: &Path) -> LineFile {
        LineFile {
            data: data.to_owned(),
            error: None,
        }
    }

    pub fn choice(&mut self) -> Option<LineChoice> {
        let result = load(&self.data);
        let error = result.as_ref().err().map(|e| format!("{e:#}"));
        if let Some(text) = error
            .as_ref()
            .filter(|&text| self.error.as_ref() != Some(text))
        {
            log(&format!("strip line off: {text}"));
        }
        self.error = error;
        result.ok()?.first().copied()
    }
}

/// Adds the line after the body. It holds only decimal numbers, so S9 still covers the
/// table. With no choice, the body stays the proved one.
pub fn with_line(mut body: Vec<u8>, app: App, choice: Option<LineChoice>) -> Vec<u8> {
    let Some(LineChoice {
        mode,
        width,
        height,
        ..
    }) = choice
    else {
        return body;
    };
    push_slot_global(&mut body, app);
    body.extend_from_slice(
        format!(".line = {{mode = {mode}, width = {width}, height = {height}}}\n").as_bytes(),
    );
    body
}

/// The newest result in the words of the player, for the Diag tab.
pub fn bar_text(choice: Option<LineChoice>) -> String {
    let Some(choice) = choice else {
        return "not measured yet. Your next message from the game measures it.".into();
    };
    if let Some(mode) = Mode::from_id(choice.mode) {
        return format!(
            "a thin line, {} px tall (mode {}), for {}x{}.",
            mode.pixels(),
            mode.id(),
            choice.width,
            choice.height
        );
    }
    let why = match choice.reason.unwrap_or(Reason::NotFound) {
        Reason::Blur => {
            "your screen blurs 1-px lines (anti-aliasing, render scale, or an upscaler)"
        }
        Reason::Scale => {
            "your screenshots are scaled (render scale below 100%, or a screen size that isn't the screenshot size)"
        }
        Reason::ColorShift => "your game changes colors (gamma, brightness, or a color filter)",
        Reason::NotFound => "the test line didn't show in the screenshot",
    };
    format!("full size, because {why}. Messages still get through.")
}

/// The `bar_text` of the file in `data`. A bad file counts as none.
pub fn bar_text_of(data: &Path) -> String {
    bar_text(load(data).ok().and_then(|screens| screens.first().copied()))
}

/// The line of `gnomish-relay status`.
pub fn status_line(data: &Path) -> String {
    format!("Colored bar: {}", bar_text_of(data))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use protocol::slot::slot_body;

    use super::*;
    use crate::line::MODES;

    const CHOICE: LineChoice = LineChoice {
        mode: 1,
        width: 2560,
        height: 1440,
        reason: None,
    };

    fn at(width: u32, height: u32) -> LineChoice {
        LineChoice {
            width,
            height,
            ..CHOICE
        }
    }

    #[test]
    fn a_remembered_choice_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        remember(dir.path(), CHOICE).unwrap();
        assert_eq!(load(dir.path()).unwrap(), [CHOICE]);
    }

    #[test]
    fn with_no_file_there_is_no_choice() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().is_empty());
        assert_eq!(LineFile::new(dir.path()).choice(), None);
    }

    #[test]
    fn a_new_result_for_a_known_screen_replaces_the_old_one_and_comes_first() {
        let dir = tempfile::tempdir().unwrap();
        remember(dir.path(), at(1920, 1080)).unwrap();
        remember(dir.path(), CHOICE).unwrap();
        let none = LineChoice {
            mode: 0,
            reason: Some(Reason::Blur),
            ..at(1920, 1080)
        };

        remember(dir.path(), none).unwrap();

        assert_eq!(load(dir.path()).unwrap(), [none, CHOICE]);
        assert_eq!(LineFile::new(dir.path()).choice(), Some(none));
    }

    #[test]
    fn the_file_keeps_the_last_8_screens() {
        let dir = tempfile::tempdir().unwrap();
        for width in 1000..1010 {
            remember(dir.path(), at(width, 720)).unwrap();
        }
        let screens = load(dir.path()).unwrap();
        assert_eq!(screens.len(), 8);
        assert_eq!(screens[0].width, 1009);
    }

    #[test]
    fn a_file_with_a_bad_mode_a_bad_screen_or_bad_json_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        for text in [
            r#"{"screens": [{"mode": 7, "width": 2560, "height": 1440}]}"#,
            r#"{"screens": [{"mode": 1, "width": 0, "height": 1440}]}"#,
            r#"{"screens": [{"mode": 1, "width": 2560, "height": 99999}]}"#,
            r#"{"screens": [{"mode": 0, "width": 2560, "height": 1440, "reason": "fog"}]}"#,
            r#"{"screens": [{"mode": 1}]}"#,
            r#"{"mode": 1, "width": 2560, "height": 1440}"#,
            "not json",
        ] {
            fs::write(dir.path().join(FILE), text).unwrap();
            assert!(load(dir.path()).is_err(), "{text}");
        }
    }

    #[test]
    fn a_file_over_4_kib_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE), " ".repeat(5000)).unwrap();
        assert!(load(dir.path()).is_err());
    }

    #[test]
    fn a_damaged_file_starts_over_at_the_next_result() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE), "not json").unwrap();
        remember(dir.path(), CHOICE).unwrap();
        assert_eq!(load(dir.path()).unwrap(), [CHOICE]);
    }

    #[test]
    fn a_bad_choice_is_never_saved() {
        let dir = tempfile::tempdir().unwrap();
        let bad = LineChoice { mode: 9, ..CHOICE };
        assert!(remember(dir.path(), bad).is_err());
        assert!(!dir.path().join(FILE).exists());
    }

    #[test]
    fn the_line_goes_after_the_body_of_each_app() {
        for (app, global) in [
            (App::Relay, "GnomishRelay_SlotData"),
            (App::Timeways, "Timeways_SlotData"),
        ] {
            let body = with_line(slot_body(app, 0, &[]), app, Some(CHOICE));
            let text = String::from_utf8(body).unwrap();
            let line = format!("}}}}\n{global}.line = {{mode = 1, width = 2560, height = 1440}}\n");
            assert!(text.ends_with(&line), "{text}");
        }
    }

    #[test]
    fn a_bad_file_counts_as_no_choice_until_it_is_fixed() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = LineFile::new(dir.path());
        fs::write(dir.path().join(FILE), "not json").unwrap();
        assert_eq!(file.choice(), None);
        assert_eq!(file.choice(), None);

        remember(dir.path(), CHOICE).unwrap();

        assert_eq!(file.choice(), Some(CHOICE));
    }

    #[test]
    fn with_no_choice_the_body_does_not_change() {
        let body = slot_body(App::Relay, 0, &[]);
        assert_eq!(with_line(body.clone(), App::Relay, None), body);
    }

    #[test]
    fn the_choice_is_the_first_clean_mode() {
        let clean = Verdict::Clean { max_error: 0 };
        let verdicts = [(MODES[0], Verdict::NotFound), (MODES[1], clean)];

        let choice = LineChoice::from_verdicts(&verdicts, 1280, 720);

        assert_eq!((choice.mode, choice.reason), (2, None));
    }

    #[test]
    fn with_no_clean_mode_the_reason_is_the_first_failure_that_was_found() {
        let blur = Verdict::Blur {
            wrong: 3,
            max_error: 90,
        };
        let verdicts = [(MODES[0], Verdict::NotFound), (MODES[3], blur)];

        let choice = LineChoice::from_verdicts(&verdicts, 1280, 720);

        assert_eq!((choice.mode, choice.reason), (0, Some(Reason::Blur)));
        let nothing = LineChoice::from_verdicts(&verdicts[..1], 1280, 720);
        assert_eq!(nothing.reason, Some(Reason::NotFound));
    }

    #[test]
    fn a_blur_is_the_reason_before_a_scale_of_an_earlier_mode() {
        let scale = Verdict::Scale { across: 1.5 };
        let blur = Verdict::Blur {
            wrong: 3,
            max_error: 90,
        };

        let both = LineChoice::from_verdicts(&[(MODES[1], scale), (MODES[3], blur)], 1280, 720);
        let only_scale = LineChoice::from_verdicts(&[(MODES[1], scale)], 1280, 720);

        assert_eq!(both.reason, Some(Reason::Blur));
        assert_eq!(only_scale.reason, Some(Reason::Scale));
    }

    #[test]
    fn the_bar_text_names_the_line_or_says_why_the_bar_is_full_size() {
        let big = |reason| LineChoice {
            mode: 0,
            reason: Some(reason),
            ..CHOICE
        };
        assert_eq!(
            bar_text(Some(CHOICE)),
            "a thin line, 1 px tall (mode 1), for 2560x1440."
        );
        assert_eq!(
            bar_text(Some(big(Reason::Blur))),
            "full size, because your screen blurs 1-px lines (anti-aliasing, render scale, or an upscaler). Messages still get through."
        );
        assert!(bar_text(Some(big(Reason::Scale))).contains("scaled"));
        assert!(bar_text(Some(big(Reason::ColorShift))).contains("gamma"));
        assert!(bar_text(Some(big(Reason::NotFound))).contains("didn't show"));
        assert!(bar_text(None).starts_with("not measured yet."));
    }

    #[test]
    fn the_status_line_reads_the_newest_result_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(status_line(dir.path()).starts_with("Colored bar: not measured yet."));

        remember(dir.path(), CHOICE).unwrap();

        assert_eq!(
            status_line(dir.path()),
            "Colored bar: a thin line, 1 px tall (mode 1), for 2560x1440."
        );
    }
}
