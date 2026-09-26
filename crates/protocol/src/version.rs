//! The addon versions that this bridge speaks, for each app (SPEC.md 7.7 and 9.7,
//! decision 17). The hello of each app carries its version in `ver=`.

use crate::apps::App;

const RELAY_OLDEST: u32 = 1;
const RELAY_NEWEST: u32 = 1;
const TIMEWAYS_OLDEST: u32 = 1;
const TIMEWAYS_NEWEST: u32 = 1;

/// Where an addon version falls in the range of its app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionFit {
    Supported,
    /// The addon is older than the bridge speaks.
    TooOld,
    /// The addon is newer than the bridge speaks.
    TooNew,
}

#[must_use]
pub fn oldest(app: App) -> u32 {
    match app {
        App::Relay => RELAY_OLDEST,
        App::Timeways => TIMEWAYS_OLDEST,
    }
}

#[must_use]
pub fn newest(app: App) -> u32 {
    match app {
        App::Relay => RELAY_NEWEST,
        App::Timeways => TIMEWAYS_NEWEST,
    }
}

/// The parameter is not `version`: in Lean that name is this module (CLAUDE.md).
#[must_use]
pub fn version_fit(app: App, reported: u32) -> VersionFit {
    if reported < oldest(app) {
        return VersionFit::TooOld;
    }
    if reported > newest(app) {
        return VersionFit::TooNew;
    }
    VersionFit::Supported
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPS: [App; 2] = [App::Relay, App::Timeways];

    #[test]
    fn every_range_holds_at_least_one_version() {
        for app in APPS {
            assert!(oldest(app) <= newest(app), "{app:?}");
        }
    }

    #[test]
    fn the_oldest_and_the_newest_version_are_supported() {
        for app in APPS {
            assert_eq!(version_fit(app, oldest(app)), VersionFit::Supported);
            assert_eq!(version_fit(app, newest(app)), VersionFit::Supported);
        }
    }

    #[test]
    fn one_below_the_oldest_version_is_too_old() {
        for app in APPS {
            assert_eq!(version_fit(app, oldest(app) - 1), VersionFit::TooOld);
        }
    }

    #[test]
    fn one_above_the_newest_version_is_too_new() {
        for app in APPS {
            assert_eq!(version_fit(app, newest(app) + 1), VersionFit::TooNew);
        }
    }

    #[test]
    fn zero_is_too_old_and_the_largest_version_is_too_new() {
        for app in APPS {
            assert_eq!(version_fit(app, 0), VersionFit::TooOld);
            assert_eq!(version_fit(app, u32::MAX), VersionFit::TooNew);
        }
    }

    /// Every version near both ends of `u32` falls where the range says.
    #[test]
    fn each_version_near_the_ends_fits_as_its_range_says() {
        let low = 0..=10_000;
        let high = u32::MAX - 10_000..=u32::MAX;
        for app in APPS {
            for version in low.clone().chain(high.clone()) {
                let expected = if version < oldest(app) {
                    VersionFit::TooOld
                } else if version > newest(app) {
                    VersionFit::TooNew
                } else {
                    VersionFit::Supported
                };
                assert_eq!(version_fit(app, version), expected, "{app:?} {version}");
            }
        }
    }
}
