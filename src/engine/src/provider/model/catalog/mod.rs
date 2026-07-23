mod anthropic;
mod kimi;
mod openai;
mod profile;
mod xai;

use std::collections::HashMap;

use super::capabilities::InputModality;
use super::capabilities::ModelCapabilities;
use super::capabilities::ReasoningCapabilities;

pub(super) fn resolve(model_id: &str) -> Option<ModelCapabilities> {
    let id = normalize_model_id(model_id);
    if id.is_empty() {
        return None;
    }

    openai::resolve(&id)
        .or_else(|| anthropic::resolve(&id))
        .or_else(|| kimi::resolve(&id))
        .or_else(|| xai::resolve(&id))
        .or_else(|| openai::fallback(&id))
        .or_else(|| anthropic::fallback(&id))
        .map(profile::ModelProfile::capabilities)
}

pub(super) fn normalize_model_id(model_id: &str) -> String {
    let normalized = model_id.trim().to_ascii_lowercase();
    for prefix in ["openai/", "xai/", "x-ai/", "anthropic/"] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    normalized
}

pub(super) fn kimi_k3_thinking_level_map() -> HashMap<crate::ThinkingLevel, Option<String>> {
    kimi::k3_thinking_level_map()
}

pub(super) fn protocol_fallback(vision: bool) -> ModelCapabilities {
    ModelCapabilities {
        context_window: if vision { 200_000 } else { 128_000 },
        max_output_tokens: if vision { 8_192 } else { 32_768 },
        input: if vision {
            vec![InputModality::Text, InputModality::Image]
        } else {
            vec![InputModality::Text]
        },
        reasoning: ReasoningCapabilities::new(true, HashMap::new(), false),
        default_verbosity: None,
        remote_compaction: false,
    }
}
