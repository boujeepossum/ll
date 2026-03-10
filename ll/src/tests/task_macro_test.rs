use crate::task;
use crate::{task_tree::TaskTree, StringReporter, Task};
use anyhow::Result;
use k9::*;
use std::sync::Arc;
use std::time::Duration;

async fn sleep() {
    tokio::time::sleep(Duration::from_millis(100)).await;
}

fn setup() -> (Arc<TaskTree>, StringReporter) {
    let string_reporter = StringReporter::new();
    let tt = TaskTree::new();
    tt.add_reporter(Arc::new(string_reporter.clone()));
    (tt, string_reporter)
}

// ── Helper functions annotated with #[task] ──────────────────────────

// --- Edge case: task param named differently ---
#[task]
async fn step_one(parent: &Task) -> Result<()> {
    parent.data("from", "step_one");
    Ok(())
}

// --- Edge case: fully qualified type crate::Task ---
#[task]
async fn step_two(t: &crate::Task) -> Result<()> {
    t.data("from", "step_two");
    Ok(())
}

// --- Edge case: owned Task (not a reference) ---
#[task(sync)]
fn step_three(t: Task) -> Result<()> {
    t.data("from", "step_three");
    Ok(())
}

// --- tags(...) attribute ---
#[task(tags(l2))]
async fn verbose_step(task: &Task) -> Result<()> {
    task.data("detail", "noisy");
    Ok(())
}

// --- tags + name override ---
#[task(name = "check", tags(l3, nostatus))]
async fn run_checks(task: &Task) -> Result<()> {
    task.data("result", "pass");
    Ok(())
}

// --- tags + sync ---
#[task(sync, tags(l3))]
fn quiet_sync_step(task: &Task) -> Result<()> {
    task.data("mode", "quiet");
    Ok(())
}

// --- tags + data ---
#[task(tags(l2), data(env))]
async fn tagged_with_data(env: &str, task: &Task) -> Result<()> {
    Ok(())
}

// --- nested: parent tagged, child untagged ---
#[task(tags(l2))]
async fn tagged_parent(task: &Task) -> Result<()> {
    untagged_child(&task).await?;
    Ok(())
}

#[task]
async fn untagged_child(task: &Task) -> Result<()> {
    task.data("child", true);
    Ok(())
}

#[task]
async fn build(task: &Task) -> Result<()> {
    task.data("compiler", "rustc");
    Ok(())
}

#[task(sync)]
fn check_lockfile(task: &Task) -> Result<()> {
    task.data("lockfile", "Cargo.lock");
    Ok(())
}

#[task(data(environment, region))]
async fn deploy(environment: &str, region: &str, task: &Task) -> Result<()> {
    Ok(())
}

#[task(name = "test #l2")]
async fn run_tests(task: &Task) -> Result<()> {
    task.data("count", 42);
    Ok(())
}

#[task]
async fn outer(task: &Task) -> Result<()> {
    task.data("outer_data", "hello");
    inner(&task).await?;
    Ok(())
}

#[task]
async fn inner(task: &Task) -> Result<()> {
    task.data("inner_data", "world");
    Ok(())
}

#[task]
async fn failing_task(task: &Task) -> Result<()> {
    task.data("attempt", 1);
    anyhow::bail!("something went wrong");
}

#[task(sync)]
fn sync_with_children(task: &Task) -> Result<()> {
    task.data("parent_val", "p");
    task.spawn_sync("child_a", |t| {
        t.data("child_val", "a");
        Ok(())
    })?;
    task.spawn_sync("child_b", |t| {
        t.data("child_val", "b");
        Ok(())
    })?;
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────

/// Basic #[task] async spawn: function name becomes the task name,
/// data is attached to the child (not the parent).
#[tokio::test]
async fn macro_async_spawn() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    build(&root).await?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:build
[ ] root:build
  |      compiler: rustc

"
    );
    Ok(())
}

/// #[task(sync)] produces a synchronous spawn_sync call.
#[tokio::test]
async fn macro_sync_spawn() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    check_lockfile(&root)?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:check_lockfile
[ ] root:check_lockfile
  |      lockfile: Cargo.lock

"
    );
    Ok(())
}

/// #[task(data(...))] auto-logs listed parameters as task data.
#[tokio::test]
async fn macro_data_logging() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    deploy("production", "us-east-1", &root).await?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:deploy
[ ] root:deploy
  |      environment: production
  |      region: us-east-1

"
    );
    Ok(())
}

/// #[task(name = "...")] overrides the task name. Tags in the name
/// (like #l2) are parsed normally.
#[tokio::test]
async fn macro_name_override() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    run_tests(&root).await?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:test
[ ] root:test
  |      count: 42

"
    );
    Ok(())
}

/// Nested #[task] functions: outer calls inner. The task tree should be
/// root > outer > inner, proving the macro passes the child task (not the
/// parent) to nested calls.
#[tokio::test]
async fn macro_nested_tasks() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    outer(&root).await?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:outer
[ ] | STARTING | root:outer:inner
[ ] root:outer:inner
  |      inner_data: world
[ ] root:outer
  |      outer_data: hello

"
    );
    Ok(())
}

/// Error propagation: a failing #[task] fn returns Err and the task
/// tree shows the error with attached data.
#[tokio::test]
async fn macro_error_propagation() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    let result = failing_task(&root).await;
    assert!(result.is_err());

    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | [ERR] root:failing_task
[ ] [ERR] root:failing_task
  |      attempt: 1
  |
  |  [Task] failing_task
  |    attempt: 1
  |  
  |  
  |  Caused by:
  |      something went wrong

"
    );
    Ok(())
}

/// Sync task with manually-spawned children: verifies that `task` inside
/// the macro body is the child task, so children appear under the
/// macro-created task, not the root.
#[tokio::test]
async fn macro_sync_with_children() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    sync_with_children(&root)?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:sync_with_children
[ ] | STARTING | root:sync_with_children:child_a
[ ] | STARTING | root:sync_with_children:child_b
[ ] root:sync_with_children:child_a
  |      child_val: a
[ ] root:sync_with_children:child_b
  |      child_val: b
[ ] root:sync_with_children
  |      parent_val: p

"
    );
    Ok(())
}

/// Mixing macro and manual spawns in one tree: root uses macro for
/// one child, manual spawn for another. Both appear as siblings.
#[tokio::test]
async fn macro_mixed_with_manual() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");

    // Macro-based
    build(&root).await?;

    // Manual spawn on the same parent
    root.spawn("manual_step", |t| async move {
        t.data("mode", "manual");
        Ok(())
    })
    .await?;

    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:build
[ ] | STARTING | root:manual_step
[ ] root:build
  |      compiler: rustc
[ ] root:manual_step
  |      mode: manual

"
    );
    Ok(())
}

/// Transitive data flows through macro-spawned tasks correctly.
#[tokio::test]
async fn macro_transitive_data() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    root.data_transitive("request_id", "abc-123");

    outer(&root).await?;
    sleep().await;

    // Both outer and inner should have the transitive data
    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:outer
[ ] | STARTING | root:outer:inner
[ ] root:outer:inner
  |      inner_data: world
  |      request_id: abc-123
[ ] root:outer
  |      outer_data: hello
  |      request_id: abc-123

"
    );
    Ok(())
}

/// Edge case: task parameter can be named anything, not just `task`.
#[tokio::test]
async fn macro_task_param_named_differently() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    step_one(&root).await?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:step_one
[ ] root:step_one
  |      from: step_one

"
    );
    Ok(())
}

/// Edge case: Task type can be fully qualified (crate::Task, some_module::Task).
#[tokio::test]
async fn macro_fully_qualified_task_type() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    step_two(&root).await?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:step_two
[ ] root:step_two
  |      from: step_two

"
    );
    Ok(())
}

/// Edge case: owned Task (not a reference) works with sync spawn.
#[tokio::test]
async fn macro_owned_task() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    step_three(root.clone())?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:step_three
[ ] root:step_three
  |      from: step_three

"
    );
    Ok(())
}

/// tags(...) appends hashtag tags to the task name.
/// Verifies that (a) the display name strips the tag and (b) the tag
/// is actually stored on the TaskInternal for reporter filtering.
#[tokio::test]
async fn macro_tags() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    verbose_step(&root).await?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:verbose_step
[ ] root:verbose_step
  |      detail: noisy

"
    );

    // Verify the tag is actually stored on the task
    let tree = tt.tree_internal.read().unwrap();
    let child = tree
        .tasks_internal
        .values()
        .find(|t| t.name == "verbose_step")
        .expect("verbose_step task must exist");
    assert!(child.tags.contains("l2"), "tag 'l2' must be set");
    Ok(())
}

/// tags(...) combined with name override: both name and tags apply.
#[tokio::test]
async fn macro_tags_with_name_override() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    run_checks(&root).await?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:check
[ ] root:check
  |      result: pass

"
    );

    // Verify both tags are stored
    let tree = tt.tree_internal.read().unwrap();
    let child = tree
        .tasks_internal
        .values()
        .find(|t| t.name == "check")
        .expect("check task must exist");
    assert!(child.tags.contains("l3"));
    assert!(child.tags.contains("nostatus"));
    Ok(())
}

/// tags(...) with sync spawn.
#[tokio::test]
async fn macro_tags_sync() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    quiet_sync_step(&root)?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:quiet_sync_step
[ ] root:quiet_sync_step
  |      mode: quiet

"
    );

    let tree = tt.tree_internal.read().unwrap();
    let child = tree
        .tasks_internal
        .values()
        .find(|t| t.name == "quiet_sync_step")
        .expect("quiet_sync_step task must exist");
    assert!(child.tags.contains("l3"));
    Ok(())
}

/// tags(...) combined with data(...): both work together.
#[tokio::test]
async fn macro_tags_with_data() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    tagged_with_data("prod", &root).await?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:tagged_with_data
[ ] root:tagged_with_data
  |      env: prod

"
    );

    let tree = tt.tree_internal.read().unwrap();
    let child = tree
        .tasks_internal
        .values()
        .find(|t| t.name == "tagged_with_data")
        .expect("tagged_with_data task must exist");
    assert!(child.tags.contains("l2"));
    Ok(())
}

/// Nested tasks: parent has tags, child does not. Tags should NOT
/// propagate to children — only the parent task has the tag.
#[tokio::test]
async fn macro_tags_do_not_propagate_to_children() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");
    tagged_parent(&root).await?;
    sleep().await;

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:tagged_parent
[ ] | STARTING | root:tagged_parent:untagged_child
[ ] root:tagged_parent:untagged_child
  |      child: true
[ ] root:tagged_parent

"
    );

    let tree = tt.tree_internal.read().unwrap();
    let parent = tree
        .tasks_internal
        .values()
        .find(|t| t.name == "tagged_parent")
        .expect("tagged_parent must exist");
    let child = tree
        .tasks_internal
        .values()
        .find(|t| t.name == "untagged_child")
        .expect("untagged_child must exist");
    assert!(parent.tags.contains("l2"), "parent should have l2 tag");
    assert!(
        !child.tags.contains("l2"),
        "child should NOT inherit l2 tag"
    );
    Ok(())
}

/// Verify tags(l2) produces the same result as name = "fn_name #l2".
/// This ensures tags(...) is equivalent to manually embedding tags in the name.
#[tokio::test]
async fn macro_tags_equivalent_to_name_with_hashtag() -> Result<()> {
    let (tt, _s) = setup();
    let root = tt.create_task("root");

    // verbose_step uses tags(l2)
    verbose_step(&root).await?;
    // run_tests uses name = "test #l2"
    run_tests(&root).await?;

    let tree = tt.tree_internal.read().unwrap();
    let via_tags = tree
        .tasks_internal
        .values()
        .find(|t| t.name == "verbose_step")
        .unwrap();
    let via_name = tree
        .tasks_internal
        .values()
        .find(|t| t.name == "test")
        .unwrap();

    // Both should have the "l2" tag
    assert!(via_tags.tags.contains("l2"));
    assert!(via_name.tags.contains("l2"));
    Ok(())
}
