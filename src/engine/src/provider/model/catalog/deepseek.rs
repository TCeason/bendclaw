use super::profile::ModelProfile;
use super::profile::ReasoningProfile;
use super::profile::BASE;
use super::profile::NO_REASONING;
use crate::ThinkingLevel;

const REASONER_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, None),
    (ThinkingLevel::High, Some("high")),
];
const REASONER: ReasoningProfile = ReasoningProfile {
    levels: REASONER_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: None,
};

// DeepSeek V4 accepts these requested effort values and maps them internally
// per model. Thinking is enabled by default at high effort.
const V4_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, None),
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
    (ThinkingLevel::Max, Some("max")),
];
const V4_REASONING: ReasoningProfile = ReasoningProfile {
    levels: V4_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: Some(super::super::capabilities::AnthropicThinkingWire::Enabled),
};

// The documented 1M total context leaves 616K input at the maximum 384K output.
// The advertised 1M window is what users recognize; 616K is the real input
// budget used for compaction/overflow math.
const V4: ModelProfile = ModelProfile {
    max_input_tokens: 616_000,
    advertised_context_window: Some(1_000_000),
    max_output_tokens: 384_000,
    vision: false,
    reasoning: V4_REASONING,
    ..BASE
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("deepseek-chat",     ModelProfile { max_input_tokens: 128_000, max_output_tokens: 8_192, vision: false, reasoning: NO_REASONING, ..BASE }),
    ("deepseek-reasoner", ModelProfile { max_input_tokens: 128_000, max_output_tokens: 64_000, vision: false, reasoning: REASONER, ..BASE }),
    ("deepseek-v4-flash", V4),
    ("deepseek-v4-pro",   V4),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}
