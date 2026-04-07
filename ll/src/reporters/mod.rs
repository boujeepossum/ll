pub mod level;
pub mod utils;

pub use level::Level;

pub const DONTPRINT_TAG: &str = "dontprint";

use crate::task_tree::TaskInternal;
use std::sync::Arc;

pub trait Reporter: Send + Sync {
    fn task_start(&self, _task: Arc<TaskInternal>) {}
    fn task_end(&self, _task: Arc<TaskInternal>) {}
    fn task_progress(&self, _task: Arc<TaskInternal>) {}
}
