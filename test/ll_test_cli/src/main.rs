use anyhow::Result;
use ll::reporters::Level;
use ll::Task;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    ll_stdio::builder().max_log_level(Level::L3).init();
    let _trace = ll_trace::init("trace.json");

    let root = Task::create_new("pipeline #nostatus #l0");
    root.data_transitive("run_id", "test-run-42");

    ll_fixtures::run_pipeline(&root).await?;

    drop(root);
    tokio::time::sleep(Duration::from_secs(4)).await;
    Ok(())
}
