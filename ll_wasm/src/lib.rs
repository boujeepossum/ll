/*!
# ll_wasm — JavaScript console reporter for ll

Reports task events to the browser/Node.js console via `web_sys::console`.

```ignore
use std::sync::Arc;
ll::add_reporter(Arc::new(ll_wasm::ConsoleReporter::new()));
```

The reporter sets up a JS `setInterval` timer to drain its event queue
automatically.
*/

mod console_reporter;

pub use console_reporter::ConsoleReporter;

#[cfg(test)]
mod tests;
