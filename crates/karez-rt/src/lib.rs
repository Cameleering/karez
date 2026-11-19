//! A shared, pipelined PostgreSQL connection for karez, using only `std`.
//!
//! See `docs/design/karez-rt.md`. The [`engine`] module is the sans-io session; the
//! [`SharedConnection`] drives it from threads or futures, with no async runtime.

#![forbid(unsafe_code)]

pub mod engine;
mod error;
mod shared;

pub use error::{Error, Result};
pub use shared::{NextChunk, Options, OwnedValue, Pending, Pipeline, SharedConnection, Stream};
