//! Favorite sessions — persisted set of session ids the user pinned in the
//! dashboard. Stored as a small standalone document (like `variables.json`) so
//! toggling a favorite never rewrites any `session.json`.

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FavoritesDocument {
    pub version: u32,
    pub ids: Vec<String>,
}
