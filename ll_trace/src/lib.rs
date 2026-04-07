/*!
Chrome Trace Format reporter for [`ll`]. Writes trace JSON viewable in
`chrome://tracing` or [Perfetto UI](https://ui.perfetto.dev).

Each finished task becomes one complete (`X`) event with timestamp, duration,
and optional metadata. The output streams to any `Write` destination — file,
buffer, or pipe.

Quick setup:
```ignore
ll_trace::init("trace.json");  // writes trace.json, returns FlushGuard
```

Builder for more control:
```ignore
let _guard = ll_trace::builder()
    .file("trace.json")
    .process_name("my-server")
    .include_args(true)
    .build();
```

**Important:** Hold the returned [`FlushGuard`] until you want to finalize
the trace file. Dropping it writes the closing `]}` and flushes the writer.
*/

pub mod writer;

use ll::reporters::{EventQueue, Reporter};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::SystemTime;

// ── Public API ──────────────────────────────────────────────────

/// Initialize a trace reporter that writes to `path`. Returns a
/// [`FlushGuard`] — hold it until shutdown, then drop to finalize.
pub fn init(path: &str) -> FlushGuard {
    builder().file(path).build()
}

pub fn builder() -> Builder {
    Builder::default()
}

// ── Builder ─────────────────────────────────────────────────────

pub struct Builder {
    process_name: String,
    include_args: bool,
    include_tags: bool,
    writer: Option<Box<dyn Write + Send>>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            process_name: "ll".into(),
            include_args: true,
            include_tags: true,
            writer: None,
        }
    }
}

impl Builder {
    pub fn file(mut self, path: &str) -> Self {
        let f = File::create(path).expect("failed to create trace file");
        self.writer = Some(Box::new(BufWriter::new(f)));
        self
    }

    pub fn writer(mut self, w: impl Write + Send + 'static) -> Self {
        self.writer = Some(Box::new(w));
        self
    }

    pub fn process_name(mut self, name: &str) -> Self {
        self.process_name = name.into();
        self
    }

    pub fn include_args(mut self, val: bool) -> Self {
        self.include_args = val;
        self
    }

    pub fn include_tags(mut self, val: bool) -> Self {
        self.include_tags = val;
        self
    }

    /// Build the reporter and register it on the global [`ll::TaskTree`].
    pub fn build(self) -> FlushGuard {
        let (reporter, guard) = self.build_reporter();
        ll::add_reporter(reporter);
        guard
    }

    /// Build the reporter without registering it. Use this to add the
    /// reporter to a custom [`ll::TaskTree`] (e.g. in tests).
    pub fn build_reporter(self) -> (Arc<dyn Reporter>, FlushGuard) {
        let w = self.writer.expect("must set a file or writer");
        let shared = Arc::new(Mutex::new(WriterState::new(w, &self.process_name)));
        let reporter = TraceReporter {
            state: shared.clone(),
            include_args: self.include_args,
            include_tags: self.include_tags,
        };
        (Arc::new(reporter), FlushGuard { state: shared })
    }
}

// ── FlushGuard ──────────────────────────────────────────────────

/// Finalizes the trace file on drop. Hold this until you're done tracing.
pub struct FlushGuard {
    state: Arc<Mutex<WriterState>>,
}

impl FlushGuard {
    /// Manually flush and finalize the trace. Called automatically on drop.
    pub fn flush(&self) {
        let mut state = self.state.lock().unwrap();
        state.finalize();
    }
}

impl Drop for FlushGuard {
    fn drop(&mut self) {
        self.flush();
    }
}

// ── Reporter ────────────────────────────────────────────────────

struct TraceReporter {
    state: Arc<Mutex<WriterState>>,
    include_args: bool,
    include_tags: bool,
}

impl Reporter for TraceReporter {
    fn start(&self, queue: EventQueue) {
        let state = self.state.clone();
        let include_args = self.include_args;
        let include_tags = self.include_tags;
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(10));
                let events = std::mem::take(&mut *queue.lock().unwrap());
                if !events.is_empty() {
                    let mut s = state.lock().unwrap();
                    if !s.finalized {
                        let epoch = s.epoch;
                        let _ = writer::write_events(
                            &mut s.writer,
                            &events,
                            epoch,
                            include_args,
                            include_tags,
                        );
                    }
                }
            }
        });
    }
}

// ── WriterState ─────────────────────────────────────────────────

struct WriterState {
    writer: Box<dyn Write + Send>,
    epoch: SystemTime,
    finalized: bool,
}

impl WriterState {
    fn new(mut w: Box<dyn Write + Send>, process_name: &str) -> Self {
        let epoch = SystemTime::now();
        let _ = writer::write_header(&mut w, process_name);
        WriterState {
            writer: w,
            epoch,
            finalized: false,
        }
    }

    fn finalize(&mut self) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        let _ = writer::write_footer(&mut self.writer);
        let _ = self.writer.flush();
    }
}
