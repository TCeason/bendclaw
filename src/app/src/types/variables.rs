//! Variable domain model — records, scopes, and persistence document.

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VariableScope {
    Global,
    Project,
    Session,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableRecord {
    pub key: String,
    pub value: String,
    pub scope: VariableScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariablesDocument {
    pub version: u32,
    pub variables: Vec<VariableRecord>,
}
