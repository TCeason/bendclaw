//! Stats carried alongside slimmed tool output.

use serde::Deserialize;
use serde::Serialize;

/// Token-saving stats for a single tool invocation.
///
/// `filter` identifiers used by the engine:
/// - `"off"`        — slim disabled via env or session toggle
/// - `"raw_error"`  — command failed; pass through untouched
/// - `"none"`       — no filter matched / no change
/// - `"tail"`       — generic head+tail truncation
/// - `"ack"`        — ack-style commands collapsed to a single line
/// - `"git_diff"` / `"git_log"` / `"git_status"` — git sub-filters
/// - `"json"`      — raw JSON compacted by depth, long strings, arrays, and key count
/// - `"cache_hit"` — returned from session cache (later phases)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlimStats {
    pub filter: String,
    pub original: usize,
    pub slimmed: usize,
}

impl SlimStats {
    pub fn passthrough(filter: &'static str, bytes: usize) -> Self {
        Self {
            filter: filter.to_string(),
            original: bytes,
            slimmed: bytes,
        }
    }

    pub fn new(filter: &'static str, original: usize, slimmed: usize) -> Self {
        Self {
            filter: filter.to_string(),
            original,
            slimmed,
        }
    }

    pub fn saved_bytes(&self) -> usize {
        self.original.saturating_sub(self.slimmed)
    }

    pub fn ratio(&self) -> f32 {
        if self.original == 0 {
            0.0
        } else {
            self.saved_bytes() as f32 / self.original as f32
        }
    }
}
