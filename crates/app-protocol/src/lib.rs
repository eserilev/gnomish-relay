//! The app protocol of SPEC.md 9.8, in a crate of its own so that an app can test against
//! the real checks of the bridge without the rest of the bridge.

// Each error enum names its cases, so an `# Errors` section only repeats them.
#![allow(clippy::missing_errors_doc)]

pub mod addon_lines;
pub mod model_answer;
pub mod story_lines;
