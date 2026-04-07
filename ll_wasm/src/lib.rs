/*!
# ll_wasm — JavaScript console reporter for ll

Reports task events to the browser/Node.js console via `web_sys::console`.

```ignore
use std::sync::Arc;
ll::add_reporter(Arc::new(ll_wasm::ConsoleReporter::new()));
```

Events are dispatched inline when tasks start/finish — no timer or
background thread needed.
*/

mod console_reporter;

pub use console_reporter::ConsoleReporter;

#[cfg(test)]
mod tests;
