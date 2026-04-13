use std::borrow::Cow;
use std::fs;
use std::sync::Arc;
use std::sync::RwLock;

use reedline::default_emacs_keybindings;
use reedline::ColumnarMenu;
use reedline::EditCommand;
use reedline::Emacs;
use reedline::FileBackedHistory;
use reedline::KeyCode;
use reedline::KeyModifiers;
use reedline::MenuBuilder;
use reedline::Prompt;
use reedline::PromptEditMode;
use reedline::PromptHistorySearch;
use reedline::PromptHistorySearchStatus;
use reedline::Reedline;
use reedline::ReedlineEvent;
use reedline::ReedlineMenu;
use reedline::Signal;
use reedline::ValidationResult;

use super::completer::CompletionState;
use super::completer::ReplCompleter;
use super::render::DIM;
use super::render::RESET;
use crate::conf::paths;
use crate::error::Result;

// ---------------------------------------------------------------------------
// ReadLineOutput
// ---------------------------------------------------------------------------

pub enum ReadLineOutput {
    /// User submitted input (may contain embedded newlines via Shift+Enter).
    Line(String),
    /// Ctrl+C
    Interrupted,
    /// Ctrl+D
    Eof,
}

// ---------------------------------------------------------------------------
// ReplEditor
// ---------------------------------------------------------------------------

pub type CompletionStateRef = Arc<RwLock<CompletionState>>;

pub struct ReplEditor {
    engine: Reedline,
    prompt: ReplPrompt,
}

impl ReplEditor {
    pub fn new(state: CompletionStateRef) -> Result<Self> {
        let completer = Box::new(ReplCompleter::new(state));
        let completion_menu = Box::new(ColumnarMenu::default().with_name("completion_menu"));

        let mut keybindings = default_emacs_keybindings();

        // Shift+Enter → insert newline (multiline editing)
        keybindings.add_binding(
            KeyModifiers::SHIFT,
            KeyCode::Enter,
            ReedlineEvent::Edit(vec![EditCommand::InsertNewline]),
        );

        // Tab → trigger completion menu
        keybindings.add_binding(
            KeyModifiers::NONE,
            KeyCode::Tab,
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::Menu("completion_menu".to_string()),
                ReedlineEvent::MenuNext,
            ]),
        );

        let edit_mode = Box::new(Emacs::new(keybindings));

        let validator = Box::new(ReplValidator);

        let mut engine = Reedline::create()
            .with_completer(completer)
            .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
            .with_edit_mode(edit_mode)
            .with_validator(validator);

        // Load history
        if let Ok(path) = paths::history_file_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(history) = FileBackedHistory::with_file(1000, path) {
                engine = engine.with_history(Box::new(history));
            }
        }

        Ok(Self {
            engine,
            prompt: ReplPrompt::default(),
        })
    }

    pub fn read_line(&mut self) -> ReadLineOutput {
        match self.engine.read_line(&self.prompt) {
            Ok(Signal::Success(buffer)) => ReadLineOutput::Line(buffer),
            Ok(Signal::CtrlC) => ReadLineOutput::Interrupted,
            Ok(Signal::CtrlD) | Ok(_) => ReadLineOutput::Eof,
            Err(_) => ReadLineOutput::Eof,
        }
    }

    pub fn set_prompt(&mut self, left: String) {
        self.prompt.left = left;
    }

    /// Sync history to disk.
    pub fn sync_history(&mut self) {
        let _ = self.engine.sync_history();
    }
}

// ---------------------------------------------------------------------------
// ReplPrompt
// ---------------------------------------------------------------------------

struct ReplPrompt {
    left: String,
}

impl Default for ReplPrompt {
    fn default() -> Self {
        Self {
            left: "> ".to_string(),
        }
    }
}

impl Prompt for ReplPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.left)
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{DIM}  ...{RESET} "))
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!(
            "({}reverse-search: {}) ",
            prefix, history_search.term
        ))
    }
}

// ---------------------------------------------------------------------------
// ReplValidator — keeps `\` continuation and ``` fenced blocks working
// ---------------------------------------------------------------------------

struct ReplValidator;

impl reedline::Validator for ReplValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        let trimmed = line.trim();
        // Backslash continuation: line ends with `\`
        if trimmed.ends_with('\\') {
            return ValidationResult::Incomplete;
        }
        // Fenced code block: odd number of ``` fences means unclosed
        let fence_count = trimmed.lines().filter(|l| l.trim() == "```").count();
        if trimmed.starts_with("```") && fence_count % 2 != 0 {
            return ValidationResult::Incomplete;
        }
        ValidationResult::Complete
    }
}
