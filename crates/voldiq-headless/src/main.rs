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
            json!({"commands":["capabilities","scan <root> [--wait]","query <snapshot_id> [JSON filters]","stats <snapshot_id>","history [offset] [limit]","details <snapshot_id> <node_id>","jobs <job_id>","cancel <job_id>","mcp serve --stdio"],"state":"--state <directory> before command (default: Voldiq local application data)","authentication":"Human-provisioned VOLDIQ_AGENT_TOKEN environment variable. No credential arguments or approval bypass."}),
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
    let service = AgentService::new(directory, principal);
    if args == &["mcp", "serve", "--stdio"] {
        mcp::serve(service).await.map_err(|_| operation())?;
        return Ok(None);
    }
    let command = args.first().map(String::as_str).ok_or_else(arguments)?;
    let value = match (command, args.len()) {
        ("capabilities", 1) => service.capabilities().map_err(|_| unauthorized())?,
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
                    .map_err(|_| operation())?,
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
        ("stats", 2) => {
            serde_json::to_value(service.stats(number(&args[1])?).map_err(|_| operation())?)
                .map_err(|_| operation())?
        }
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
        _ => return Err(arguments()),
    };
    Ok(Some(success(value)))
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
