use crate::data::{Data, DataEntry, DataValue};
use crate::reporters::Reporter;
use crate::task::{Task, TaskData};
use crate::uniq_id::UniqID;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use web_time::SystemTime;

lazy_static::lazy_static! {
    pub static ref TASK_TREE: Arc<TaskTree>  = TaskTree::new();
}

pub fn add_reporter(reporter: Arc<dyn Reporter>) {
    TASK_TREE.add_reporter(reporter);
}

pub trait ErrorFormatter: Send + Sync {
    fn format_error(&self, err: &anyhow::Error) -> String;
}

pub struct TaskTree {
    pub tree_internal: RwLock<TaskTreeInternal>,
    /// If true, it will block the current thread until all task events are
    /// reported (e.g. written to STDOUT)
    force_flush: AtomicBool,
}

pub struct TaskTreeInternal {
    pub tasks_internal: BTreeMap<UniqID, TaskInternal>,
    parent_to_children: BTreeMap<UniqID, BTreeSet<UniqID>>,
    child_to_parents: BTreeMap<UniqID, BTreeSet<UniqID>>,
    root_tasks: BTreeSet<UniqID>,
    reporters: Vec<Arc<dyn Reporter>>,
    tasks_marked_for_deletion: HashMap<UniqID, SystemTime>,
    report_start: Vec<UniqID>,
    report_end: Vec<UniqID>,
    data_transitive: Data,
    remove_task_after_done_ms: u64,
    hide_errors_default_msg: Option<Arc<String>>,
    attach_transitive_data_to_errors_default: bool,
    error_formatter: Option<Arc<dyn ErrorFormatter>>,
}

#[derive(Clone)]
pub struct TaskInternal {
    pub id: UniqID,
    pub name: String,
    pub parent_names: Vec<String>,
    pub started_at: SystemTime,
    pub status: TaskStatus,
    pub data: Data,
    pub data_transitive: Data,
    pub tags: BTreeSet<String>,
    /// optional tuple containing values indicating task progress, where
    /// first value is how many items finished and the second value is how many
    /// items there are total. E.g. if it's a task processing 10 pieces of work,
    /// (1, 10) would mean that 1 out of ten pieces is done.
    pub progress: Option<(i64, i64)>,
    pub hide_errors: Option<Arc<String>>,
    pub attach_transitive_data_to_errors: bool,
}

#[derive(Clone)]
pub enum TaskStatus {
    Running,
    Finished(TaskResult, SystemTime),
}

#[derive(Clone)]
pub enum TaskResult {
    Success,
    Failure(String),
}

impl TaskTree {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            tree_internal: RwLock::new(TaskTreeInternal {
                tasks_internal: BTreeMap::new(),
                parent_to_children: BTreeMap::new(),
                child_to_parents: BTreeMap::new(),
                root_tasks: BTreeSet::new(),
                reporters: vec![],
                tasks_marked_for_deletion: HashMap::new(),
                report_start: vec![],
                report_end: vec![],
                data_transitive: Data::empty(),
                remove_task_after_done_ms: 0,
                hide_errors_default_msg: None,
                attach_transitive_data_to_errors_default: true,
                error_formatter: None,
            }),
            force_flush: AtomicBool::new(false),
        })
    }

    pub fn set_force_flush(&self, enabled: bool) {
        self.force_flush.store(enabled, Ordering::SeqCst)
    }

    pub fn force_flush_enabled(&self) -> bool {
        self.force_flush.load(Ordering::SeqCst)
    }

    pub fn create_task(self: &Arc<Self>, name: &str) -> Task {
        let id = self.create_task_internal(name, None);
        Task(Arc::new(TaskData {
            id,
            task_tree: self.clone(),
            mark_done_on_drop: true,
        }))
    }

    pub fn add_reporter(&self, reporter: Arc<dyn Reporter>) {
        self.tree_internal.write().unwrap().reporters.push(reporter);
    }

    fn pre_spawn(self: &Arc<Self>, name: String, parent: Option<UniqID>) -> Task {
        let task = Task(Arc::new(TaskData {
            id: self.create_task_internal(&name, parent),
            task_tree: self.clone(),
            mark_done_on_drop: false,
        }));
        self.maybe_force_flush();
        task
    }

    fn post_spawn<T>(self: &Arc<Self>, id: UniqID, result: Result<T>) -> Result<T> {
        let result = result.with_context(|| {
            let mut desc = String::from("[Task]");
            if let Some(task_internal) = self.get_cloned_task(id) {
                desc.push_str(&format!(" {}", task_internal.name));
                if task_internal.attach_transitive_data_to_errors {
                    for (k, v) in task_internal.all_data() {
                        desc.push_str(&format!("\n  {k}: {}", v.0));
                    }
                } else {
                    for (k, v) in &task_internal.data.map {
                        desc.push_str(&format!("\n  {k}: {}", v.0));
                    }
                };
                if !desc.is_empty() {
                    desc.push('\n');
                }
            }
            desc
        });
        let error_msg = if let Err(err) = &result {
            let formatter = {
                let formatter = self.tree_internal.read().unwrap().error_formatter.clone();
                formatter
            };
            if let Some(formatter) = formatter {
                Some(formatter.format_error(err))
            } else {
                Some(format!("{err:?}"))
            }
        } else {
            None
        };
        self.mark_done(id, error_msg);
        self.maybe_force_flush();
        result
    }

    pub fn spawn_sync<F, T>(
        self: &Arc<Self>,
        name: String,
        f: F,
        parent: Option<UniqID>,
    ) -> Result<T>
    where
        F: FnOnce(Task) -> Result<T>,
        T: Send,
    {
        let task = self.pre_spawn(name, parent);
        let id = task.0.id;
        let result = f(task);
        self.post_spawn(id, result)
    }

    pub(crate) async fn spawn<F, FT, T>(
        self: &Arc<Self>,
        name: String,
        f: F,
        parent: Option<UniqID>,
    ) -> Result<T>
    where
        F: FnOnce(Task) -> FT,
        FT: Future<Output = Result<T>> + Send,
        T: Send,
    {
        let task = self.pre_spawn(name, parent);
        let id = task.0.id;
        let result = f(task).await;
        self.post_spawn(id, result)
    }

    #[cfg(feature = "tokio")]
    pub(crate) async fn spawn_tokio<F, FT, T>(
        self: &Arc<Self>,
        name: String,
        f: F,
        parent: Option<UniqID>,
    ) -> Result<T>
    where
        F: FnOnce(Task) -> FT + Send + 'static,
        FT: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let task = self.pre_spawn(name, parent);
        let id = task.0.id;
        let tree = self.clone();
        match tokio::spawn(async move {
            let result = f(task).await;
            tree.post_spawn(id, result)
        })
        .await
        {
            Ok(result) => result,
            Err(join_err) => {
                let msg = format!("spawned task panicked: {join_err}");
                self.mark_done(id, Some(msg.clone()));
                self.maybe_force_flush();
                Err(anyhow::anyhow!(msg))
            }
        }
    }

    #[cfg(feature = "tokio")]
    pub(crate) async fn spawn_blocking<F, T>(
        self: &Arc<Self>,
        name: String,
        f: F,
        parent: Option<UniqID>,
    ) -> Result<T>
    where
        F: FnOnce(Task) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let task = self.pre_spawn(name, parent);
        let id = task.0.id;
        let tree = self.clone();
        match tokio::task::spawn_blocking(move || {
            let result = f(task);
            tree.post_spawn(id, result)
        })
        .await
        {
            Ok(result) => result,
            Err(join_err) => {
                let msg = format!("blocking task panicked: {join_err}");
                self.mark_done(id, Some(msg.clone()));
                self.maybe_force_flush();
                Err(anyhow::anyhow!(msg))
            }
        }
    }

    pub fn create_task_internal<S: Into<String>>(
        self: &Arc<Self>,
        name: S,
        parent: Option<UniqID>,
    ) -> UniqID {
        let mut tree = self.tree_internal.write().unwrap();

        let mut parent_names = vec![];
        let mut data_transitive = tree.data_transitive.clone();
        let (name, tags) = crate::utils::extract_tags(name.into());
        let id = UniqID::new();
        if let Some(parent_task) = parent.and_then(|pid| tree.tasks_internal.get(&pid)) {
            parent_names = parent_task.parent_names.clone();
            parent_names.push(parent_task.name.clone());
            data_transitive.merge(&parent_task.data_transitive);
            let parent_id = parent_task.id;

            tree.parent_to_children
                .entry(parent_id)
                .or_default()
                .insert(id);
            tree.child_to_parents
                .entry(id)
                .or_default()
                .insert(parent_id);
        } else {
            tree.root_tasks.insert(id);
        }

        let task_internal = TaskInternal {
            status: TaskStatus::Running,
            name,
            parent_names,
            id,
            started_at: SystemTime::now(),
            data: Data::empty(),
            data_transitive,
            tags,
            progress: None,
            hide_errors: tree.hide_errors_default_msg.clone(),
            attach_transitive_data_to_errors: tree.attach_transitive_data_to_errors_default,
        };

        tree.tasks_internal.insert(id, task_internal);
        tree.report_start.push(id);

        id
    }

    pub fn mark_done(&self, id: UniqID, error_message: Option<String>) {
        let mut tree = self.tree_internal.write().unwrap();
        if let Some(task_internal) = tree.tasks_internal.get_mut(&id) {
            task_internal.mark_done(error_message);
            tree.mark_for_gc(id);
            tree.report_end.push(id);
        }
    }

    pub fn add_data<S: Into<String>, D: Into<DataValue>>(&self, id: UniqID, key: S, value: D) {
        let mut tree = self.tree_internal.write().unwrap();
        if let Some(task_internal) = tree.tasks_internal.get_mut(&id) {
            task_internal.data.add(key, value);
        }
    }

    pub fn get_data<S: Into<String>>(&self, id: UniqID, key: S) -> Option<DataValue> {
        let mut tree = self.tree_internal.write().unwrap();
        if let Some(task_internal) = tree.tasks_internal.get_mut(&id) {
            let all_data: BTreeMap<_, _> = task_internal.all_data().collect();
            return all_data.get(&key.into()).map(|de| de.0.clone());
        }
        None
    }

    pub(crate) fn add_data_transitive_for_task<S: Into<String>, D: Into<DataValue>>(
        &self,
        id: UniqID,
        key: S,
        value: D,
    ) {
        let mut tree = self.tree_internal.write().unwrap();
        if let Some(task_internal) = tree.tasks_internal.get_mut(&id) {
            task_internal.data_transitive.add(key, value);
        }
    }
    /// Reporters can use this flag to choose to not report errors.
    /// This is useful for cases where there's a large task chain and every
    /// single task reports a partial errors (that gets built up with each task)
    /// It would make sense to report it only once at the top level (thrift
    /// request, cli call, etc) and only mark other tasks.
    /// If set to Some, the message inside is what would be reported by default
    /// instead of reporting errors to avoid confusion (e.g. "error was hidden,
    /// see ...")
    pub fn hide_errors_default_msg<S: Into<String>>(&self, msg: Option<S>) {
        let mut tree = self.tree_internal.write().unwrap();
        let msg = msg.map(|msg| Arc::new(msg.into()));
        tree.hide_errors_default_msg = msg;
    }

    pub(crate) fn hide_error_msg_for_task(&self, id: UniqID, msg: Option<Arc<String>>) {
        let mut tree = self.tree_internal.write().unwrap();
        if let Some(task_internal) = tree.tasks_internal.get_mut(&id) {
            task_internal.hide_errors = msg;
        }
    }

    /// When errors occur, we attach task data to it in the description.
    /// If set to false, only task direct data will be attached and not
    /// transitive data. This is useful sometimes to remove the noise of
    /// transitive data appearing in every error in the chain (e.g. hostname)
    pub fn attach_transitive_data_to_errors_default(&self, val: bool) {
        let mut tree = self.tree_internal.write().unwrap();
        tree.attach_transitive_data_to_errors_default = val;
    }

    pub(crate) fn attach_transitive_data_to_errors_for_task(&self, id: UniqID, val: bool) {
        let mut tree = self.tree_internal.write().unwrap();
        if let Some(task_internal) = tree.tasks_internal.get_mut(&id) {
            task_internal.attach_transitive_data_to_errors = val;
        }
    }

    /// Add a custom error formatter to change how error messages look in
    /// reporters.
    /// Unfortunately it is not configurable per reporter, because errors
    /// normally don't implement `Clone` and it will be almost impossible to add
    /// reference counters to all errors in all chains
    pub fn set_error_formatter(&self, error_formatter: Option<Arc<dyn ErrorFormatter>>) {
        let mut tree = self.tree_internal.write().unwrap();
        tree.error_formatter = error_formatter;
    }

    /// Add transitive data to the task tree. This transitive data will be
    /// added to every task created in this task tree
    pub fn add_data_transitive<S: Into<String>, D: Into<DataValue>>(&self, key: S, value: D) {
        let mut tree = self.tree_internal.write().unwrap();
        tree.data_transitive.add(key, value);
    }

    pub fn task_progress(&self, id: UniqID, done: i64, total: i64) {
        let mut tree = self.tree_internal.write().unwrap();
        if let Some(task_internal) = tree.tasks_internal.get_mut(&id) {
            task_internal.progress = Some((done, total));
        }
    }

    fn get_cloned_task(&self, id: UniqID) -> Option<TaskInternal> {
        let tree = self.tree_internal.read().unwrap();
        tree.get_task(id).ok().cloned()
    }

    /// If force_flush set to true, this function will block the thread until everything
    /// is reported. Useful for cases when the process exits before all async events
    /// are reported and stuff is missing from stdout.
    pub fn maybe_force_flush(&self) {
        if self.force_flush.load(Ordering::SeqCst) {
            self.report_all();
        }
    }

    /// Run garbage collection to remove finished tasks from memory.
    /// Without background workers, callers (e.g. a reporter thread in
    /// `ll_stdout`) should call this periodically.
    pub fn garbage_collect(&self) {
        let mut tree = self.tree_internal.write().unwrap();
        tree.garbage_collect();
    }

    pub fn report_all(&self) {
        let mut tree = self.tree_internal.write().unwrap();
        let (start_tasks, end_tasks, reporters) = tree.get_tasks_and_reporters();
        drop(tree);
        for reporter in reporters {
            for task in &start_tasks {
                reporter.task_start(task.clone());
            }
            for task in &end_tasks {
                reporter.task_end(task.clone());
            }
        }
    }
}

impl TaskTreeInternal {
    pub fn get_task(&self, id: UniqID) -> Result<&TaskInternal> {
        self.tasks_internal.get(&id).context("task must be present")
    }

    pub fn root_tasks(&self) -> &BTreeSet<UniqID> {
        &self.root_tasks
    }

    pub fn child_to_parents(&self) -> &BTreeMap<UniqID, BTreeSet<UniqID>> {
        &self.child_to_parents
    }

    pub fn parent_to_children(&self) -> &BTreeMap<UniqID, BTreeSet<UniqID>> {
        &self.parent_to_children
    }

    fn mark_for_gc(&mut self, id: UniqID) {
        let mut stack = vec![id];

        let mut tasks_to_finished_status = BTreeMap::new();

        while let Some(id) = stack.pop() {
            if let Some(task_internal) = self.tasks_internal.get(&id) {
                tasks_to_finished_status
                    .insert(id, matches!(task_internal.status, TaskStatus::Finished(..)));
            }

            for child_id in self.parent_to_children.get(&id).into_iter().flatten() {
                stack.push(*child_id);
            }
        }

        if tasks_to_finished_status
            .iter()
            .all(|(_, finished)| *finished)
        {
            for id in tasks_to_finished_status.keys().copied() {
                self.tasks_marked_for_deletion
                    .entry(id)
                    .or_insert_with(SystemTime::now); // can't use or_default, need current time
            }

            // This sub branch might have been holding other parent branches that
            // weren't able to be garbage collected because of this subtree. we'll go
            // level up and perform the same logic.
            let parents = self.child_to_parents.get(&id).cloned().unwrap_or_default();
            for parent_id in parents {
                self.mark_for_gc(parent_id);
            }
        }
    }

    fn garbage_collect(&mut self) {
        let mut will_delete = vec![];
        for (id, time) in &self.tasks_marked_for_deletion {
            if let Ok(elapsed) = time.elapsed() {
                if elapsed > Duration::from_millis(self.remove_task_after_done_ms) {
                    will_delete.push(*id);
                }
            }
        }

        for id in will_delete {
            self.tasks_internal.remove(&id);
            self.parent_to_children.remove(&id);
            self.root_tasks.remove(&id);
            if let Some(parents) = self.child_to_parents.remove(&id) {
                for parent in parents {
                    if let Some(children) = self.parent_to_children.get_mut(&parent) {
                        children.remove(&id);
                    }
                }
            }
            self.tasks_marked_for_deletion.remove(&id);
        }
    }

    #[allow(clippy::type_complexity)]
    fn get_tasks_and_reporters(
        &mut self,
    ) -> (
        Vec<Arc<TaskInternal>>,
        Vec<Arc<TaskInternal>>,
        Vec<Arc<dyn Reporter>>,
    ) {
        let mut start_ids = vec![];
        std::mem::swap(&mut start_ids, &mut self.report_start);
        let mut end_ids = vec![];
        std::mem::swap(&mut end_ids, &mut self.report_end);

        let mut start_tasks = vec![];
        let mut end_tasks = vec![];

        for id in start_ids {
            if let Ok(task_internal) = self.get_task(id) {
                start_tasks.push(Arc::new(task_internal.clone()));
            }
        }
        for id in end_ids {
            if let Ok(task_internal) = self.get_task(id) {
                end_tasks.push(Arc::new(task_internal.clone()));
            }
        }

        let reporters = self.reporters.clone();

        (start_tasks, end_tasks, reporters)
    }
}

impl TaskInternal {
    pub(crate) fn mark_done(&mut self, error_message: Option<String>) {
        let task_status = match error_message {
            None => TaskResult::Success,
            Some(msg) => TaskResult::Failure(msg),
        };
        self.status = TaskStatus::Finished(task_status, SystemTime::now());
    }

    pub fn full_name(&self) -> String {
        let mut full_name = String::new();
        for parent_name in &self.parent_names {
            full_name.push_str(parent_name);
            full_name.push(':');
        }
        full_name.push_str(&self.name);
        full_name
    }

    pub fn all_data(
        &self,
    ) -> std::iter::Chain<
        std::collections::btree_map::Iter<'_, String, DataEntry>,
        std::collections::btree_map::Iter<'_, String, DataEntry>,
    > {
        self.data.map.iter().chain(self.data_transitive.map.iter())
    }
}
