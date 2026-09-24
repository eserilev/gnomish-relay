//! The Gnomish Relay bridge. See `SPEC.md` section 8.

// The library exists for the binary and its tests, not for other crates.
#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]

pub mod agent;
pub mod flags;
pub mod fs_safe;
pub mod receive;
pub mod relay;
pub mod run;
pub mod screenshots;
pub mod slots;
pub mod strip;
