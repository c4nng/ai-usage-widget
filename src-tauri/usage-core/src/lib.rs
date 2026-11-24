//! Network-free building blocks shared by every provider: the report model the
//! UI renders, and parsers that turn raw provider responses into it.
//!
//! Everything here is pure so it can be unit-tested against recorded fixtures.
//! The subscription endpoints are undocumented and drift over time, so the
//! parsers are deliberately lenient about field names and shapes.

pub mod claude;
pub mod codex;
pub mod cost;
pub mod model;
pub mod schedule;
pub mod time;

pub use model::*;
