//! The wire format of Gnomish Relay. See `SPEC.md` section 7.
//!
//! This crate is pure: no I/O, no allocation tricks, no dependencies.
//! We prove it correct with Aeneas, so it stays inside the Rust subset that
//! Aeneas supports. `CLAUDE.md` lists the rules.

// Aeneas has no model for `From` between integers, for ranges, for `?`, or for
// `is_empty`. So we widen with `as`, compare with `<=`, return errors with `match`
// or `let else`, and compare `len()` with zero.
// Bool-to-integer casts make proofs hard, so we write the `if` out.
#![allow(
    clippy::cast_lossless,
    clippy::manual_range_contains,
    clippy::question_mark,
    clippy::bool_to_int_with_if,
    clippy::len_zero
)]

pub mod ascii;
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
