//! A shared, pipelined PostgreSQL connection for karez, using only `std`.
//!
//! See `docs/design/karez-rt.md`. The [`engine`] module is the sans-io session; the
//! shared connection drives it from threads.

#![forbid(unsafe_code)]

pub mod engine;
mod error;

pub use error::{Error, Result};
