use ll::reporters::TaskEvent;
use ll::task_tree::{TaskInternal, TaskResult, TaskStatus};
use ll::DataValue;
use serde::Serialize;
use std::io::Write;
use web_time::SystemTime;

// ── Public API ──────────────────────────────────────────────────

/// Write the JSON header that opens the trace file.
pub fn write_header(w: &mut impl Write, process_name: &str) -> std::io::Result<()> {
    write!(w, "{{\"traceEvents\":[\n")?;
    let meta = TraceEvent {
        ph: "M",
        name: "process_name",
        cat: "",
        ts: 0.0,
        dur: None,
        pid: 1,
        tid: 0,
        args: Some(serde_json::json!({ "name": process_name })),
    };
    serde_json::to_writer(&mut *w, &meta)?;
    Ok(())
}

/// Write the JSON footer that closes the trace file.
pub fn write_footer(w: &mut impl Write) -> std::io::Result<()> {
    write!(w, "\n]}}\n")
}

/// Convert a batch of task events into trace events and write them.
/// Only `End` events produce output (as complete `X` events with duration).
pub fn write_events(
    w: &mut impl Write,
    events: &[TaskEvent],
    epoch: SystemTime,
    include_args: bool,
    include_tags: bool,
) -> std::io::Result<()> {
    for event in events {
        let task = match event {
            TaskEvent::End(t) => t,
            _ => continue,
        };
        let trace = task_to_trace_event(task, epoch, include_args, include_tags);
        write!(w, ",\n")?;
        serde_json::to_writer(&mut *w, &trace)?;
    }
    Ok(())
}

// ── Internals ───────────────────────────────────────────────────

#[derive(Serialize)]
struct TraceEvent<'a> {
    ph: &'a str,
    name: &'a str,
    cat: &'a str,
    ts: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    dur: Option<f64>,
    pid: u64,
    tid: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<serde_json::Value>,
}

fn task_to_trace_event(
    task: &TaskInternal,
    epoch: SystemTime,
    include_args: bool,
    include_tags: bool,
) -> TraceEvent<'_> {
    let ts_us = to_micros(task.started_at, epoch);
    let dur_us = match &task.status {
        TaskStatus::Finished(_, end_time) => Some(to_micros(*end_time, epoch) - ts_us),
        TaskStatus::Running => None,
    };

    let cat = extract_level_tag(task);

    let args = build_args(task, include_args, include_tags);

    TraceEvent {
        ph: "X",
        name: &task.name,
        cat,
        ts: ts_us,
        dur: dur_us,
        pid: 1,
        tid: task.id.to_string().parse::<u64>().unwrap_or(0),
        args,
    }
}

fn to_micros(time: SystemTime, epoch: SystemTime) -> f64 {
    time.duration_since(epoch).unwrap_or_default().as_secs_f64() * 1_000_000.0
}

fn extract_level_tag(task: &TaskInternal) -> &'static str {
    for tag in &task.tags {
        match tag.as_str() {
            "l0" => return "l0",
            "l1" => return "l1",
            "l2" => return "l2",
            "l3" => return "l3",
            _ => {}
        }
    }
    "l1"
}

fn build_args(
    task: &TaskInternal,
    include_args: bool,
    include_tags: bool,
) -> Option<serde_json::Value> {
    let mut map = serde_json::Map::new();

    // status / error
    match &task.status {
        TaskStatus::Finished(TaskResult::Failure(msg), _) => {
            map.insert("error".into(), serde_json::Value::String(msg.clone()));
        }
        _ => {}
    }

    // parent path
    if !task.parent_names.is_empty() {
        map.insert(
            "parent".into(),
            serde_json::Value::String(task.parent_names.join(" > ")),
        );
    }

    // task data
    if include_args {
        for (key, entry) in &task.data.map {
            let val = match &entry.0 {
                DataValue::String(s) => serde_json::Value::String(s.clone()),
                DataValue::Int(i) => serde_json::json!(i),
                DataValue::Float(f) => serde_json::json!(f),
                DataValue::None => serde_json::Value::Null,
            };
            map.insert(key.clone(), val);
        }
    }

    // tags
    if include_tags && !task.tags.is_empty() {
        let tags: Vec<_> = task.tags.iter().cloned().collect();
        map.insert("tags".into(), serde_json::json!(tags));
    }

    if map.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(map))
    }
}
