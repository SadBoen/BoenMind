//! bm-compat — vendored QuickJS extension engine as a library (BoenMind
//! main line B, task B1 copy-in).
//!
//! Module layout mirrors `legacy/pi_agent_rust/src` so the vendored files keep
//! their upstream `crate::xxx::` paths untouched and stay diff-comparable with
//! the legacy source:
//!
//! - Byte-identical vendored copies (diff-verified against legacy):
//!   `extensions_js`, `scheduler`, `hostcall_queue`, `hostcall_io_uring_lane`,
//!   `embedded_assets`, `error`.
//! - Byte-identical whole-file shims (diff-verified against legacy):
//!   `http_shim`, `crypto_shim`, `buffer_shim`, `hostcall_s3_fifo`.
//! - Extracted shims (verbatim line ranges from legacy, see per-block
//!   "extracted from" headers): `extensions`, `tools`, `provider_metadata`,
//!   `provider`.
//!
//! `wasm-host` feature: extensions_js.rs gates its `pi_wasm` references behind
//! `#[cfg(feature = "wasm-host")]`. The `pi_wasm` module is not vendored yet
//! (needs wasmtime wiring — see README.md), so it only exists under that
//! feature which defaults off.

pub mod buffer_shim;
pub mod crypto_shim;
pub mod embedded_assets;
pub mod error;
pub mod extensions;
// B2 — host thread (drain → dispatch → complete → tick pump).
pub mod host;
pub mod extensions_js;
pub mod hostcall_io_uring_lane;
pub mod hostcall_queue;
pub mod hostcall_s3_fifo;
pub mod http_shim;
pub mod provider;
pub mod provider_metadata;
pub mod scheduler;
pub mod tools;

#[cfg(feature = "wasm-host")]
pub mod pi_wasm;

// B1 5-symbol re-export surface (see DEPENDENCIES.md §5).
pub use extensions::{ExtensionPolicy, ExtensionPolicyMode, PolicyProfile};
pub use extensions_js::{HostcallKind, PiJsRuntime};
