mod mcp;

use serde_json::{Value, json};
use std::path::PathBuf;
use voldiq_core::{agents::Registry, service::AgentService};

fn success(result: Value) -> Value {
    json!({"schema_version":1,"result":result})
}
fn failure(code: &str, message: &str) -> Value {
    json!({"schema_version":1,"error":{"code":code,"message":message}})
}

#[tokio::main(worker_threads = 2)]
async fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let stdio = args.iter().any(|arg| arg == "mcp");
    let result = run(&mut args).await;
    match result {
        Ok(Some(value)) => println!("{value}"),
        Ok(None) => {}
        Err((code, message, exit)) => {
            let value = failure(code, &message);
            if stdio {
                eprintln!("{value}");
            } else {
                println!("{value}");
            }
            std::process::exit(exit);
        }
    }
}

type CliError = (&'static str, String, i32);

async fn run(args: &mut Vec<String>) -> Result<Option<Value>, CliError> {
    if args.is_empty() || args == &["--help"] {
        return Ok(Some(success(
            json!({"commands":["capabilities","translate <description>","scan <root> [--wait]","query <snapshot_id> [JSON filters]","stats <snapshot_id>","history [offset] [limit]","compare <before_id> <after_id> [offset] [limit]","calendar <snapshot_id> <YYYY-MM> [day] [offset] [limit]","diary <snapshot_id> [offset] [limit]","details <snapshot_id> <node_id>","folder <snapshot_id> <node_id> [--note]","budget <snapshot_id> <node_id>","jobs <job_id>","cancel <job_id>","plan prepare <snapshot_id> <JSON node IDs>","plan get <plan_id>","plan request <plan_id>","plan execute <plan_id> <manifest_hash>","plan cancel <plan_id> <manifest_hash>","plan audit <plan_id> [after_sequence] [limit]","mcp serve --stdio"],"state":"--state <directory> before command (default: Voldiq local application data)","authentication":"Human-provisioned VOLDIQ_AGENT_TOKEN environment variable. No credential arguments or approval bypass."}),
        )));
    }
    let directory = if args.first().is_some_and(|arg| arg == "--state") {
        if args.len() < 3 {
            return Err(arguments());
        }
        let directory = PathBuf::from(args.remove(1));
        args.remove(0);
        directory
    } else {
        default_directory().map_err(|_| operation())?
    };
    let token = std::env::var("VOLDIQ_AGENT_TOKEN").map_err(|_| unauthorized())?;
    let principal = Registry::open(&directory.join("agents.db"))
        .and_then(|mut registry| registry.authenticate(&token))
        .map_err(|_| unauthorized())?;
    let service = AgentService::new(directory, principal).map_err(|_| operation())?;
    if args == &["mcp", "serve", "--stdio"] {
        mcp::serve(service).await.map_err(|_| operation())?;
        return Ok(None);
    }
    let command = args.first().map(String::as_str).ok_or_else(arguments)?;
    let value = match (command, args.len()) {
        ("capabilities", 1) => service.capabilities().map_err(|_| unauthorized())?,
        ("translate", 2) => {
            serde_json::to_value(service.translate_query(&args[1]).map_err(query_operation)?)
                .map_err(|_| operation())?
        }
        ("scan", 2 | 3) => {
            if args.len() == 3 && args[2] != "--wait" {
                return Err(arguments());
            }
            let id = service
                .start_scan(std::path::Path::new(&args[1]))
                .map_err(|_| operation())?;
            eprintln!(
                "{}",
                json!({"schema_version":1,"job_id":id,"state":"running"})
            );
            let waiting = service.clone();
            let mut worker = tokio::task::spawn_blocking(move || waiting.wait(id));
            let job = tokio::select! {
                result = &mut worker => result.map_err(|_| operation())?.map_err(|_| operation())?,
                signal = tokio::signal::ctrl_c() => {
                    signal.map_err(|_| operation())?;
                    service.cancel(id).map_err(|_| operation())?;
                    worker.await.map_err(|_| operation())?.map_err(|_| operation())?
                }
            };
            if job.state == voldiq_core::jobs::JobState::Cancelled {
                return Err((
                    "scan_cancelled",
                    "Scan cancelled; no completed snapshot was published.".into(),
                    130,
                ));
            }
            if job.state != voldiq_core::jobs::JobState::Completed {
                return Err((
                    "scan_failed",
                    "Scan failed or was interrupted. Inspect its job status before retrying."
                        .into(),
                    4,
                ));
            }
            serde_json::to_value(job).map_err(|_| operation())?
        }
        ("query", 2 | 3) => {
            let query = if args.len() == 3 {
                serde_json::from_str(&args[2]).map_err(|_| arguments())?
            } else {
                Default::default()
            };
            serde_json::to_value(
                service
                    .query(number(&args[1])?, &query)
                    .map_err(query_operation)?,
            )
            .map_err(|_| operation())?
        }
        ("history", 1..=3) => {
            let offset = args
                .get(1)
                .map(|value| value.parse::<usize>())
                .transpose()
                .map_err(|_| arguments())?
                .unwrap_or(0);
            let limit = args
                .get(2)
                .map(|value| value.parse::<usize>())
                .transpose()
                .map_err(|_| arguments())?
                .unwrap_or(100);
            serde_json::to_value(service.history(offset, limit).map_err(|_| operation())?)
                .map_err(|_| operation())?
        }
        ("compare", 3..=5) => {
            let before = number(&args[1])?;
            let after = number(&args[2])?;
            let offset = args
                .get(3)
                .map(|value| value.parse::<usize>())
                .transpose()
                .map_err(|_| arguments())?
                .unwrap_or(0);
            let limit = args
                .get(4)
                .map(|value| value.parse::<usize>())
                .transpose()
                .map_err(|_| arguments())?
                .unwrap_or(50);
            if before == after || offset > 10_000 || !(1..=200).contains(&limit) {
                return Err(arguments());
            }
            read_history(
                service.clone(),
                ("comparison_cancelled", "Comparison cancelled."),
                move |service, cancel| {
                    service.compare_snapshots(before, after, offset, limit, cancel)
                },
            )
            .await?
        }
        ("calendar", 3..=6) => {
            use voldiq_core::snapshots::CalendarRequest;
            let snapshot = number(&args[1])?;
            let month = args[2].clone();
            let request = if let Some(day) = args.get(3) {
                let day = day.parse::<u8>().map_err(|_| arguments())?;
                let offset = args
                    .get(4)
                    .map(|value| value.parse::<usize>())
                    .transpose()
                    .map_err(|_| arguments())?
                    .unwrap_or(0);
                let limit = args
                    .get(5)
                    .map(|value| value.parse::<usize>())
                    .transpose()
                    .map_err(|_| arguments())?
                    .unwrap_or(100);
                if !(1..=31).contains(&day) || offset > 10000 || !(1..=100).contains(&limit) {
                    return Err(arguments());
                }
                CalendarRequest::Day {
                    snapshot,
                    month,
                    day,
                    offset,
                    limit,
                }
            } else {
                CalendarRequest::Month { snapshot, month }
            };
            read_history(
                service.clone(),
                ("calendar_cancelled", "Calendar query cancelled."),
                move |service, cancel| service.calendar(request, cancel),
            )
            .await?
        }
        ("diary", 2..=4) => {
            let snapshot = number(&args[1])?;
            let offset = args
                .get(2)
                .map(|value| value.parse::<usize>())
                .transpose()
                .map_err(|_| arguments())?
                .unwrap_or(0);
            let limit = args
                .get(3)
                .map(|value| value.parse::<usize>())
                .transpose()
                .map_err(|_| arguments())?
                .unwrap_or(5);
            if offset > 10000 || !(1..=10).contains(&limit) {
                return Err(arguments());
            }
            read_history(
                service.clone(),
                ("diary_cancelled", "Diary query cancelled."),
                move |service, cancel| service.diary(snapshot, offset, limit, cancel),
            )
            .await?
        }
        ("stats", 2) => {
            serde_json::to_value(service.stats(number(&args[1])?).map_err(|_| operation())?)
                .map_err(|_| operation())?
        }
        ("budget", 3) => serde_json::to_value(
            service
                .folder_budget(
                    number(&args[1])?,
                    voldiq_core::NodeId(args[2].parse().map_err(|_| arguments())?),
                )
                .map_err(|_| operation())?,
        )
        .map_err(|_| operation())?,
        ("folder", 3 | 4) if args.len() == 3 || args[3] == "--note" => serde_json::to_value(
            service
                .folder_metadata(
                    number(&args[1])?,
                    voldiq_core::NodeId(args[2].parse().map_err(|_| arguments())?),
                    args.len() == 4,
                )
                .map_err(|_| operation())?,
        )
        .map_err(|_| operation())?,
        ("details", 3) => serde_json::to_value(
            service
                .details(
                    number(&args[1])?,
                    voldiq_core::NodeId(args[2].parse().map_err(|_| arguments())?),
                )
                .map_err(|_| operation())?,
        )
        .map_err(|_| operation())?,
        ("jobs", 2) => {
            serde_json::to_value(service.job(number(&args[1])?).map_err(|_| operation())?)
                .map_err(|_| operation())?
        }
        ("cancel", 2) => {
            service.cancel(number(&args[1])?).map_err(|_| operation())?;
            json!({"cancel_requested":true})
        }
        ("plan", 4) if args[1] == "prepare" => {
            if args[3].len() > 16_384 {
                return Err(arguments());
            }
            let selection = serde_json::from_str::<Vec<voldiq_core::NodeId>>(&args[3])
                .map_err(|_| arguments())?;
            serde_json::to_value(
                service
                    .plan_prepare(number(&args[2])?, &selection)
                    .map_err(plan_operation)?,
            )
            .map_err(|_| operation())?
        }
        ("plan", 3) if args[1] == "get" => {
            serde_json::to_value(service.plan_get(&args[2]).map_err(plan_operation)?)
                .map_err(|_| operation())?
        }
        ("plan", 4) if args[1] == "execute" => service
            .plan_execute(&args[2], &args[3])
            .map_err(plan_operation)?,
        ("plan", 4) if args[1] == "cancel" => serde_json::to_value(
            service
                .plan_cancel(&args[2], &args[3])
                .map_err(plan_operation)?,
        )
        .map_err(|_| operation())?,
        ("plan", 3) if args[1] == "request" => {
            serde_json::to_value(service.plan_request(&args[2]).map_err(plan_operation)?)
                .map_err(|_| operation())?
        }
        ("plan", 3..=5) if args[1] == "audit" => {
            let after = args
                .get(3)
                .map(|value| value.parse::<i64>())
                .transpose()
                .map_err(|_| arguments())?
                .unwrap_or(0);
            let limit = args
                .get(4)
                .map(|value| value.parse::<usize>())
                .transpose()
                .map_err(|_| arguments())?
                .unwrap_or(100);
            serde_json::to_value(
                service
                    .plan_audit(&args[2], after, limit)
                    .map_err(plan_operation)?,
            )
            .map_err(|_| operation())?
        }
        _ => return Err(arguments()),
    };
    Ok(Some(success(value)))
}

async fn read_history<T: serde::Serialize + Send + 'static>(
    service: AgentService,
    cancelled: (&'static str, &'static str),
    read: impl FnOnce(&AgentService, &std::sync::atomic::AtomicBool) -> voldiq_core::Result<Option<T>>
    + Send
    + 'static,
) -> Result<Value, CliError> {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let mut worker = tokio::task::spawn_blocking(move || read(&service, &worker_cancel));
    let result = tokio::select! {
        result = &mut worker => result,
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|_| operation())?;
            cancel.store(true,std::sync::atomic::Ordering::Relaxed);
            worker.await
        }
    }
    .map_err(|_| operation())?
    .map_err(query_operation)?;
    if cancel.load(std::sync::atomic::Ordering::Relaxed) || result.is_none() {
        return Err((cancelled.0, cancelled.1.into(), 130));
    }
    serde_json::to_value(result).map_err(|_| operation())
}

fn number(value: &str) -> Result<i64, CliError> {
    value
        .parse::<i64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(arguments)
}
fn arguments() -> CliError {
    (
        "invalid_arguments",
        "Invalid command or arguments. Run --help for supported commands.".into(),
        2,
    )
}
fn unauthorized() -> CliError {
    (
        "not_authorized",
        "A valid human-provisioned Voldiq connection is required.".into(),
        3,
    )
}
fn operation() -> CliError {
    ("operation_failed", "Operation unavailable or outside this connection's scope. Check capability and job status.".into(), 4)
}

fn query_operation(error: voldiq_core::Error) -> CliError {
    match error {
        voldiq_core::Error::Query(error) => ("invalid_query", error.to_string(), 2),
        _ => operation(),
    }
}

fn plan_operation(error: voldiq_core::Error) -> CliError {
    match error {
        voldiq_core::Error::NeedsHumanApproval => ("needs_human_approval", "Independent human review in Voldiq is required. Use plan get to inspect status. This request did not start file execution.".into(), 5),
        _ => operation(),
    }
}

fn default_directory() -> voldiq_core::Result<PathBuf> {
    #[cfg(windows)]
    {
        Ok(voldiq_core::windows::local_app_data()?.join("io.voldiq.desktop"))
    }
    #[cfg(not(windows))]
    {
        Err(voldiq_core::Error::Invalid(
            "Use --state <directory> on this platform.".into(),
        ))
    }
}
