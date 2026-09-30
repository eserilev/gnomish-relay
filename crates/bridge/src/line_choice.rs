//! `strip-line.json`: the mode of the strip line that the self-test chose, and the
//! screen that it fits (SPEC.md 7.1.3). `selftest collect` writes it, and each publish
//! of the bridge reads it.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use protocol::apps::{App, push_slot_global};
use serde::{Deserialize, Serialize};

use crate::fs_safe::{make_private_dir, read_at_most, write_private};
use crate::line::Mode;
use crate::run::log;

pub const FILE: &str = "strip-line.json";
const MAX_FILE: u64 = 1024;
const MAX_SIDE: u32 = 16384;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub struct LineChoice {
    /// 1 to 6, as in `line::MODES`.
    pub mode: u8,
    /// The physical screen size of the self-test.
    pub width: u32,
    pub height: u32,
}

impl LineChoice {
    fn check(self) -> Result<LineChoice> {
        if Mode::from_id(self.mode).is_none() {
            bail!("mode {} is not 1 to 6", self.mode);
        }
        let side_fits = |side: u32| (1..=MAX_SIDE).contains(&side);
        if !side_fits(self.width) || !side_fits(self.height) {
            bail!("the screen {}x{} is out of range", self.width, self.height);
        }
        Ok(self)
    }
}

/// `None` with no file. A file that fails is an error, and the caller counts it as none.
pub fn load(data: &Path) -> Result<Option<LineChoice>> {
    let path = data.join(FILE);
    let bytes = match read_at_most(&path, MAX_FILE) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        Ok(None) => bail!("{} is over {MAX_FILE} bytes", path.display()),
        Ok(Some(bytes)) => bytes,
    };
    let choice: LineChoice = serde_json::from_slice(&bytes)
        .with_context(|| format!("{} is not a line choice", path.display()))?;
    choice.check().map(Some)
}

/// `None` removes the file, so the addons go back to the old strip.
pub fn save(data: &Path, choice: Option<LineChoice>) -> Result<()> {
    let Some(choice) = choice else {
        return match fs::remove_file(data.join(FILE)) {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        };
    };
    make_private_dir(data)?;
    write_private(
        data,
        FILE,
        &(serde_json::to_string(&choice.check()?)? + "\n"),
    )
}

/// The file as each publish reads it. A bad file counts as none, and each new error
/// goes into the log once.
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
        result.ok().flatten()
    }
}

/// Adds the line after the body. It holds only decimal numbers, so S9 still covers the
/// table. With no choice, the body stays the proved one.
pub fn with_line(mut body: Vec<u8>, app: App, choice: Option<LineChoice>) -> Vec<u8> {
    let Some(LineChoice {
        mode,
        width,
        height,
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

#[cfg(test)]
mod tests {
    use protocol::slot::slot_body;

    use super::*;

    const CHOICE: LineChoice = LineChoice {
        mode: 1,
        width: 2560,
        height: 1440,
    };

    #[test]
    fn a_saved_choice_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), Some(CHOICE)).unwrap();
        assert_eq!(load(dir.path()).unwrap(), Some(CHOICE));
    }

    #[test]
    fn with_no_file_there_is_no_choice() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap(), None);
    }

    #[test]
    fn saving_no_choice_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), Some(CHOICE)).unwrap();
        save(dir.path(), None).unwrap();
        save(dir.path(), None).unwrap();
        assert_eq!(load(dir.path()).unwrap(), None);
    }

    #[test]
    fn a_file_with_a_bad_mode_a_bad_screen_or_bad_json_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        for text in [
            r#"{"mode": 7, "width": 2560, "height": 1440}"#,
            r#"{"mode": 0, "width": 2560, "height": 1440}"#,
            r#"{"mode": 1, "width": 0, "height": 1440}"#,
            r#"{"mode": 1, "width": 2560, "height": 99999}"#,
            r#"{"mode": 1}"#,
            "not json",
        ] {
            fs::write(dir.path().join(FILE), text).unwrap();
            assert!(load(dir.path()).is_err(), "{text}");
        }
    }

    #[test]
    fn a_file_over_1_kib_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE), " ".repeat(2000)).unwrap();
        assert!(load(dir.path()).is_err());
    }

    #[test]
    fn a_bad_choice_is_never_saved() {
        let dir = tempfile::tempdir().unwrap();
        let bad = LineChoice { mode: 9, ..CHOICE };
        assert!(save(dir.path(), Some(bad)).is_err());
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

        save(dir.path(), Some(CHOICE)).unwrap();

        assert_eq!(file.choice(), Some(CHOICE));
    }

    #[test]
    fn with_no_choice_the_body_does_not_change() {
        let body = slot_body(App::Relay, 0, &[]);
        assert_eq!(with_line(body.clone(), App::Relay, None), body);
    }
}
