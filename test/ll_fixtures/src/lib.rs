//! Reusable fixture task trees for testing ll reporters and visualizations.
//!
//! Provides a realistic task tree with build, deploy, test, and monitoring
//! phases that exercise all ll features: nesting, progress bars, data,
//! transitive data, tags, sync/async mix, detached tasks.

use anyhow::Result;
use ll::task;
use ll::Task;
use rand::Rng;
use std::time::Duration;

fn sleep_ms(lo: u64, hi: u64) -> Duration {
    Duration::from_millis(rand::rng().random_range(lo..=hi))
}

/// Run the full fixture pipeline under the given root task.
pub async fn run_pipeline(root: &Task) -> Result<()> {
    let (a, b, c, d) = tokio::join!(
        build(root),
        deploy("staging", root),
        test_phase(root),
        monitoring(root),
    );
    a?;
    b?;
    c?;
    d?;
    Ok(())
}

// ── Build phase ──────────────────────────────────────────────────

#[task]
async fn build(task: &Task) -> Result<()> {
    task.data("compiler", "rustc 1.78");

    task.spawn_sync("resolve_deps", |t| {
        std::thread::sleep(sleep_ms(200, 600));
        t.data("crates", 147);
        t.spawn_sync("check_lockfile", |t2| {
            std::thread::sleep(sleep_ms(100, 300));
            t2.data("lockfile", "Cargo.lock");
            Ok(())
        })?;
        Ok(())
    })?;

    task.spawn("compile", |t| async move {
        let total = 48;
        for i in 0..=total {
            t.progress(i, total);
            tokio::time::sleep(sleep_ms(40, 120)).await;
        }
        t.data("artifacts", 48);
        Ok(())
    })
    .await?;

    task.spawn("link", |t| async move {
        t.data("target", "x86_64-unknown-linux-gnu");
        tokio::time::sleep(sleep_ms(500, 1500)).await;
        Ok(())
    })
    .await?;

    Ok(())
}

// ── Deploy phase ─────────────────────────────────────────────────

#[task(data(environment))]
async fn deploy(environment: &str, task: &Task) -> Result<()> {
    task.spawn("provision_hosts", |t| async move {
        let hosts = ["us-east-1a", "us-west-2b", "eu-west-1c", "ap-south-1a"];
        let mut handles = vec![];
        for (i, host) in hosts.iter().enumerate() {
            let t2 = t.clone();
            let h = host.to_string();
            handles.push(tokio::spawn(async move {
                t2.spawn(format!("host_{h}"), |t3| async move {
                    t3.data("region", h.clone());
                    t3.progress(0, 3);
                    tokio::time::sleep(sleep_ms(300, 800)).await;
                    t3.progress(1, 3);
                    tokio::time::sleep(sleep_ms(300, 800)).await;
                    t3.progress(2, 3);

                    t3.spawn_sync("health_check", |hc| {
                        std::thread::sleep(sleep_ms(100, 400));
                        hc.data("status", "healthy");
                        Ok(())
                    })?;

                    t3.progress(3, 3);

                    if i == 2 {
                        t3.spawn("extra_config", |ec| async move {
                            ec.data("reason", "eu compliance");
                            tokio::time::sleep(sleep_ms(800, 1500)).await;
                            Ok(())
                        })
                        .await?;
                    }
                    Ok(())
                })
                .await
            }));
        }
        for h in handles {
            h.await??;
        }
        Ok(())
    })
    .await?;

    task.spawn("rolling_restart", |t| async move {
        for shard in 0..6 {
            t.spawn(format!("shard_{shard}"), |s| async move {
                s.progress(0, 2);
                tokio::time::sleep(sleep_ms(200, 500)).await;
                s.progress(1, 2);
                tokio::time::sleep(sleep_ms(200, 500)).await;
                s.progress(2, 2);
                Ok(())
            })
            .await?;
        }
        Ok(())
    })
    .await?;

    Ok(())
}

// ── Test phase ───────────────────────────────────────────────────

#[task(name = "test #l2")]
async fn test_phase(task: &Task) -> Result<()> {
    task.data_transitive("test_suite", "integration");

    let (unit, integration, e2e) = tokio::join!(
        task.spawn("unit_tests", |t| async move {
            let modules = [
                "parser",
                "lexer",
                "codegen",
                "optimizer",
                "linker",
                "serde",
                "validator",
                "transform",
            ];
            for (i, module) in modules.iter().enumerate() {
                t.spawn_sync(format!("test_{module}"), |m| {
                    std::thread::sleep(sleep_ms(50, 200));
                    m.data("assertions", rand::rng().random_range(3..40));
                    if *module == "optimizer" {
                        m.data("slow #trace", true);
                    }
                    Ok(())
                })?;
                t.progress(i as i64 + 1, modules.len() as i64);
            }
            Ok(())
        }),
        task.spawn("integration_tests", |t| async move {
            t.spawn("api_tests", |api| async move {
                let endpoints = [
                    "GET /users",
                    "POST /users",
                    "DELETE /users/{id}",
                    "PUT /settings",
                    "GET /health",
                ];
                for ep in endpoints {
                    api.spawn(
                        format!("test_{}", ep.replace([' ', '/'], "_")),
                        |req| async move {
                            req.data("endpoint", ep.to_string());
                            tokio::time::sleep(sleep_ms(100, 500)).await;
                            req.data("status", 200);
                            Ok(())
                        },
                    )
                    .await?;
                }
                Ok(())
            })
            .await?;

            t.spawn("db_tests", |db| async move {
                db.data("db", "postgres");
                db.spawn("migrations", |m| async move {
                    m.spawn_sync("up", |_| {
                        std::thread::sleep(sleep_ms(100, 300));
                        Ok(())
                    })?;
                    m.spawn_sync("seed", |s| {
                        std::thread::sleep(sleep_ms(100, 300));
                        s.data("rows", 1500);
                        Ok(())
                    })?;
                    Ok(())
                })
                .await?;

                db.spawn("queries", |q| async move {
                    for i in 0..5 {
                        q.spawn(format!("query_{i}"), |qi| async move {
                            qi.data("rows_scanned", rand::rng().random_range(100..10000));
                            tokio::time::sleep(sleep_ms(100, 400)).await;
                            Ok(())
                        })
                        .await?;
                    }
                    Ok(())
                })
                .await?;

                Ok(())
            })
            .await?;

            Ok(())
        }),
        task.spawn("e2e_tests #l3", |t| async move {
            t.spawn("browser_tests", |b| async move {
                let scenarios = [
                    "login_flow",
                    "checkout_flow",
                    "signup_flow",
                    "search_flow",
                    "settings_flow",
                ];
                for (i, s) in scenarios.iter().enumerate() {
                    b.spawn(s.to_string(), |sc| async move {
                        let steps = rand::rng().random_range(4..10);
                        for step in 0..=steps {
                            sc.progress(step, steps);
                            tokio::time::sleep(sleep_ms(100, 500)).await;
                        }
                        Ok(())
                    })
                    .await?;
                    b.progress(i as i64 + 1, scenarios.len() as i64);
                }
                Ok(())
            })
            .await?;

            t.spawn("load_test", |lt| async move {
                lt.data("rps_target", 5000);
                let total: i64 = 100;
                for i in 0..=total {
                    lt.progress(i, total);
                    tokio::time::sleep(sleep_ms(20, 60)).await;
                }
                lt.data("p99_ms", rand::rng().random_range(15..120));
                Ok(())
            })
            .await?;

            Ok(())
        }),
    );
    unit?;
    integration?;
    e2e?;

    Ok(())
}

// ── Monitoring phase ─────────────────────────────────────────────

#[task]
async fn monitoring(task: &Task) -> Result<()> {
    task.spawn("collect_metrics", |t| async move {
        let cpu = t.create("cpu_usage");
        cpu.data("cores", 16);
        let mem = t.create("memory_usage");
        mem.data("total_gb", 64);
        let disk = t.create("disk_io");
        disk.data("devices", 4);

        for i in 0..8 {
            cpu.data("sample", rand::rng().random_range(10..95));
            mem.data("used_gb", rand::rng().random_range(8..58));
            disk.data("iops", rand::rng().random_range(100..5000));
            tokio::time::sleep(sleep_ms(200, 500)).await;
            t.progress(i + 1, 8);
        }

        drop(cpu);
        drop(mem);
        drop(disk);
        Ok(())
    })
    .await?;

    let t_clone = task.clone();
    tokio::spawn(async move {
        t_clone
            .spawn("alert_watcher #l3", |aw| async move {
                for i in 0..3 {
                    aw.spawn(format!("check_alert_{i}"), |a| async move {
                        tokio::time::sleep(sleep_ms(500, 1500)).await;
                        a.data("triggered", false);
                        Ok(())
                    })
                    .await?;
                }
                Ok(())
            })
            .await
            .ok();
    });

    task.spawn_sync("aggregate_logs", |t| {
        t.data("sources", 12);
        std::thread::sleep(sleep_ms(200, 600));
        t.spawn_sync("parse_logs", |p| {
            p.data("lines", 84_291);
            std::thread::sleep(sleep_ms(300, 700));
            p.spawn_sync("extract_errors", |e| {
                std::thread::sleep(sleep_ms(100, 300));
                e.data("errors_found", rand::rng().random_range(0..15));
                e.spawn_sync("classify_errors", |c| {
                    std::thread::sleep(sleep_ms(100, 200));
                    c.data("categories", 4);
                    Ok(())
                })?;
                Ok(())
            })?;
            Ok(())
        })?;
        Ok(())
    })?;

    Ok(())
}
