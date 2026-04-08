# ll

Structured, task-based logging/instrumentation library for async Rust. Instead of flat log lines, `ll` wraps sections of code into hierarchical **Tasks** that form a tree. Each task emits `start`/`end` events consumed by pluggable **Reporters** — text loggers, in-memory capture for tests, or a live terminal status display with progress bars and colored indicators.

The key use case is instrumenting complex async workflows (e.g. a CLI tool orchestrating many concurrent operations) where you need both a scrolling log and a live "what's running right now" view.

## Workspace

```
ll/              core engine — compiles for native + WASM
ll_macros/       #[task] proc macro
ll_stdio/        terminal reporters (StdioReporter, StringReporter, TermStatus)
ll_trace/        Chrome Trace Format reporter (TraceReporter → JSON file)
ll_wasm/         WASM reporter (ConsoleReporter → JS console)
test/
  ll_fixtures/   reusable fixture task trees for testing
  ll_test_cli/   demo CLI that runs fixtures with ll_stdio
```

## Architecture

```
User Code
   │
   ▼
  Task              thin Arc-wrapped handle, user-facing API
   │
   ▼
 TaskTree           global singleton (lazy_static), core engine
   │
   ├──► EventQueue per reporter (Mutex<Vec<TaskEvent>>)
   │      core pushes Start/End/Progress events (non-blocking)
   │      each reporter drains on its own schedule
   │
   ├──► TaskInternal    per-task state (status, data, tags, progress)
   ├──► Data/DataValue  structured key-value metadata
   ├──► UniqID          monotonic u64 identity
   └──► utils           hashtag tag extraction from names

ll_stdio (separate crate):
   ├── StdioReporter    text log to stderr/stdout (background drain thread)
   ├── StringReporter   in-memory capture for tests (lazy drain)
   └── TermStatus       live TUI status tree (crossterm, own render thread)

ll_wasm (separate crate):
   └── ConsoleReporter  JS console.log/error (setInterval drain)

ll_trace (separate crate):
   └── TraceReporter    Chrome Trace JSON to file (background drain thread)
```

### Key concepts

- **TaskTree** (`ll/src/task_tree.rs`) — process-wide `Arc<TaskTree>` behind a `lazy_static`. All mutable state lives in `TaskTreeInternal` behind a single `RwLock`. No background threads — events are pushed to per-reporter queues synchronously, and tasks are cleaned up immediately when finished (drop-style, cascading up to parents).

- **Reporter trait** (`ll/src/reporters/mod.rs`) — single method: `fn start(&self, queue: EventQueue)`. Core creates a `Mutex<Vec<TaskEvent>>` per reporter and pushes events to it. The reporter owns the drain strategy (thread, timer, on-demand). This guarantees reporters can never block task threads.

- **Task** (`ll/src/task.rs`) — the user-facing handle. Created via `Task::create_new()` / `Task::spawn_new()` / `Task::spawn_sync_new()` (root) or `task.spawn()` / `task.create()` (child). RAII-based lifecycle: dropping the handle marks the task finished.

- **Spawn variants** — `spawn` (inline async) and `spawn_sync` (inline sync).

- **Tag system** (`ll/src/utils.rs`) — metadata encoded inline via hashtag syntax. `"download #l3 #nostatus"` becomes name `"download"` with tags `{"l3", "nostatus"}`. Tags control reporter visibility, log level filtering, etc.

- **Transitive data** — data marked transitive propagates from parent to child tasks at creation time. Useful for request IDs, session context.

- **Task cleanup** — when a task finishes, `try_remove()` checks if all children are gone. If yes, removes it and cascades up to the parent. No periodic GC needed.

- **`web-time`** — used instead of `std::time::SystemTime` so the core compiles for WASM (`SystemTime::now()` panics on `wasm32-unknown-unknown`).

## File layout

```
ll/src/
  lib.rs              public API, re-exports
  task.rs             Task handle
  task_tree.rs        TaskTree, TaskTreeInternal, TaskInternal, event dispatch
  data.rs             Data, DataEntry, DataValue
  level.rs            data-level filtering (Info/Debug/Trace)
  uniq_id.rs          monotonic ID generator
  utils.rs            hashtag tag parsing
  reporters/
    mod.rs            Reporter trait, TaskEvent enum, EventQueue type
    level.rs          reporter-level filtering (L0–L3)
    utils.rs          tag → reporter level parsing

ll_stdio/src/
  lib.rs              init(), builder(), spawn_drain_thread() helper
  text.rs             StdioReporter, StringReporter
  term_status.rs      live terminal status display

ll_wasm/src/
  lib.rs              crate root
  console_reporter.rs ConsoleReporter (JS console via web_sys)

ll_trace/src/
  lib.rs              TraceReporter, Builder, init(), FlushGuard
  writer.rs           Chrome Trace JSON event serialization

ll_macros/src/
  lib.rs              #[task] proc macro

test/ll_fixtures/src/
  lib.rs              run_pipeline() — reusable fixture task tree

test/ll_test_cli/src/
  main.rs             demo binary
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
