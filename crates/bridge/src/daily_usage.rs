//! The tokens and the cost of each day, and the daily cap (SPEC.md 9.10). A day is the
//! UTC date: the bridge has no time zone database.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::fs_safe::{read_at_most, write_private};
use crate::iso_time::iso_time;
use crate::usage::{Usage, dollars};

pub const FILE: &str = "usage.json";
const DAYS: usize = 31;
/// 31 days are far below this.
const MAX_FILE: u64 = 64 * 1024;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Day {
    day: String,
    usage: Usage,
}

pub struct DailyUsage {
    dir: PathBuf,
    /// Oldest first.
    days: Vec<Day>,
}

/// The UTC date of a Unix time, such as `2026-09-29`.
pub fn day_of(now: u32) -> String {
    iso_time(u128::from(now) * 1000)[..10].to_owned()
}

/// The error of a message that the cap stops, with the next step.
pub fn cap_text(cap: f64) -> String {
    format!(
        "Not started: today's agent cost reached your {} limit. It resets at 00:00 UTC, \
         or raise daily_cost_cap_usd in config.toml on your desktop.",
        dollars(cap)
    )
}

impl DailyUsage {
    /// A damaged file gives a new total and a line for the log: the total only informs,
    /// so a lost total never runs a message twice.
    pub fn load(dir: &Path) -> (DailyUsage, Option<String>) {
        let (days, problem) = match read_days(dir) {
            Ok(days) => (days, None),
            Err(e) => (
                Vec::new(),
                Some(format!(
                    "{FILE} is damaged, so the totals start again: {e:#}"
                )),
            ),
        };
        let usage = DailyUsage {
            dir: dir.to_owned(),
            days,
        };
        (usage, problem)
    }

    /// Adds one report to its day, and writes the file.
    pub fn add(&mut self, now: u32, usage: Usage) -> Result<()> {
        let day = day_of(now);
        match self.days.iter_mut().find(|d| d.day == day) {
            Some(known) => known.usage = known.usage.plus(usage),
            None => self.days.push(Day { day, usage }),
        }
        let extra = self.days.len().saturating_sub(DAYS);
        self.days.drain(..extra);
        write_private(&self.dir, FILE, &serde_json::to_string(&self.days)?)
    }

    pub fn today(&self, now: u32) -> Option<Usage> {
        let day = day_of(now);
        self.days.iter().find(|d| d.day == day).map(|d| d.usage)
    }

    /// True when the cost of today reached `cap`.
    pub fn cap_reached(&self, now: u32, cap: f64) -> bool {
        let cost = self.today(now).and_then(|u| u.cost_usd).unwrap_or(0.0);
        cost >= cap
    }
}

/// No file yet is an empty list.
fn read_days(dir: &Path) -> Result<Vec<Day>> {
    let path = dir.join(FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let Some(bytes) = read_at_most(&path, MAX_FILE)? else {
        bail!("{FILE} is bigger than {MAX_FILE} bytes");
    };
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-29 12:00 UTC.
    const NOON: u32 = 1_790_683_200;
    const DAY: u32 = 86_400;

    fn cost(usd: f64) -> Usage {
        Usage {
            input: 100,
            cached: 0,
            output: 10,
            cost_usd: Some(usd),
        }
    }

    #[test]
    fn a_day_is_the_utc_date() {
        assert_eq!(day_of(NOON), "2026-09-29");
        assert_eq!(day_of(NOON + 12 * 3600 - 1), "2026-09-29");
        assert_eq!(day_of(NOON + 12 * 3600), "2026-09-30");
    }

    #[test]
    fn reports_of_one_day_add_up_and_a_new_day_starts_at_zero() {
        let dir = tempfile::tempdir().unwrap();
        let (mut usage, _) = DailyUsage::load(dir.path());

        usage.add(NOON, cost(0.5)).unwrap();
        usage.add(NOON + 60, cost(0.25)).unwrap();

        let today = usage.today(NOON).unwrap();
        assert_eq!((today.input, today.output), (200, 20));
        assert_eq!(today.cost_usd, Some(0.75));
        assert_eq!(usage.today(NOON + DAY), None);
    }

    #[test]
    fn the_totals_survive_a_restart_in_a_private_file() {
        let dir = tempfile::tempdir().unwrap();
        let (mut usage, _) = DailyUsage::load(dir.path());
        usage.add(NOON, cost(0.5)).unwrap();

        let (again, problem) = DailyUsage::load(dir.path());

        assert_eq!(problem, None);
        assert_eq!(again.today(NOON), Some(cost(0.5)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn the_file_keeps_the_last_31_days() {
        let dir = tempfile::tempdir().unwrap();
        let (mut usage, _) = DailyUsage::load(dir.path());

        for n in 0..40 {
            usage.add(NOON + n * DAY, cost(1.0)).unwrap();
        }

        assert_eq!(usage.days.len(), 31);
        assert_eq!(usage.today(NOON), None);
        assert!(usage.today(NOON + 39 * DAY).is_some());
    }

    #[test]
    fn a_damaged_file_starts_the_totals_again_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{ not json").unwrap();

        let (usage, problem) = DailyUsage::load(dir.path());

        assert!(problem.unwrap().contains("usage.json is damaged"));
        assert_eq!(usage.today(NOON), None);
    }

    #[test]
    fn the_cap_is_reached_when_the_cost_of_today_meets_it() {
        let dir = tempfile::tempdir().unwrap();
        let (mut usage, _) = DailyUsage::load(dir.path());
        assert!(!usage.cap_reached(NOON, 1.0));

        usage.add(NOON, cost(0.99)).unwrap();
        assert!(!usage.cap_reached(NOON, 1.0));
        usage.add(NOON, cost(0.01)).unwrap();

        assert!(usage.cap_reached(NOON, 1.0));
        assert!(!usage.cap_reached(NOON + DAY, 1.0));
    }

    #[test]
    fn tokens_with_no_cost_never_reach_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let (mut usage, _) = DailyUsage::load(dir.path());
        let codex = Usage {
            cost_usd: None,
            ..cost(0.0)
        };

        usage.add(NOON, codex).unwrap();

        assert!(!usage.cap_reached(NOON, 0.01));
    }

    #[test]
    fn the_cap_text_says_what_happened_and_what_to_do() {
        assert_eq!(
            cap_text(5.0),
            "Not started: today's agent cost reached your $5.00 limit. It resets at 00:00 UTC, \
             or raise daily_cost_cap_usd in config.toml on your desktop."
        );
    }
}
