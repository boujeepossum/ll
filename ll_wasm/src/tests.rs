use anyhow::Result;
use ll::reporters::{EventQueue, Reporter, TaskEvent};
use ll::task_tree::{TaskResult, TaskStatus, TaskTree};
use std::sync::{Arc, Mutex, RwLock};
use wasm_bindgen_test::*;

#[derive(Clone)]
struct VecReporter {
    queue: Arc<RwLock<Option<EventQueue>>>,
    events: Arc<Mutex<Vec<String>>>,
}

impl VecReporter {
    fn new() -> Self {
        Self {
            queue: Arc::new(RwLock::new(None)),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn drain(&self) {
        let queue_guard = self.queue.read().unwrap();
        let Some(queue) = queue_guard.as_ref() else {
            return;
        };
        let raw_events = std::mem::take(&mut *queue.lock().unwrap());
        drop(queue_guard);

        let mut events = self.events.lock().unwrap();
        for event in raw_events {
            match event {
                TaskEvent::Start(task) => {
                    events.push(format!("start:{}", task.full_name()));
                }
                TaskEvent::End(task) => {
                    let status = match &task.status {
                        TaskStatus::Finished(TaskResult::Success, _) => "ok",
                        TaskStatus::Finished(TaskResult::Failure(_), _) => "err",
                        TaskStatus::Running => "running",
                    };
                    events.push(format!("end:{status}:{}", task.full_name()));
                }
                TaskEvent::Progress(_) => {}
            }
        }
    }

    fn events(&self) -> Vec<String> {
        self.drain();
        self.events.lock().unwrap().clone()
    }
}

impl Reporter for VecReporter {
    fn start(&self, queue: EventQueue) {
        *self.queue.write().unwrap() = Some(queue);
    }
}

fn setup() -> (Arc<TaskTree>, VecReporter) {
    let reporter = VecReporter::new();
    let tt = TaskTree::new();
    tt.add_reporter(Arc::new(reporter.clone()));
    (tt, reporter)
}

#[wasm_bindgen_test]
fn create_task_and_spawn_sync() {
    let (tt, r) = setup();
    let root = tt.create_task("root");

    root.spawn_sync("child", |t| {
        t.data("key", "value");
        Ok(())
    })
    .unwrap();

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
    let (tt, r) = setup();
    let root = tt.create_task("root");

    root.spawn_sync("step #l2 #nostatus", |_| Ok(())).unwrap();

    let events = r.events();
    assert!(events.contains(&"start:root:step".to_string()));
    assert!(events.contains(&"end:ok:root:step".to_string()));
}

#[wasm_bindgen_test]
fn console_reporter_smoke_test() {
    let tt = TaskTree::new();
    tt.add_reporter(Arc::new(crate::ConsoleReporter::new()));

    let root = tt.create_task("wasm_smoke_test");
    root.spawn_sync("hello_wasm", |t| {
        t.data("platform", "wasm32");
        Ok(())
    })
    .unwrap();
}
