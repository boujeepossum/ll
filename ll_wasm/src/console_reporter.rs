use ll::reporters::{Reporter, DONTPRINT_TAG};
use ll::task_tree::{TaskInternal, TaskResult, TaskStatus};
use std::sync::Arc;
use wasm_bindgen::JsValue;

pub struct ConsoleReporter {
    pub log_task_start: bool,
}

impl Default for ConsoleReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleReporter {
    pub fn new() -> Self {
        Self {
            log_task_start: false,
        }
    }
}

impl Reporter for ConsoleReporter {
    fn task_start(&self, task: Arc<TaskInternal>) {
        if self.log_task_start && !task.tags.contains(DONTPRINT_TAG) {
            let msg = format!("[START] {}", task.full_name());
            web_sys::console::log_1(&JsValue::from_str(&msg));
        }
    }

    fn task_end(&self, task: Arc<TaskInternal>) {
        if task.tags.contains(DONTPRINT_TAG) {
            return;
        }

        let name = task.full_name();
        let data = format_data(&task);

        match &task.status {
            TaskStatus::Finished(TaskResult::Failure(err), _) => {
                let msg = format!("[ERR] {name}{data}\n{err}");
                web_sys::console::error_1(&JsValue::from_str(&msg));
            }
            _ => {
                let msg = format!("[OK] {name}{data}");
                web_sys::console::log_1(&JsValue::from_str(&msg));
            }
        }
    }
}

fn format_data(task: &TaskInternal) -> String {
    let entries: Vec<String> = task
        .all_data()
        .filter(|(_, entry)| !entry.1.contains(DONTPRINT_TAG))
        .map(|(k, entry)| format!("{k}={}", entry.0))
        .collect();

    if entries.is_empty() {
        String::new()
    } else {
        format!(" ({})", entries.join(", "))
    }
}
