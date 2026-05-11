//! Filter trait and shared input types.
//!
//! Filters are pure functions: `(input, stream) -> Option<String>`.
//! `None` means "this filter declined to process"; the router falls
//! back to `TailFilter` or the caller passes through.

/// Command metadata shared across command filters.
#[derive(Debug, Clone, Copy)]
pub struct CmdCtx<'a> {
    pub head: &'a str,
    pub full: &'a str,
}

impl<'a> CmdCtx<'a> {
    pub fn new(full: &'a str) -> Self {
        let head = full
            .split_whitespace()
            .next()
            .map(|s| {
                // Strip leading env assignments like `FOO=bar cmd` — unusual but possible.
                if s.contains('=') {
                    ""
                } else {
                    s
                }
            })
            .unwrap_or("");
        Self { head, full }
    }

    /// Return the first non-flag sub-word after the head, e.g.
    /// `"git diff --stat"` → `Some("diff")`, `"cargo -v test"` → `Some("test")`.
    pub fn subcmd(&self) -> Option<&'a str> {
        self.full
            .split_whitespace()
            .skip(1)
            .find(|w| !w.starts_with('-'))
    }
}

/// Which stream a piece of output came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// A command filter transforms a single stream's text.
///
/// Filters are stateless; they may read config-style constants from
/// their own module but must not touch the filesystem or environment.
pub trait CmdFilter: Send + Sync {
    fn id(&self) -> &'static str;

    fn apply(&self, ctx: &CmdCtx<'_>, stream: Stream, text: &str) -> Option<String>;
}
