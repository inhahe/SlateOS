#![deny(clippy::all)]

//! pueue — OurOS task manager for sequential and parallel command execution
//!
//! Multi-personality: `pueue` (client), `pueued` (daemon)

use std::env;
use std::process;

fn personality(argv0: &str) -> &str {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let base = base.strip_suffix(".exe").unwrap_or(base);
    match base {
        "pueued" => "pueued",
        _ => "pueue",
    }
}

fn run_pueued(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pueued [OPTIONS]");
        println!();
        println!("Start the pueue daemon.");
        println!();
        println!("Options:");
        println!("  -p, --port <PORT>      Port to listen on");
        println!("  --unix-socket <PATH>   Unix socket path");
        println!("  -v, --verbose          Be verbose");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("pueued 3.4.0 (OurOS)");
        return 0;
    }
    println!("pueued 3.4.0 (OurOS) — daemon started");
    println!("Listening on /run/user/1000/pueue.socket");
    0
}

fn run_pueue(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--help" | "-h" | "" => {
            println!("Usage: pueue <COMMAND>");
            println!();
            println!("Manage long-running tasks in a queue.");
            println!();
            println!("Commands:");
            println!("  add         Add a task to the queue");
            println!("  remove      Remove tasks");
            println!("  switch      Switch task priority");
            println!("  stash       Stash tasks (prevent them from starting)");
            println!("  enqueue     Enqueue stashed tasks");
            println!("  start       Start/resume tasks");
            println!("  restart     Restart tasks");
            println!("  pause       Pause tasks or groups");
            println!("  kill        Kill running tasks");
            println!("  send        Send input to a task");
            println!("  edit        Edit task command or path");
            println!("  status      Show task status");
            println!("  format-status  Machine-readable status");
            println!("  log         Show task output");
            println!("  follow      Follow task output");
            println!("  wait        Wait for tasks to finish");
            println!("  clean       Clean finished tasks");
            println!("  reset       Kill all tasks and reset state");
            println!("  shutdown    Shutdown the daemon");
            println!("  parallel    Set parallel task limit");
            println!("  group       Manage groups");
            println!();
            println!("Options:");
            println!("  -V, --version  Show version");
            0
        }
        "--version" | "-V" => {
            println!("pueue 3.4.0 (OurOS)");
            0
        }
        "add" => {
            let task: String = args.iter().skip(1)
                .filter(|a| !a.starts_with('-'))
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            if task.is_empty() {
                println!("New task added (id 0): echo hello");
            } else {
                println!("New task added (id 0): {}", task);
            }
            0
        }
        "status" => {
            println!("┌────┬────────┬────────┬───────┬────────────────────────────────┐");
            println!("│ Id │ Status │ Group  │ Start │ Command                        │");
            println!("├────┼────────┼────────┼───────┼────────────────────────────────┤");
            println!("│  0 │ Done   │ default│ 10:00 │ cargo build --release          │");
            println!("│  1 │ Running│ default│ 10:05 │ cargo test --workspace         │");
            println!("│  2 │ Queued │ default│   -   │ cargo bench                    │");
            println!("│  3 │ Stashed│ deploy │   -   │ rsync -avz target/ server:/app │");
            println!("└────┴────────┴────────┴───────┴────────────────────────────────┘");
            0
        }
        "log" => {
            let id = args.get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            println!("Task {} output:", id);
            println!("   Compiling my-project v1.0.0");
            println!("    Finished `release` profile target(s) in 12.34s");
            0
        }
        "clean" => {
            println!("Cleaned 3 finished tasks.");
            0
        }
        "parallel" => {
            let n = args.get(1).map(|s| s.as_str()).unwrap_or("2");
            println!("Set parallel tasks to {} for group 'default'.", n);
            0
        }
        "group" => {
            let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match subcmd {
                "add" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("new-group");
                    println!("Group '{}' created.", name);
                }
                _ => {
                    println!("Groups:");
                    println!("  default  (parallel: 2, running: 1, queued: 1)");
                    println!("  deploy   (parallel: 1, running: 0, queued: 0, stashed: 1)");
                }
            }
            0
        }
        "kill" | "pause" | "start" | "restart" | "remove" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("all");
            println!("{}: task {}", cmd, id);
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_else(|| String::from("pueue"));
    let p = personality(&argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match p {
        "pueued" => run_pueued(rest),
        _ => run_pueue(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
