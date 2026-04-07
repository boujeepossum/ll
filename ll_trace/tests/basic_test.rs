use anyhow::Result;
use ll::task_tree::TaskTree;
use std::sync::Arc;

/// A writer that appends to a shared buffer.
#[derive(Clone)]
struct SharedWriter(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn setup_shared() -> (Arc<TaskTree>, SharedWriter, ll_trace::FlushGuard) {
    let shared_buf = Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = SharedWriter(shared_buf.clone());

    let (reporter, guard) = ll_trace::builder()
        .writer(writer.clone())
        .process_name("test")
        .build_reporter();

    let tt = TaskTree::new();
    tt.add_reporter(reporter);

    (tt, writer, guard)
}

#[tokio::test]
async fn basic_trace_output() -> Result<()> {
    let (tt, writer, guard) = setup_shared();

    let root = tt.create_task("root");
    root.spawn_sync("child_a", |t| {
        t.data("key", "value");
        Ok(())
    })?;
    root.spawn_sync("child_b", |_| Ok(()))?;
    drop(root);

    // Give the drain thread time to process events
    std::thread::sleep(std::time::Duration::from_millis(50));
    guard.flush();

    let output = String::from_utf8(writer.0.lock().unwrap().clone())?;

    // Should be valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&output)?;
    let events = parsed["traceEvents"].as_array().unwrap();

    // First event is the metadata event
    assert_eq!(events[0]["ph"], "M");
    assert_eq!(events[0]["name"], "process_name");
    assert_eq!(events[0]["args"]["name"], "test");

    // Remaining events are complete (X) events for finished tasks
    let task_events: Vec<_> = events.iter().filter(|e| e["ph"] == "X").collect();

    // We expect events for: root, child_a, child_b
    let names: Vec<_> = task_events.iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"child_a"));
    assert!(names.contains(&"child_b"));
    assert!(names.contains(&"root"));

    // child_a should have args with the data
    let child_a = task_events.iter().find(|e| e["name"] == "child_a").unwrap();
    assert_eq!(child_a["args"]["key"], "value");

    // All task events should have ts and dur
    for event in &task_events {
        assert!(event["ts"].as_f64().is_some());
        assert!(event["dur"].as_f64().is_some());
        assert_eq!(event["pid"], 1);
    }

    Ok(())
}

#[tokio::test]
async fn error_task_includes_error_in_args() -> Result<()> {
    let (tt, writer, guard) = setup_shared();

    let root = tt.create_task("root");
    let _: Result<()> = root.spawn_sync("failing", |_| {
        anyhow::bail!("something went wrong");
    });
    drop(root);

    std::thread::sleep(std::time::Duration::from_millis(50));
    guard.flush();

    let output = String::from_utf8(writer.0.lock().unwrap().clone())?;
    let parsed: serde_json::Value = serde_json::from_str(&output)?;
    let events = parsed["traceEvents"].as_array().unwrap();

    let failing = events.iter().find(|e| e["name"] == "failing").unwrap();
    assert!(failing["args"]["error"].as_str().unwrap().contains("something went wrong"));

    Ok(())
}

#[tokio::test]
async fn no_args_when_disabled() -> Result<()> {
    let shared_buf = Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = SharedWriter(shared_buf.clone());

    let (reporter, guard) = ll_trace::builder()
        .writer(writer.clone())
        .process_name("test")
        .include_args(false)
        .include_tags(false)
        .build_reporter();

    let tt = TaskTree::new();
    tt.add_reporter(reporter);

    let root = tt.create_task("root");
    root.spawn_sync("task_with_data", |t| {
        t.data("should_not_appear", "hidden");
        Ok(())
    })?;
    drop(root);

    std::thread::sleep(std::time::Duration::from_millis(50));
    guard.flush();

    let output = String::from_utf8(writer.0.lock().unwrap().clone())?;
    let parsed: serde_json::Value = serde_json::from_str(&output)?;
    let events = parsed["traceEvents"].as_array().unwrap();

    let task = events.iter().find(|e| e["name"] == "task_with_data").unwrap();
    // Should not contain the data key (args may still have parent path)
    assert!(task["args"].get("should_not_appear").is_none());

    Ok(())
}
