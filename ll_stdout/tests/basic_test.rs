use anyhow::Result;
use k9::*;
use ll::task_tree::TaskTree;
use ll::ErrorFormatter;
use ll_stdout::StringReporter;
use std::sync::Arc;

fn setup() -> (std::sync::Arc<TaskTree>, StringReporter) {
    let string_reporter = StringReporter::new();
    let tt = TaskTree::new();
    tt.set_force_flush(true);
    tt.add_reporter(Arc::new(string_reporter.clone()));
    (tt, string_reporter)
}

#[tokio::test]
async fn basic_events_test() -> Result<()> {
    let (tt, s) = setup();

    let root = tt.create_task("root");

    root.spawn_sync("test", |_| {
        let _r = 1 + 1;
        Ok(())
    })?;

    root.spawn_sync("test_with_data", |t| -> Result<()> {
        t.data("hello", "hi");
        t.data("int", 5);
        t.data("float", 5.98);
        anyhow::bail!("here is error msg");
    })
    .ok();

    root.spawn_sync("test_3", |_e| Ok(()))?;

    tt.report_all();
    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:test
[ ] root:test
[ ] | STARTING | root:test_with_data
[ ] [ERR] root:test_with_data
  |      float: 5.98
  |      hello: hi
  |      int: 5
  |
  |  [Task] test_with_data
  |    float: 5.98
  |    hello: hi
  |    int: 5
  |  
  |  
  |  Caused by:
  |      here is error msg
[ ] | STARTING | root:test_3
[ ] root:test_3

"
    );

    Ok(())
}

#[tokio::test]
async fn error_chain_test() -> Result<()> {
    let (tt, s) = setup();

    let root = tt.create_task("root");
    let result = root.spawn_sync("top_level", |t| {
        t.data("top_level_data", 5);

        t.spawn_sync("1_level", |t| {
            t.data("1_level_data", 9);
            t.spawn_sync("2_level", |_| {
                anyhow::ensure!(false, "oh noes, this fails");
                Ok(())
            })
        })?;
        Ok(())
    });

    tt.report_all();
    snapshot!(
        format!("{:?}", result.unwrap_err()),
        "
[Task] top_level
  top_level_data: 5


Caused by:
    0: [Task] 1_level
         1_level_data: 9
       
    1: [Task] 2_level
       
    2: oh noes, this fails
"
    );

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:top_level
[ ] | STARTING | root:top_level:1_level
[ ] | STARTING | root:top_level:1_level:2_level
[ ] [ERR] root:top_level:1_level:2_level
  |
  |  [Task] 2_level
  |  
  |  
  |  Caused by:
  |      oh noes, this fails
[ ] [ERR] root:top_level:1_level
  |      1_level_data: 9
  |
  |  [Task] 1_level
  |    1_level_data: 9
  |  
  |  
  |  Caused by:
  |      0: [Task] 2_level
  |         
  |      1: oh noes, this fails
[ ] [ERR] root:top_level
  |      top_level_data: 5
  |
  |  [Task] top_level
  |    top_level_data: 5
  |  
  |  
  |  Caused by:
  |      0: [Task] 1_level
  |           1_level_data: 9
  |         
  |      1: [Task] 2_level
  |         
  |      2: oh noes, this fails

"
    );
    Ok(())
}

#[tokio::test]
async fn error_chain_test_no_transitive() -> Result<()> {
    let (tt, s) = setup();

    tt.attach_transitive_data_to_errors_default(false);
    tt.add_data_transitive("transitive_data", "transitive_value");
    let root = tt.create_task("root");
    let result = root.spawn_sync("top_level", |t| {
        t.data("top_level_data", 5);

        t.spawn_sync("1_level", |t| {
            t.data("1_level_data", 9);
            t.attach_transitive_data_to_errors(true);
            t.spawn_sync("2_level", |_| {
                anyhow::ensure!(false, "oh noes, this fails");
                Ok(())
            })
        })?;
        Ok(())
    });

    tt.report_all();
    snapshot!(
        format!("{:?}", result.unwrap_err()),
        "
[Task] top_level
  top_level_data: 5


Caused by:
    0: [Task] 1_level
         1_level_data: 9
         transitive_data: transitive_value
       
    1: [Task] 2_level
       
    2: oh noes, this fails
"
    );

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:top_level
[ ] | STARTING | root:top_level:1_level
[ ] | STARTING | root:top_level:1_level:2_level
[ ] [ERR] root:top_level:1_level:2_level
  |      transitive_data: transitive_value
  |
  |  [Task] 2_level
  |  
  |  
  |  Caused by:
  |      oh noes, this fails
[ ] [ERR] root:top_level:1_level
  |      1_level_data: 9
  |      transitive_data: transitive_value
  |
  |  [Task] 1_level
  |    1_level_data: 9
  |    transitive_data: transitive_value
  |  
  |  
  |  Caused by:
  |      0: [Task] 2_level
  |         
  |      1: oh noes, this fails
[ ] [ERR] root:top_level
  |      top_level_data: 5
  |      transitive_data: transitive_value
  |
  |  [Task] top_level
  |    top_level_data: 5
  |  
  |  
  |  Caused by:
  |      0: [Task] 1_level
  |           1_level_data: 9
  |           transitive_data: transitive_value
  |         
  |      1: [Task] 2_level
  |         
  |      2: oh noes, this fails

"
    );
    Ok(())
}

#[tokio::test]
async fn error_chain_test_hide_errors() -> Result<()> {
    let (tt, s) = setup();

    tt.hide_errors_default_msg(Some(" <error omitted>"));

    let root = tt.create_task("root");
    let result = root.spawn_sync("top_level", |t| {
        t.hide_error_msg(None);
        t.data("top_level_data", 5);

        t.spawn_sync("1_level", |t| {
            t.data("1_level_data", 9);
            t.spawn_sync("2_level", |_| {
                anyhow::ensure!(false, "oh noes, this fails");
                Ok(())
            })
        })?;
        Ok(())
    });

    tt.report_all();
    snapshot!(
        format!("{:?}", result.unwrap_err()),
        "
[Task] top_level
  top_level_data: 5


Caused by:
    0: [Task] 1_level
         1_level_data: 9
       
    1: [Task] 2_level
       
    2: oh noes, this fails
"
    );

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:top_level
[ ] | STARTING | root:top_level:1_level
[ ] | STARTING | root:top_level:1_level:2_level
[ ] [ERR] root:top_level:1_level:2_level <error omitted>
[ ] [ERR] root:top_level:1_level
  |      1_level_data: 9
 <error omitted>
[ ] [ERR] root:top_level
  |      top_level_data: 5
  |
  |  [Task] top_level
  |    top_level_data: 5
  |  
  |  
  |  Caused by:
  |      0: [Task] 1_level
  |           1_level_data: 9
  |         
  |      1: [Task] 2_level
  |         
  |      2: oh noes, this fails

"
    );
    Ok(())
}

#[tokio::test]
async fn error_chain_test_error_formatter() -> Result<()> {
    let (tt, s) = setup();

    tt.hide_errors_default_msg(Some(" <error omitted>"));

    struct CustomFormatter {}

    impl ErrorFormatter for CustomFormatter {
        fn format_error(&self, err: &anyhow::Error) -> String {
            err.chain()
                .rev()
                .enumerate()
                .map(|(i, e)| format!("{} --> {}", i, e.to_string().trim()))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    tt.set_error_formatter(Some(Arc::new(CustomFormatter {})));
    let root = tt.create_task("root");
    let result = root.spawn_sync("top_level", |t| {
        t.hide_error_msg(None);
        t.spawn_sync("random_stuff", |_| Ok(()))?;
        t.data("top_level_data", 5);

        t.spawn_sync("1_level", |t| {
            t.data("1_level_data", 9);
            t.spawn_sync("2_level", |_| {
                anyhow::ensure!(false, "oh noes, this fails");
                Ok(())
            })
        })?;
        Ok(())
    });

    tt.report_all();
    snapshot!(
        format!("{:?}", result.unwrap_err()),
        "
[Task] top_level
  top_level_data: 5


Caused by:
    0: [Task] 1_level
         1_level_data: 9
       
    1: [Task] 2_level
       
    2: oh noes, this fails
"
    );

    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:top_level
[ ] | STARTING | root:top_level:random_stuff
[ ] root:top_level:random_stuff
[ ] | STARTING | root:top_level:1_level
[ ] | STARTING | root:top_level:1_level:2_level
[ ] [ERR] root:top_level:1_level:2_level <error omitted>
[ ] [ERR] root:top_level:1_level
  |      1_level_data: 9
 <error omitted>
[ ] [ERR] root:top_level
  |      top_level_data: 5
  |
  |  0 --> oh noes, this fails
  |  1 --> [Task] 2_level
  |  2 --> [Task] 1_level
  |    1_level_data: 9
  |  3 --> [Task] top_level
  |    top_level_data: 5

"
    );
    Ok(())
}

#[tokio::test]
async fn logger_data_test() -> Result<()> {
    let (tt, s) = setup();
    tt.add_data_transitive("tree_transitive_data", 5);

    let root = tt.create_task("root");

    let t1 = root.create("t1");
    t1.data_transitive("process_id", 123);

    t1.spawn_sync("has_process_id", |_| Ok(()))?;

    let t2 = t1.create("t2");
    t2.data_transitive("request_id", 234);
    t2.spawn_sync("has_process_and_request_id", |_| Ok(()))?;

    let t3 = t2.create("t3");
    t3.data_transitive("request_id #dontprint", 592);
    t3.spawn_sync("wont_print_request_id", |_| Ok(()))?;

    let t4 = t3.create("t4");
    t4.spawn_sync("wont_print_request_id", |task| {
        task.data("hello", "meow");
        snapshot!(
            format!("{:?}", task.get_data("tree_transitive_data")),
            "Some(Int(5))"
        );
        snapshot!(
            format!("{:?}", task.get_data("hello")),
            r#"Some(String("meow"))"#
        );
        snapshot!(
            format!("{:?}", task.get_data("this_data_doesnt_exist")),
            "None"
        );
        snapshot!(
            task.get_data("tree_transitive_data").unwrap().to_string(),
            "5"
        );
        Ok(())
    })?;

    tt.report_all();
    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:t1
[ ] | STARTING | root:t1:has_process_id
[ ] root:t1:has_process_id
  |      process_id: 123
  |      tree_transitive_data: 5
[ ] | STARTING | root:t1:t2
[ ] | STARTING | root:t1:t2:has_process_and_request_id
[ ] root:t1:t2:has_process_and_request_id
  |      process_id: 123
  |      request_id: 234
  |      tree_transitive_data: 5
[ ] | STARTING | root:t1:t2:t3
[ ] | STARTING | root:t1:t2:t3:wont_print_request_id
[ ] root:t1:t2:t3:wont_print_request_id
  |      process_id: 123
  |      tree_transitive_data: 5
[ ] | STARTING | root:t1:t2:t3:t4
[ ] | STARTING | root:t1:t2:t3:t4:wont_print_request_id
[ ] root:t1:t2:t3:t4:wont_print_request_id
  |      hello: meow
  |      process_id: 123
  |      tree_transitive_data: 5

"
    );
    Ok(())
}

#[tokio::test]
async fn async_test() -> Result<()> {
    let (tt, s) = setup();
    let root = tt.create_task("root");

    root.spawn("async_event", |e| async move {
        e.data("async_data", 5);
        let block = async {};
        block.await;
        Ok(())
    })
    .await?;

    tt.report_all();
    snapshot!(
        s.to_string(),
        "
[ ] | STARTING | root
[ ] | STARTING | root:async_event
[ ] root:async_event
  |      async_data: 5

"
    );
    Ok(())
}
