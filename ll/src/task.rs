use crate::data::DataValue;
use crate::task_tree::{TaskTree, TASK_TREE};
use crate::uniq_id::UniqID;
use anyhow::Result;
use std::future::Future;
use std::sync::Arc;

pub type MarkDoneOnDrop = bool;

#[derive(Clone)]
pub struct Task(pub(crate) Arc<TaskData>);

pub(crate) struct TaskData {
    pub(crate) id: UniqID,
    pub(crate) task_tree: Arc<TaskTree>,
    pub(crate) mark_done_on_drop: MarkDoneOnDrop,
}

impl Task {
    pub fn create_new(name: &str) -> Self {
        let id = TASK_TREE.create_task_internal(name, None);
        Self(Arc::new(TaskData {
            id,
            task_tree: TASK_TREE.clone(),
            mark_done_on_drop: true,
        }))
    }

    pub fn create(&self, name: &str) -> Self {
        let id = self.0.task_tree.create_task_internal(name, Some(self.0.id));
        Self(Arc::new(TaskData {
            id,
            task_tree: self.0.task_tree.clone(),
            mark_done_on_drop: true,
        }))
    }

    /// Spawn a new top level task, with no parent.
    /// This should usually be done in the very beginning of
    /// the process/application.
    pub async fn spawn_new<F, FT, T>(name: &str, f: F) -> Result<T>
    where
        F: FnOnce(Task) -> FT,
        FT: Future<Output = Result<T>> + Send,
        T: Send,
    {
        TASK_TREE.spawn(name.into(), f, None).await
    }

    /// Spawn a new top level synchronous task, with no parent.
    pub fn spawn_sync_new<F, T>(name: &str, f: F) -> Result<T>
    where
        F: FnOnce(Task) -> Result<T>,
        T: Send,
    {
        TASK_TREE.spawn_sync(name.into(), f, None)
    }

    /// Run an async closure as a child task on the current async thread.
    ///
    /// The future runs inline — it is `.await`ed directly without creating
    /// a new tokio task. This means it shares the current tokio task and will
    /// not make progress unless the caller awaits the returned future.
    pub async fn spawn<F, FT, T, S: Into<String>>(&self, name: S, f: F) -> Result<T>
    where
        F: FnOnce(Task) -> FT,
        FT: Future<Output = Result<T>> + Send,
        T: Send,
    {
        self.0
            .task_tree
            .spawn(name.into(), f, Some(self.0.id))
            .await
    }

    /// Run an async closure as a child task on a **new tokio task**
    /// (`tokio::spawn`).
    ///
    /// Unlike [`spawn`](Self::spawn), the future runs concurrently on the
    /// tokio runtime — it does not block the caller's async task. Use this
    /// when you need true parallelism across multiple async operations without
    /// manually calling `tokio::spawn` and cloning the parent task handle.
    ///
    /// The closure and its return type must be `'static` because they are
    /// moved into a detached tokio task.
    ///
    /// If the spawned task panics, the ll task is marked as failed and the
    /// panic is returned as an `anyhow::Error`.
    ///
    /// Without the `tokio` feature, falls back to inline await (same as
    /// [`spawn`](Self::spawn)), so code compiles unchanged on WASM.
    pub async fn spawn_tokio<F, FT, T, S: Into<String>>(&self, name: S, f: F) -> Result<T>
    where
        F: FnOnce(Task) -> FT + Send + 'static,
        FT: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        self.0
            .task_tree
            .spawn_tokio(name.into(), f, Some(self.0.id))
            .await
    }

    /// Run a synchronous closure as a child task on the current thread.
    ///
    /// The closure runs inline and blocks the current thread until it returns.
    /// Good for cheap synchronous work. For CPU-heavy or blocking I/O work,
    /// use [`spawn_blocking`](Self::spawn_blocking) instead to avoid starving
    /// the async executor.
    pub fn spawn_sync<F, T, S: Into<String>>(&self, name: S, f: F) -> Result<T>
    where
        F: FnOnce(Task) -> Result<T>,
        T: Send,
    {
        self.0.task_tree.spawn_sync(name.into(), f, Some(self.0.id))
    }

    /// Run a synchronous closure as a child task on **tokio's blocking thread
    /// pool** (`tokio::task::spawn_blocking`).
    ///
    /// Use this for CPU-heavy computation or blocking I/O that would otherwise
    /// stall the async executor thread. The closure runs on a dedicated OS
    /// thread, and the returned future resolves once it completes.
    ///
    /// The closure must be `'static` because it is moved to a separate thread.
    ///
    /// If the blocking task panics, the ll task is marked as failed and the
    /// panic is returned as an `anyhow::Error`.
    ///
    /// Without the `tokio` feature, falls back to inline sync execution
    /// (same as [`spawn_sync`](Self::spawn_sync)), so code compiles
    /// unchanged on WASM.
    pub async fn spawn_blocking<F, T, S: Into<String>>(&self, name: S, f: F) -> Result<T>
    where
        F: FnOnce(Task) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.0
            .task_tree
            .spawn_blocking(name.into(), f, Some(self.0.id))
            .await
    }

    pub fn data<D: Into<DataValue>>(&self, name: &str, data: D) {
        self.0.task_tree.add_data(self.0.id, name, data);
    }

    /// Get a piece of previously set data or transitive data. This can be
    /// useful if session/request tracking IDs need to be past to other loggers,
    /// e.g. when shelling out to another process that needs to set the same
    /// `session_id` inside so we can group the events together.
    pub fn get_data(&self, name: &str) -> Option<DataValue> {
        self.0.task_tree.get_data(self.0.id, name)
    }

    pub fn data_transitive<D: Into<DataValue>>(&self, name: &str, data: D) {
        self.0
            .task_tree
            .add_data_transitive_for_task(self.0.id, name, data);
    }

    pub fn progress(&self, done: i64, total: i64) {
        self.0.task_tree.task_progress(self.0.id, done, total);
    }

    /// Reporters can use this flag to choose to not report errors.
    /// This is useful for cases where there's a large task chain and every
    /// single task reports a partial errors (that gets built up with each task)
    /// It would make sense to report it only once at the top level (thrift
    /// request, cli call, etc) and only mark other tasks.
    /// If set to Some, the message inside is what would be reported by default
    /// instead of reporting errors to avoid confusion (e.g. "error was hidden,
    /// see ...")
    /// see [hide_errors_default_msg()](crate::task_tree::TaskTree::hide_errors_default_msg)
    pub fn hide_error_msg(&self, msg: Option<String>) {
        let msg = msg.map(Arc::new);
        self.0.task_tree.hide_error_msg_for_task(self.0.id, msg);
    }

    /// When errors occur, we attach task data to it in the description.
    /// If set to false, only task direct data will be attached and not
    /// transitive data. This is useful sometimes to remove the noise of
    /// transitive data appearing in every error in the chain (e.g. hostname)
    /// see [attach_transitive_data_to_errors_default()](crate::task_tree::TaskTree::attach_transitive_data_to_errors_default)
    pub fn attach_transitive_data_to_errors(&self, val: bool) {
        self.0
            .task_tree
            .attach_transitive_data_to_errors_for_task(self.0.id, val);
    }
}

impl Drop for TaskData {
    fn drop(&mut self) {
        if self.mark_done_on_drop {
            self.task_tree.mark_done(self.id, None);
        }
    }
}
