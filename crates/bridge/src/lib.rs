//! The Gnomish Relay bridge. See `SPEC.md` section 8.

// The library exists for the binary and its tests, not for other crates.
#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]

pub mod acp;
pub mod action_input;
pub mod activity;
pub mod agent;
pub mod allow;
pub mod claude;
pub mod claude_sessions;
pub mod codex;
pub mod config;
pub mod flags;
pub mod fs_safe;
pub mod history;
pub mod install;
pub mod lock;
pub mod process;
pub mod program;
pub mod receive;
pub mod relay;
pub mod reply;
pub mod run;
pub mod saved;
pub mod screenshots;
pub mod slots;
pub mod state;
pub mod strip;
pub mod turn;
pub mod update;
