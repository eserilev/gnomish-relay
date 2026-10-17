//! The roots of the config that don't exist at start. The bridge skips them, so an
//! unplugged drive stops no start (SPEC.md 12).

use std::path::Path;

use crate::config::with_tilde;

/// The line of the log at start and of `status`.
pub fn missing_root_line(root: &Path, home: &Path) -> String {
    format!(
        "Can't find {}, so agents can't work there for now. Connect its drive, or run \
         gnomish-relay setup to remove it.",
        with_tilde(root, home)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_root_in_the_home_folder_is_named_with_a_tilde() {
        let line = missing_root_line(Path::new("/home/x/Drive"), Path::new("/home/x"));

        assert!(line.starts_with("Can't find ~/Drive, "), "{line}");
    }
}
