use crate::term_status;
use chrono::prelude::*;
use chrono::{DateTime, Local, Utc};
use colored::*;
use ll::reporters::{EventQueue, Level, Reporter, TaskEvent, DONTPRINT_TAG};
use ll::task_tree::{TaskInternal, TaskResult, TaskStatus};
use std::sync::{Arc, Mutex, RwLock};

/// Text reporter that logs to stderr (or stdout).
/// Spawns a background drain thread when started.
pub struct StdioReporter {
    pub timestamp_format: Option<TimestampFormat>,
    pub use_stdout: bool,
    pub log_task_start: bool,
    pub max_log_level: Level,
}

/// In-memory reporter that captures output as a string.
/// Drains lazily when `.to_string()` or `.drain()` is called.
#[derive(Clone)]
pub struct StringReporter {
    pub output: Arc<Mutex<String>>,
    queue: Arc<RwLock<Option<EventQueue>>>,
    timestamp_format: Arc<RwLock<TimestampFormat>>,
    duration_format: Arc<RwLock<DurationFormat>>,
    strip_ansi: bool,
}

#[derive(Clone, Copy)]
pub enum TaskReportType {
    Start,
    End,
}

impl Default for StdioReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl StdioReporter {
    pub fn new() -> Self {
        Self {
            timestamp_format: None,
            use_stdout: false,
            log_task_start: false,
            max_log_level: Level::default(),
        }
    }
}

impl Reporter for StdioReporter {
    fn start(&self, queue: EventQueue) {
        let timestamp_format = self.timestamp_format.unwrap_or(TimestampFormat::UTC);
        let use_stdout = self.use_stdout;
        let log_task_start = self.log_task_start;
        let max_log_level = self.max_log_level;

        crate::drain_loop(queue, move |events| {
            for event in events {
                let (task, report_type) = match event {
                    TaskEvent::Start(t) => {
                        if !log_task_start {
                            continue;
                        }
                        (t, TaskReportType::Start)
                    }
                    TaskEvent::End(t) => (t, TaskReportType::End),
                    TaskEvent::Progress(_) => continue,
                };

                let level = ll::reporters::utils::parse_level(&task);
                if level > max_log_level || task.tags.contains(DONTPRINT_TAG) {
                    continue;
                }

                let result = make_string(
                    &task,
                    timestamp_format,
                    DurationFormat::Milliseconds,
                    report_type,
                );

                if use_stdout {
                    println!("{result}");
                } else if term_status::is_active() {
                    term_status::buffer_line(result);
                } else {
                    eprintln!("{result}");
                }
            }
        });
    }
}

#[derive(Clone, Copy)]
#[allow(clippy::upper_case_acronyms)]
pub enum TimestampFormat {
    UTC,
    Local,
    None,
    Redacted,
}

#[derive(Clone, Copy)]
pub enum DurationFormat {
    Milliseconds,
    None,
}

pub fn strip_ansi(s: &str) -> String {
    String::from_utf8(strip_ansi_escapes::strip(s)).expect("not a utf8 string")
}

impl Default for StringReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl StringReporter {
    pub fn new() -> Self {
        Self {
            output: Arc::new(Mutex::new(String::new())),
            queue: Arc::new(RwLock::new(None)),
            timestamp_format: Arc::new(RwLock::new(TimestampFormat::Redacted)),
            duration_format: Arc::new(RwLock::new(DurationFormat::None)),
            strip_ansi: true,
        }
    }

    /// Drain all pending events from the queue into the output string.
    pub fn drain(&self) {
        let queue_guard = self.queue.read().unwrap();
        let Some(queue) = queue_guard.as_ref() else {
            return;
        };
        let events = std::mem::take(&mut *queue.lock().unwrap());
        drop(queue_guard);

        let timestamp_format = *self.timestamp_format.read().unwrap();
        let duration_format = *self.duration_format.read().unwrap();

        for event in events {
            let (task, report_type) = match event {
                TaskEvent::Start(t) => (t, TaskReportType::Start),
                TaskEvent::End(t) => (t, TaskReportType::End),
                TaskEvent::Progress(_) => continue,
            };

            if task.tags.contains(DONTPRINT_TAG) {
                continue;
            }

            let mut result = make_string(&task, timestamp_format, duration_format, report_type);
            if self.strip_ansi {
                result = strip_ansi(&result);
            }
            let mut output = self.output.lock().expect("poisoned lock");
            output.push_str(&result);
            output.push('\n');
        }
    }

    pub fn set_timestamp_format(&self, format: TimestampFormat) {
        *self.timestamp_format.write().unwrap() = format;
    }

    pub fn log_duration(&self, enabled: bool) {
        *self.duration_format.write().unwrap() = if enabled {
            DurationFormat::Milliseconds
        } else {
            DurationFormat::None
        };
    }
}

impl Reporter for StringReporter {
    fn start(&self, queue: EventQueue) {
        *self.queue.write().unwrap() = Some(queue);
    }
}

impl std::fmt::Display for StringReporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.drain();
        let s = self.output.lock().expect("poisoned lock");
        write!(f, "{}", &s)
    }
}

pub fn make_string(
    task_internal: &TaskInternal,
    timestamp_format: TimestampFormat,
    duration_format: DurationFormat,
    report_type: TaskReportType,
) -> String {
    let timestamp = format_timestamp(timestamp_format, task_internal, report_type);
    let status = format_status(task_internal, duration_format, report_type);
    let name = format_name(task_internal, report_type);
    let (mut data, error) = if let TaskReportType::End = report_type {
        (format_data(task_internal), format_error(task_internal))
    } else {
        (String::new(), String::new())
    };

    if !data.is_empty() && task_internal.hide_errors.is_some() {
        data = format!("{data}\n");
    }

    format!("{timestamp}{status}{name}{data}{error}")
}

fn format_timestamp(
    timestamp_format: TimestampFormat,
    task_internal: &TaskInternal,
    report_type: TaskReportType,
) -> String {
    let datetime: Option<DateTime<Utc>> = match report_type {
        TaskReportType::Start => Some(task_internal.started_at.into()),
        TaskReportType::End => {
            if let TaskStatus::Finished(_, at) = task_internal.status {
                Some(at.into())
            } else {
                None
            }
        }
    };

    match timestamp_format {
        TimestampFormat::None => String::new(),
        TimestampFormat::Redacted => "[ ] ".to_string(),
        TimestampFormat::Local => {
            if let Some(datetime) = datetime {
                let datetime: DateTime<Local> = datetime.into();
                let rounded = datetime.round_subsecs(0);
                let formatted = rounded.format("%I:%M:%S%p");
                format!("[{formatted}] ").dimmed().to_string()
            } else {
                "[          ]".to_string()
            }
        }
        TimestampFormat::UTC => {
            if let Some(datetime) = datetime {
                let rounded = datetime.round_subsecs(0);
                format!("[{rounded:?}] ").dimmed().to_string()
            } else {
                "[                    ]".to_string()
            }
        }
    }
}

fn format_name(task_internal: &TaskInternal, report_type: TaskReportType) -> ColoredString {
    match (&task_internal.status, report_type) {
        (TaskStatus::Finished(TaskResult::Failure(_), _), _) => {
            format!("[ERR] {}", task_internal.full_name()).red()
        }
        (_, TaskReportType::Start) => task_internal.full_name().yellow(),
        (_, TaskReportType::End) => task_internal.full_name().green(),
    }
}

fn format_status(
    task_internal: &TaskInternal,
    format: DurationFormat,
    report_type: TaskReportType,
) -> String {
    match report_type {
        TaskReportType::Start => format!("| {} | ", "STARTING".yellow()),
        TaskReportType::End => {
            if let TaskStatus::Finished(_, finished_at) = task_internal.status {
                let d = finished_at.duration_since(task_internal.started_at).ok();
                match (d, format) {
                    (Some(d), DurationFormat::Milliseconds) => {
                        format!("| {:>6}ms | ", d.as_millis())
                            .bold()
                            .dimmed()
                            .to_string()
                    }
                    (Some(_), DurationFormat::None) => String::new(),
                    (None, _) => String::new(),
                }
            } else {
                String::new()
            }
        }
    }
}

fn format_data(task_internal: &TaskInternal) -> String {
    let mut result = String::new();
    let mut data = vec![];
    for (k, entry) in task_internal.all_data() {
        if entry.1.contains(DONTPRINT_TAG) {
            continue;
        }
        data.push(format!("  |      {k}: {}", entry.0).dimmed().to_string());
    }
    if !data.is_empty() {
        result.push('\n');
        result.push_str(&data.join("\n"));
    }
    result
}

fn format_error(task_internal: &TaskInternal) -> String {
    let mut result = String::new();
    if let TaskStatus::Finished(TaskResult::Failure(error_msg), _) = &task_internal.status {
        if let Some(msg) = &task_internal.hide_errors {
            return msg.dimmed().red().to_string();
        }
        result.push_str("\n  |\n");
        let error_log = error_msg
            .split('\n')
            .map(|line| format!("  |  {line}"))
            .collect::<Vec<String>>()
            .join("\n");
        result.push_str(&error_log);
    }
    result
}
