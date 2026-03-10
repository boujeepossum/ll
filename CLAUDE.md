# ll

Structured, task-based logging/instrumentation library for async Rust. Instead of flat log lines, `ll` wraps sections of code into hierarchical **Tasks** that form a tree. Each task emits `start`/`end` events consumed by pluggable **Reporters** — text loggers, in-memory capture for tests, or a live terminal status display with progress bars and colored indicators.

The key use case is instrumenting complex async workflows (e.g. a CLI tool orchestrating many concurrent operations) where you need both a scrolling log and a live "what's running right now" view.

## Architecture

```
User Code
   │
   ▼
  Task           thin Arc-wrapped handle, user-facing API
   │
   ▼
 TaskTree        global singleton (lazy_static), core engine
   │
   ├──► Reporter trait (observers)
   │      ├── StdioReporter   text log to stderr/stdout
   │      ├── StringReporter   in-memory capture for tests
   │      └── TermStatus       live TUI status tree (crossterm)
   │
   ├──► TaskInternal           per-task state (status, data, tags, progress)
   ├──► Data / DataValue       structured key-value metadata
   ├──► UniqID                 monotonic u64 identity
   └──► utils                  hashtag tag extraction from names
```

### Key concepts

- **TaskTree** (`task_tree.rs`) — process-wide `Arc<TaskTree>` behind a `lazy_static`. All mutable state lives in `TaskTreeInternal` behind a single `RwLock`. On creation it spawns two background workers: a tokio task for GC (every 500ms) and a native OS thread for reporter dispatch (every 10ms).

- **Task** (`task.rs`) — the user-facing handle. Created via `Task::create_new()` (root) or `task.spawn()` / `task.create()` (child). RAII-based lifecycle: dropping the handle marks the task finished. `spawn` closures handle completion explicitly with error context.

- **Tag system** (`utils.rs`) — metadata encoded inline via hashtag syntax. `"download #l3 #nostatus"` becomes name `"download"` with tags `{"l3", "nostatus"}`. Tags control reporter visibility, log level filtering, etc.

- **Transitive data** — data marked transitive propagates from parent to child tasks at creation time. Useful for request IDs, session context.

- **Reporter trait** (`reporters/mod.rs`) — observer interface with `task_start`, `task_end`, `task_progress`. Three built-in implementations.

- **TermStatus** (`reporters/term_status.rs`) — live terminal tree rendered via crossterm on its own native thread. Acquires both stdout/stderr locks to prevent interleaving. DFS traversal with connector-line tracking for indentation.

## File layout

```
ll/src/
  lib.rs              public API, re-exports
  main.rs             demo binary
  task.rs             Task handle
  task_tree.rs         TaskTree, TaskTreeInternal, TaskInternal
  data.rs             Data, DataEntry, DataValue
  level.rs            data-level filtering (Info/Debug/Trace)
  uniq_id.rs          monotonic ID generator
  utils.rs            hashtag tag parsing
  reporters/
    mod.rs            Reporter trait
    level.rs          task-level filtering (L0–L3)
    text.rs           StdioReporter, StringReporter
    term_status.rs    live terminal status display
    utils.rs          level tag parsing
  tests/
    mod.rs
    basic_test.rs     snapshot-based integration tests
```

## Style

### Error handling

- **Use `anyhow::Result` everywhere.** Avoid `.unwrap()` and `.expect()` unless there is a true invariant that cannot be expressed by types.
- **Put `.context()` inside the function**, not at every call site. The function knows what it does — let it provide the context once. Callers should just use `?` without adding redundant context.

```rust
// GOOD: context lives inside the function — callers just use `?`
fn load_config(path: &Path) -> Result<Config> {
  {
    let text = std::fs::read_to_string(path)
        .context("failed to read config file")?;
    toml::from_str(&text)
        .context("failed to parse config file")
  }.context("failed to load config")
}

let cfg = load_config(path)?; // no .context() needed — load_config handles it

// BAD: context added at the call site — now every caller has to repeat this
let cfg = load_config(path).context("failed to load config")?;
```

### Testing

- **Prefer high-level, end-to-end-style tests.** A single test with randomized ingestion of multiple graphs, reconstruction, and comparison tests the whole flow and hits edge cases that unit tests miss.
- **Declarative, procedural test structure.** The test body should be a sequence of one-liner setup calls + `snapshot!()` assertions. It is fine to test multiple things in a single test — go higher level until it starts hurting runtime or comprehensiveness.
- **Randomized roundtrip tests** are particularly valuable for serialization, delta derive/apply, and storage. Use deterministic seeds so failures are reproducible.
- **Do not blindly update snapshots.** Look at failures first, validate the change is intentional, then update snapshots

### Architecture & documentation

- **Optimize architecture for easy testing.** Modular components, fast environment setup and reset. We want to move fast without breaking things.
- **Write high-level docs for larger modules** — describe the basic idea, architecture, why certain decisions were made, trade-offs, what to look for. Light ASCII diagrams are welcome.
- **Public API at the top of the file.** Make it follow the one-liner/declarative style. All implementation details go to the bottom. Don't make things `pub` unless they really need to be.
