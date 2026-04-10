use bendclaw::cli::format::mask_value;
use bendclaw::cli::repl::render::truncate_head_tail;

#[test]
fn short_string_unchanged() {
    assert_eq!(truncate_head_tail("hello world", 56), "hello world");
}

#[test]
fn exact_boundary_unchanged() {
    let s = "a".repeat(56);
    assert_eq!(truncate_head_tail(&s, 56), s);
}

#[test]
fn long_string_shows_head_and_tail() {
    let s = "implement the new session title truncation with head tail display mode";
    let result = truncate_head_tail(s, 40);
    assert!(
        result.contains(" ... "),
        "should contain separator: {result}"
    );
    assert!(
        result.starts_with("implement"),
        "should keep head: {result}"
    );
    assert!(
        result.ends_with("display mode"),
        "should keep tail: {result}"
    );
    assert!(
        result.chars().count() <= 40,
        "should respect max: {} chars in {result}",
        result.chars().count()
    );
}

#[test]
fn unicode_chars_handled() {
    let s = "会话标题截断测试：这是一个很长的中文标题用来验证Unicode字符的正确处理能力";
    let result = truncate_head_tail(s, 24);
    assert!(
        result.contains(" ... "),
        "should contain separator: {result}"
    );
    assert!(
        result.chars().count() <= 24,
        "should respect max: {} chars in {result}",
        result.chars().count()
    );
}

#[test]
fn very_small_max_falls_back_to_plain_truncate() {
    let s = "a]short but still needs truncation";
    let result = truncate_head_tail(s, 10);
    // max < sep_len + 6 = 11, so falls back to plain truncate
    assert!(result.ends_with("..."), "should fall back: {result}");
    assert!(
        !result.contains(" ... "),
        "should not use head-tail: {result}"
    );
}

// ---------------------------------------------------------------------------
// mask_value
// ---------------------------------------------------------------------------

#[test]
fn mask_value_long_string() {
    // "secret-token-123" → "se************23"
    let result = mask_value("secret-token-123");
    assert!(result.starts_with("se"), "should keep first 2: {result}");
    assert!(result.ends_with("23"), "should keep last 2: {result}");
    assert_eq!(result.len(), 16); // same char count as input
    assert!(result.contains('*'), "should contain mask chars: {result}");
}

#[test]
fn mask_value_short_fully_masked() {
    assert_eq!(mask_value("abc"), "***");
    assert_eq!(mask_value("ab"), "**");
    assert_eq!(mask_value("a"), "*");
    assert_eq!(mask_value(""), "");
}

#[test]
fn mask_value_boundary_five_chars() {
    // 5 chars → fully masked
    assert_eq!(mask_value("12345"), "*****");
}

#[test]
fn mask_value_six_chars_shows_edges() {
    let result = mask_value("abcdef");
    assert_eq!(result, "ab**ef");
}

#[test]
fn mask_value_unicode() {
    let result = mask_value("密码是很长的秘密值");
    assert!(result.starts_with("密码"), "should keep first 2: {result}");
    assert!(result.ends_with("密值"), "should keep last 2: {result}");
    let star_count = result.chars().filter(|c| *c == '*').count();
    assert_eq!(star_count, 5, "middle should be masked: {result}");
}
