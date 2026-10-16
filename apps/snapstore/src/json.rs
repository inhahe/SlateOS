//! The JSON the store's manifests and records are written in.
//!
//! The reader and writer moved out to `apps/jsonvalue` on 2026-10-09, the
//! applications' one JSON crate, when the weather app needed one; they are
//! re-exported here unchanged, so `snapstore::json` reads and writes exactly
//! the manifests existing stores hold.

pub use jsonvalue::{JsonValue, json_parse, json_pretty};
