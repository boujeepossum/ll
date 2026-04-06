/*!
# ll_wasm — JavaScript console reporter for ll

Reports task events to the browser/Node.js console via `web_sys::console`.

```ignore
use ll_wasm::ConsoleReporter;
use std::sync::Arc;

ll::add_reporter(Arc::new(ConsoleReporter::new()));
```

Task starts log with `console.log`, successes with `console.log`,
failures with `console.error`.
*/

mod console_reporter;

pub use console_reporter::ConsoleReporter;

#[cfg(test)]
mod tests;
