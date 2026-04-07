pub mod level;
pub mod utils;

pub use level::Level;

pub const DONTPRINT_TAG: &str = "dontprint";

use crate::task_tree::TaskInternal;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub enum TaskEvent {
    Start(Arc<TaskInternal>),
    End(Arc<TaskInternal>),
    Progress(Arc<TaskInternal>),
}

pub type EventQueue = Arc<Mutex<Vec<TaskEvent>>>;

pub trait Reporter: Send + Sync {
    /// Called once when the reporter is registered. The queue will
    /// receive events as tasks start/end/progress. The reporter is
    /// responsible for draining it (background thread, timer, on-demand).
    fn start(&self, queue: EventQueue);
}
