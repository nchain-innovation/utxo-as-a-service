//! Library face of the UTXO-as-a-Service indexer.
//!
//! The crate is split into a library and a thin binary so that integration
//! tests under `tests/` and fuzz targets can link against it. A binary-only
//! crate exposes nothing to link, which previously forced every test to be an
//! in-module `#[cfg(test)] mod tests` and made `cargo fuzz` impossible.
//!
//! # What belongs in the public surface
//!
//! Nothing outside this repository consumes `uaas` as a library, so the
//! surface is kept narrow on purpose: a module is `pub` only when the binary
//! or a test genuinely needs it, and `pub(crate)` otherwise. Making a module
//! public makes its types public API, which brings obligations — clippy's
//! public-API lints among them — that an application should take on
//! deliberately rather than by default.
//!
//! When adding a module, start it `pub(crate)` and promote it only when
//! something outside the library actually needs it.
//!
//! Note that `src/main.rs` is a *separate crate* that links this one, so
//! anything it uses must be `pub`, not `pub(crate)`.

#[macro_use]
extern crate lazy_static;

// Used by the binary.
pub mod config;
pub mod migrate;
pub mod peer_event;
pub mod rate_limit;
pub mod rest_api;
pub mod thread_manager;
pub mod thread_tracker;
pub mod thread_util;
pub mod uaas;

// Internal to the library.
pub(crate) mod dynamic_config;
pub(crate) mod event_handler;
pub(crate) mod peer_connection;
pub(crate) mod peer_thread;
pub(crate) mod services;
