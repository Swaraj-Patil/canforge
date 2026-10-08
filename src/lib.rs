//! canforge: a toolchain for CAN databases (DBC files).
//!
//! Parse a DBC file, lint it, render its bit layouts, decode raw frames,
//! detect breaking changes between two revisions, and generate embedded C
//! or Python decoders. The crate has no dependencies and builds both as a
//! native command-line tool and as a WebAssembly module for the browser.
//!
//! The behaviour is specified by an independent Python reference model in
//! `reference/`. The integration tests in `tests/golden.rs` require this
//! crate to reproduce that model's output exactly.

pub mod bits;
pub mod codegen_c;
pub mod codegen_py;
pub mod decode;
pub mod diff;
pub mod json;
pub mod layout;
pub mod lexer;
pub mod lint;
pub mod model;
pub mod names;
pub mod numfmt;
pub mod parser;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use model::{Database, DbcError, Message, Mux, Signal, ValueType};
pub use parser::parse;

/// The crate version, embedded in generated code. Comes from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where generated files point readers for documentation.
pub const PROJECT_URL: &str = "https://github.com/Swaraj-Patil/canforge";
