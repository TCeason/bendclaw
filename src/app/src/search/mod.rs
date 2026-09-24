mod matcher;
mod session_search;
mod session_task;

pub use matcher::TextMatcher;
pub use session_search::SearchHit;
pub use session_search::SessionSearcher;
pub use session_search::SessionWithText;
pub use session_task::SessionSearch;
pub use session_task::DEFAULT_WINDOW_DAYS;
