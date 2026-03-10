use std::sync::LazyLock;

use tiktoken_rs::o200k_base;
use tiktoken_rs::CoreBPE;

static BPE: LazyLock<CoreBPE> =
    LazyLock::new(|| o200k_base().expect("failed to load o200k_base tokenizer"));

pub fn count_tokens(text: &str) -> usize {
    BPE.encode_with_special_tokens(text).len()
}
