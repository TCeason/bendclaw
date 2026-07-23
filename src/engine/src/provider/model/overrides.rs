use super::capabilities::InputModality;
use super::capabilities::ModelCapabilities;

/// Explicit user/config overrides, applied after catalog and route resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModelOverrides {
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub supports_image: Option<bool>,
    pub reasoning: Option<bool>,
}

impl ModelOverrides {
    pub(super) fn apply(self, capabilities: &mut ModelCapabilities) {
        if let Some(context_window) = self.context_window {
            capabilities.context_window = context_window;
        }
        if let Some(max_output_tokens) = self.max_output_tokens {
            capabilities.max_output_tokens = max_output_tokens;
        }
        if let Some(supports_image) = self.supports_image {
            capabilities.input = if supports_image {
                vec![InputModality::Text, InputModality::Image]
            } else {
                vec![InputModality::Text]
            };
        }
        if let Some(reasoning) = self.reasoning {
            capabilities.reasoning.set_supported(reasoning);
        }
    }
}
