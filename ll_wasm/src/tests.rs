use anyhow::Result;
use ll::reporters::Reporter;
use ll::task_tree::{TaskInternal, TaskResult, TaskStatus, TaskTree};
use std::sync::{Arc, Mutex};
use wasm_bindgen_test::*;

// ── Test reporter: collects events into a Vec for assertions ─────

#[derive(Clone)]
struct VecReporter {
    events: Arc<Mutex<Vec<String>>>,
}

impl VecReporter {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }
}

impl Reporter for VecReporter {
    fn task_start(&self, task: Arc<TaskInternal>) {
        self.events
            .lock()
            .unwrap()
            .push(format!("start:{}", task.full_name()));
    }

    fn task_end(&self, task: Arc<TaskInternal>) {
        let status = match &task.status {
            TaskStatus::Finished(TaskResult::Success, _) => "ok",
            TaskStatus::Finished(TaskResult::Failure(_), _) => "err",
            TaskStatus::Running => "running",
        };
        self.events
            .lock()
            .unwrap()
            .push(format!("end:{status}:{}", task.full_name()));
    }
}

fn setup() -> (Arc<TaskTree>, VecReporter) {
    let reporter = VecReporter::new();
    let tt = TaskTree::new();
    tt.set_force_flush(true);
    tt.add_reporter(Arc::new(reporter.clone()));
    (tt, reporter)
}

// ── Tests ────────────────────────────────────────────────────────

#[wasm_bindgen_test]
fn create_task_and_spawn_sync() {
    let (tt, r) = setup();
    let root = tt.create_task("root");

    root.spawn_sync("child", |t| {
        t.data("key", "value");
        Ok(())
    })
    .unwrap();

    tt.report_all();
    let events = r.events();
    assert!(events.contains(&"start:root".to_string()));
    assert!(events.contains(&"start:root:child".to_string()));
    assert!(events.contains(&"end:ok:root:child".to_string()));
}

#[wasm_bindgen_test]
fn nested_tasks() {
    let (tt, r) = setup();
    let root = tt.create_task("pipeline");

    root.spawn_sync("build", |t| {
        t.spawn_sync("compile", |_| Ok(()))?;
        t.spawn_sync("link", |_| Ok(()))?;
        Ok(())
    })
    .unwrap();

    tt.report_all();
    let events = r.events();
    assert!(events.contains(&"start:pipeline:build:compile".to_string()));
    assert!(events.contains(&"start:pipeline:build:link".to_string()));
    assert!(events.contains(&"end:ok:pipeline:build".to_string()));
}

#[wasm_bindgen_test]
fn error_propagation() {
    let (tt, r) = setup();
    let root = tt.create_task("root");

    let result: Result<()> = root.spawn_sync("will_fail", |_| {
        anyhow::bail!("oops");
    });

    assert!(result.is_err());
    tt.report_all();
    let events = r.events();
    assert!(events.contains(&"end:err:root:will_fail".to_string()));
}

#[wasm_bindgen_test]
fn transitive_data() {
    let (tt, _r) = setup();
    let root = tt.create_task("root");
    root.data_transitive("session", "abc-123");

    root.spawn_sync("child", |t| {
        let val = t.get_data("session");
        assert_eq!(val.unwrap().to_string(), "abc-123");
        Ok(())
    })
    .unwrap();
}

#[wasm_bindgen_test]
fn tags_extracted_from_name() {
    let (tt, _r) = setup();
    let root = tt.create_task("root");

    root.spawn_sync("step #l2 #nostatus", |_| Ok(())).unwrap();

    let tree = tt.tree_internal.read().unwrap();
    let task = tree
        .tasks_internal
        .values()
        .find(|t| t.name == "step")
        .expect("task must exist");
    assert!(task.tags.contains("l2"));
    assert!(task.tags.contains("nostatus"));
}

#[wasm_bindgen_test]
fn console_reporter_smoke_test() {
    let tt = TaskTree::new();
    tt.set_force_flush(true);
    tt.add_reporter(Arc::new(crate::ConsoleReporter::new()));

    let root = tt.create_task("wasm_smoke_test");
    root.spawn_sync("hello_wasm", |t| {
        t.data("platform", "wasm32");
        Ok(())
    })
    .unwrap();

    tt.report_all();
    // If we got here without panicking, the ConsoleReporter works on WASM.
}
