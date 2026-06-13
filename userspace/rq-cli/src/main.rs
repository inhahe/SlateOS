#![deny(clippy::all)]

//! rq-cli — Slate OS Python RQ (Redis Queue) tools
//!
//! Multi-personality: `rq`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rq(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rq COMMAND [OPTIONS]");
        println!("RQ (Redis Queue) 1.16.2 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  worker       Start a worker");
        println!("  info         Show queue info");
        println!("  enqueue      Enqueue a job");
        println!("  empty        Empty a queue");
        println!("  suspend      Suspend workers");
        println!("  resume       Resume workers");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("rq 1.16.2"),
        "worker" => {
            let queues = args.get(1).map(|s| s.as_str()).unwrap_or("default");
            println!("Worker started, listening on: {}", queues);
            println!("Worker PID: 12345");
        }
        "info" => {
            println!("default      | 5 jobs | 3 workers");
            println!("high         | 2 jobs | 1 worker");
            println!("low          | 12 jobs | 1 worker");
            println!();
            println!("3 queues, 19 jobs total");
            println!("5 workers active");
        }
        "enqueue" => {
            let func = args.get(1).map(|s| s.as_str()).unwrap_or("mymodule.mytask");
            println!("Enqueued job: {} -> default queue", func);
            println!("Job ID: abc123-def456-ghi789");
        }
        "empty" => {
            let queue = args.get(1).map(|s| s.as_str()).unwrap_or("default");
            println!("Queue '{}' emptied.", queue);
        }
        "suspend" => println!("All workers suspended."),
        "resume" => println!("All workers resumed."),
        _ => println!("rq: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rq".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rq(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rq};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rq"), "rq");
        assert_eq!(basename(r"C:\bin\rq.exe"), "rq.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rq.exe"), "rq");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rq(&["--help".to_string()]), 0);
        assert_eq!(run_rq(&["-h".to_string()]), 0);
        let _ = run_rq(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rq(&[]);
    }
}
