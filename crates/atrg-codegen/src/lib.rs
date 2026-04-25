#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Lexicon-driven Rust code generation for at-rust-go.
//!
//! Takes AT Protocol lexicon JSON files and generates:
//! - Strongly-typed `serde`-derived structs for records, objects, params, and outputs
//! - Axum handler stubs with correct input/output types
//! - A `xrpc_routes()` function wiring all generated handlers
//! - AT-URI helper functions
//!
//! **This crate ships zero lexicon files.** It only provides the generator.

pub mod generator;
pub mod lexicon;

pub use generator::{generate, GenOptions, GenReport};
