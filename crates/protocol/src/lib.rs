//! The wire format of Gnomish Relay. See `SPEC.md` section 7.
//!
//! This crate is pure: no I/O, no allocation tricks, no dependencies.
//! We prove it correct with Aeneas, so it stays inside the Rust subset that
//! Aeneas supports. `CLAUDE.md` lists the rules.

// TODO: remove when every stub in VERIFICATION.md has a body.
#![allow(
    unused_variables,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::needless_pass_by_value
)]

pub mod cell;
pub mod folder;
pub mod frame;
pub mod lua;
pub mod policy;
pub mod popup;
pub mod rate;
pub mod record;
pub mod seen;
pub mod slot;
pub mod wow_text;
