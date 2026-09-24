//! The wire format of Gnomish Relay. See `SPEC.md` section 7.
//!
//! This crate is pure: no I/O, no allocation tricks, no dependencies.
//! We prove it correct with Aeneas, so it stays inside the Rust subset that
//! Aeneas supports. `CLAUDE.md` lists the rules.

pub mod cell;
