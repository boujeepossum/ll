/*!
# ll_wasm — JavaScript console reporter for ll

Reports task events to the browser/Node.js console via `web_sys::console`.

```ignore
ll_wasm::init();
ll::add_reporter(Arc::new(ll_wasm::ConsoleReporter::new()));
```

Call `init()` to start a JS `setInterval` timer that periodically
dispatches reporter events and runs garbage collection — the WASM
equivalent of the background threads that `TaskTree` spawns on native.
*/

mod console_reporter;

pub use console_reporter::ConsoleReporter;

use ll::task_tree::TASK_TREE;
use std::sync::atomic::{AtomicBool, Ordering};
use wasm_bindgen::prelude::*;

static WORKER_STARTED: AtomicBool = AtomicBool::new(false);

/// Start a JS `setInterval` timer that runs report + GC.
/// This is the WASM equivalent of the background threads that
/// `TaskTree::new()` spawns on native targets.
///
/// Safe to call multiple times — the timer is only started once.
pub fn init() {
    if WORKER_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let tick = Closure::wrap(Box::new(move || {
            TASK_TREE.report_all();
            let mut tree = TASK_TREE.tree_internal.write().unwrap();
            tree.garbage_collect();
        }) as Box<dyn FnMut()>);

        web_sys::window()
            .expect("no global window — use init_with_global for Node.js")
            .set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                50,
            )
            .expect("setInterval failed");

        tick.forget();
    }
}

#[cfg(test)]
mod tests;
