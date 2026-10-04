//! Where this program came from (SPEC.md 11.3). The release job sets
//! `GNOMISH_RELEASE_BUILD` when it compiles. A build from source never updates or
//! restarts itself: an update writes a release over it, and a restart points the login
//! service at it, in a folder that `cargo clean` empties.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildKind {
    Release,
    Source,
}

impl BuildKind {
    pub const THIS: BuildKind = if option_env!("GNOMISH_RELEASE_BUILD").is_some() {
        BuildKind::Release
    } else {
        BuildKind::Source
    };

    /// Auto-update, and the restarts for a new game (7.9) and new lore (11.4).
    pub fn manages_itself(self) -> bool {
        self == BuildKind::Release
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_release_build_updates_and_restarts_itself() {
        assert!(BuildKind::Release.manages_itself());
        assert!(!BuildKind::Source.manages_itself());
    }

    #[test]
    fn the_release_job_marks_its_builds() {
        let release = include_str!("../../../.github/workflows/release.yml");

        assert!(
            release.contains("GNOMISH_RELEASE_BUILD: \"1\""),
            "{release}"
        );
    }

    #[test]
    fn a_test_build_is_a_build_from_source() {
        assert_eq!(BuildKind::THIS, BuildKind::Source);
    }
}
