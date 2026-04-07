pub mod term_status;
pub mod text;

pub use ll::reporters::Level;
pub use term_status::TermStatus;
pub use text::StdioReporter;
pub use text::StringReporter;

use std::sync::Arc;

/// Initialize ll_stdio with sensible defaults:
/// - StdioReporter with `log_task_start = true`
/// - TermStatus live display (if TTY)
pub fn init() {
    Builder::default().init();
}

pub fn builder() -> Builder {
    Builder::default()
}

pub struct Builder {
    log_task_start: bool,
    max_log_level: Level,
    term_status: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            log_task_start: true,
            max_log_level: Level::default(),
            term_status: true,
        }
    }
}

impl Builder {
    pub fn log_task_start(mut self, val: bool) -> Self {
        self.log_task_start = val;
        self
    }

    pub fn max_log_level(mut self, level: Level) -> Self {
        self.max_log_level = level;
        self
    }

    pub fn term_status(mut self, val: bool) -> Self {
        self.term_status = val;
        self
    }

    pub fn init(self) {
        let mut reporter = StdioReporter::new();
        reporter.log_task_start = self.log_task_start;
        reporter.max_log_level = self.max_log_level;
        ll::add_reporter(Arc::new(reporter));

        if self.term_status {
            term_status::show();
        }
    }
}
