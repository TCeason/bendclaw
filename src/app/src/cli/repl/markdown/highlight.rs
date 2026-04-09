//! Syntax highlighting for code blocks using syntect.
//!
//! The heavy `SyntaxSet` / `ThemeSet` are loaded once via `OnceLock` and shared
//! for the lifetime of the process, avoiding repeated ~30ms loads on every
//! `MarkdownStream` creation.

use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;

use super::theme::RESET;

pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

static GLOBAL_HIGHLIGHTER: OnceLock<Highlighter> = OnceLock::new();

impl Highlighter {
    fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    /// Return a shared global instance (initialized on first call).
    pub fn global() -> &'static Highlighter {
        GLOBAL_HIGHLIGHTER.get_or_init(Highlighter::new)
    }

    /// Highlight a single line of code, returning ANSI-escaped string.
    pub fn highlight_line(&self, line: &str, language: Option<&str>) -> String {
        let syntax = language
            .and_then(|lang| self.syntax_set.find_syntax_by_token(lang))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let theme = &self.theme_set.themes["base16-ocean.dark"];
        let mut hl = HighlightLines::new(syntax, theme);

        match hl.highlight_line(line, &self.syntax_set) {
            Ok(ranges) => format!("{}{}", as_24_bit_terminal_escaped(&ranges, false), RESET),
            Err(_) => line.to_string(),
        }
    }
}
