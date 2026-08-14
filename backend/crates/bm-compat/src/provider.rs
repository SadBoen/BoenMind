//! `provider` module shim for bm-compat (B1).
//!
//! Hosts only the `InputType` enum required by the extracted
//! `provider_metadata` shim. Extracted verbatim from
//! `legacy/pi_agent_rust/src/provider.rs:194-202`.

use serde::{Deserialize, Serialize};

// extracted from legacy/pi_agent_rust/src/provider.rs:194-202
/// Input types supported by a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputType {
    Text,
    Image,
}
