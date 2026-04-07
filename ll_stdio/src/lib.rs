pub mod term_status;
pub mod text;

pub use ll::reporters::Level;
pub use term_status::TermStatus;
pub use text::StdioReporter;
pub use text::StringReporter;

use ll::reporters::{EventQueue, TaskEvent};
use std::sync::Arc;
use std::time::Duration;

/// Spawn a background thread that continuously drains an event queue,
/// calling `handler` with each batch every 10ms. Useful for implementing
/// custom reporters without writing the thread boilerplate.
///
/// ```ignore
/// impl Reporter for MyReporter {
///     fn start(&self, queue: EventQueue) {
///         let this = self.clone();
///         ll_stdio::spawn_drain_thread(queue, move |events| {
///             for event in events { /* ... */ }
///         });
///     }
/// }
/// ```
pub fn spawn_drain_thread(queue: EventQueue, handler: impl Fn(Vec<TaskEvent>) + Send + 'static) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(10));
        let events = std::mem::take(&mut *queue.lock().unwrap());
        if !events.is_empty() {
            handler(events);
        }
    });
}

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
