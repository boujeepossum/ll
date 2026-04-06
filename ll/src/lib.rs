/*!
# ll — structured task-tree instrumentation

**ll** instruments async and sync Rust code by wrapping sections into
hierarchical [`Task`]s. Each task emits `start`/`end` events consumed by
pluggable [`reporters::Reporter`]s (text loggers, in-memory capture, live
terminal status).

Tasks form a tree: every task can spawn children, and reporters see the full
parent-child structure.

- [Quick start](#quick-start)
- [The `#[task]` macro](#the-task-macro)
  - [Spawn variants](#spawn-variants)
  - [Optional attributes](#optional-attributes) — `data(...)`, `tags(...)`, `name = "..."`
  - [Examples](#examples)
- [Manual spawning](#manual-spawning)
- [Tags](#tags) — `#l0`–`#l3`, `#nostatus`, `#dontprint`

## Quick start

```ignore
use ll::{task, Task};
use anyhow::Result;

#[task]
async fn build(task: &Task) -> Result<()> {
    task.data("compiler", "rustc");
    compile(&task).await?;
    Ok(())
}

#[task]
async fn compile(task: &Task) -> Result<()> {
    // task tree: build > compile
    Ok(())
}

async fn run() -> Result<()> {
    ll::reporters::term_status::show();
    let root = Task::create_new("root");
    build(&root).await
}
```

## The `#[task]` macro

The [`macro@task`] attribute macro eliminates spawn boilerplate. It wraps your
function body in the appropriate `task.spawn*()` call, using the function
name as the task name and shadowing the parent task parameter with the
child task.

### Spawn variants

| Attribute | Spawn method | Function signature |
|---|---|---|
| `#[task]` | [`Task::spawn`] | `async fn` |
| `#[task(sync)]` | [`Task::spawn_sync`] | `fn` |
| `#[task(tokio)]` | [`Task::spawn_tokio`] | `async fn` |
| `#[task(blocking)]` | [`Task::spawn_blocking`] | `async fn` |

### Optional attributes

- **`data(arg1, arg2, ...)`** — auto-emit `task.data("arg", arg)` for the
  listed function parameters.
- **`tags(l2, nostatus, ...)`** — append `#`-tags to the task name. Tags
  control reporter visibility: `#l2`/`#l3` mute at lower log levels,
  `#nostatus` hides from the terminal status display.
- **`name = "custom"`** — override the task name (defaults to the function
  name). Can be combined with `tags(...)`.

Attributes combine freely: `#[task(sync, data(path), tags(l2))]`.

### Examples

Async task (most common):

```ignore
#[task]
async fn fetch(url: &str, task: &Task) -> Result<String> {
    task.data("url", url);
    Ok(reqwest::get(url).await?.text().await?)
}

// caller:
let body = fetch("https://example.com", &parent).await?;
```

Sync task with automatic data logging:

```ignore
#[task(sync, data(path))]
fn read_config(path: &str, task: &Task) -> Result<Config> {
    // task.data("path", path) is emitted automatically
    Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
}
```

Muting verbose tasks with tags:

```ignore
#[task(tags(l2))]
async fn verbose_step(task: &Task) -> Result<()> {
    // only shown when reporter log level is L2 or higher
    Ok(())
}
```

Nested tasks build the tree automatically:

```ignore
#[task]
async fn deploy(task: &Task) -> Result<()> {
    provision(&task).await?;  // task tree: deploy > provision
    restart(&task).await?;    // task tree: deploy > restart
    Ok(())
}
```

## Manual spawning

You can also spawn tasks without the macro:

```ignore
let root = Task::create_new("root");
root.spawn("subtask", |task| async move {
    task.spawn_sync("child", |task| {
        Ok(())
    })?;
    Ok(())
}).await?;
```

## Tags

Tags are metadata encoded inline in task names via `#` syntax. They control
reporter visibility and filtering.

| Tag | Effect |
|---|---|
| `#l0` | Reporter level L0 — highest priority, always shown |
| `#l1` | Reporter level L1 — default |
| `#l2` | Reporter level L2 — hidden unless reporter threshold is L2+ |
| `#l3` | Reporter level L3 — lowest priority, most filtered |
| `#nostatus` | Hidden from [`TermStatus`] live display (still in text logs) |
| `#dontprint` | Suppressed from all text reporters |

Tags can be set via the [`macro@task`] attribute (`tags(l2, nostatus)`) or
embedded directly in task names (`"download #l3"`). Unrecognized tags are
stored but have no built-in effect.

Data keys also support tags: `task.data("response_body #trace", val)` marks
the entry as trace-level, hiding it unless the data log level is set to Trace.
 */
#![allow(clippy::new_without_default)]

pub mod data;
pub mod level;
pub mod task;
pub mod task_tree;
pub mod uniq_id;
pub mod utils;

pub use ll_macros::task;
pub use task::Task;

pub mod reporters;
pub use task_tree::add_reporter;

pub use data::{Data, DataEntry, DataValue};
pub use task_tree::ErrorFormatter;
pub use task_tree::TaskInternal;
pub use task_tree::TaskTree;
