//! The Gnomish Relay bridge. See `SPEC.md` section 8.

// The library exists for the binary and its tests, not for other crates.
#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]

pub mod acp;
pub mod action_input;
pub mod activity;
pub mod addon_lines;
pub mod agent;
pub mod allow;
pub mod app_files;
pub mod app_protocol;
pub mod claude;
pub mod claude_sessions;
pub mod codex;
pub mod config;
pub mod config_text;
pub mod desktop;
pub mod flags;
pub mod fs_safe;
pub mod gate;
pub mod history;
pub mod install;
pub mod lane;
pub mod lock;
pub mod model;
pub mod model_claude;
pub mod model_local;
pub mod model_setup;
pub mod process;
pub mod program;
pub mod receive;
pub mod relay;
pub mod reply;
pub mod run;
pub mod saved;
pub mod screenshots;
pub mod setup;
pub mod slots;
pub mod state;
pub mod story;
pub mod story_sandbox;
pub mod strip;
pub mod timeways;
pub mod turn;
pub mod update;
pub mod versions;
