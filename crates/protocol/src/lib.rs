//! The wire format of Gnomish Relay. See `SPEC.md` section 7.
//!
//! This crate is pure: no I/O, no allocation tricks, no dependencies.
//! We prove it correct with Aeneas, so it stays inside the Rust subset that
//! Aeneas supports. `CLAUDE.md` lists the rules.

// Aeneas has no model for `From` between integers, for ranges, for `?`, or for
// `is_empty`, or for `vec!`. So we widen with `as`, compare with `<=`, return errors with `match`
// or `let else`, compare `len()` with zero, and push into a new `Vec`.
// Bool-to-integer casts make proofs hard, so we write the `if` out. Aeneas has no model for
// `Option::as_ref`, `Option::map`, `is_some`, or `matches!`, so an option is a plain `match`.
#![allow(
    clippy::ref_option,
    clippy::manual_map,
    clippy::redundant_pattern_matching,
    clippy::match_like_matches_macro,
    clippy::cast_lossless,
    clippy::manual_range_contains,
    clippy::question_mark,
    clippy::bool_to_int_with_if,
    clippy::len_zero,
    clippy::vec_init_then_push
)]

pub mod action;
pub mod always;
pub mod apps;
pub mod ascii;
pub mod cell;
pub(crate) mod command_rules;
pub mod connect;
pub mod folder;
pub mod frame;
pub mod hosts;
pub mod inline;
pub mod ip;
pub mod live;
pub mod lua;
pub mod markdown;
pub mod notice;
pub(crate) mod path_rules;
pub mod policy;
pub mod popup;
pub mod rate;
pub mod record;
pub mod restore;
pub mod sandbox;
pub mod sbpl;
pub(crate) mod search;
pub mod seen;
pub mod sessions;
pub mod shell;
pub mod slot;
pub mod version;
pub mod wow_text;
