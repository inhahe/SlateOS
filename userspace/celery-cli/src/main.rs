#![deny(clippy::all)]

//! celery-cli — OurOS Celery CLI
//!
//! Single personality: `celery`

use std::env;
use std::process;

fn run_celery(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: celery <COMMAND> [OPTIONS]");
        println!();
        println!("Celery distributed task queue CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  worker       Start a worker");
        println!("  beat         Start the beat scheduler");
        println!("  inspect      Inspect workers");
        println!("  status       Show worker status");
        println!("  call         Call a task");
        println!("  result       Get task result");
        println!("  purge        Purge all messages");
        println!("  flower       Start Flower monitoring");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("celery 5.3.6 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "worker" => {
            let concurrency = args.windows(2).find(|w| w[0] == "-c" || w[0] == "--concurrency")
                .map(|w| w[1].as_str()).unwrap_or("4");
            let queue = args.windows(2).find(|w| w[0] == "-Q" || w[0] == "--queues")
                .map(|w| w[1].as_str()).unwrap_or("celery");
            println!(" -------------- celery@myhost v5.3.6 (emerald-rush)");
            println!("--- ***** -----");
            println!("-- ******* ---- OurOS x86_64");
            println!("- *** --- * ---");
            println!("- ** ---------- [config]");
            println!("- ** ---------- .> app:         myapp:0x7f1234567890");
            println!("- ** ---------- .> transport:   amqp://guest:**@localhost:5672//");
            println!("- ** ---------- .> results:     redis://localhost:6379/0");
            println!("- *** --- * --- .> concurrency: {} (prefork)", concurrency);
            println!("-- ******* ---- .> task events: OFF");
            println!("--- ***** -----");
            println!(" -------------- [queues]");
            println!("                .> {}        exchange={}(direct) key={}", queue, queue, queue);
            println!();
            println!("[tasks]");
            println!("  . myapp.tasks.send_email");
            println!("  . myapp.tasks.process_order");
            println!("  . myapp.tasks.generate_report");
            0
        }
        "beat" => {
            println!("celery beat v5.3.6 is starting.");
            println!("  __    -    ... __   -        _");
            println!("  LocalTime -> 2024-01-15 14:00:00");
            println!("  Configuration ->");
            println!("    . broker -> amqp://guest:**@localhost:5672//");
            println!("    . loader -> celery.loaders.app.AppLoader");
            println!("    . scheduler -> celery.beat.PersistentScheduler");
            println!("    . db -> celerybeat-schedule");
            0
        }
        "inspect" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("active");
            match sub {
                "active" => {
                    println!("-> celery@myhost: OK");
                    println!("    * myapp.tasks.process_order[abc123-def456] running (1.2s)");
                    println!("    * myapp.tasks.send_email[ghi789-jkl012] running (0.3s)");
                }
                "stats" => {
                    println!("-> celery@myhost: OK");
                    println!("    {{");
                    println!("      \"total\": {{\"myapp.tasks.process_order\": 1234, \"myapp.tasks.send_email\": 5678}}");
                    println!("      \"pool\": {{\"max-concurrency\": 4, \"processes\": [1234, 1235, 1236, 1237]}}");
                    println!("    }}");
                }
                "registered" => {
                    println!("-> celery@myhost: OK");
                    println!("    * myapp.tasks.send_email");
                    println!("    * myapp.tasks.process_order");
                    println!("    * myapp.tasks.generate_report");
                }
                _ => { println!("Inspect: {}", sub); }
            }
            0
        }
        "status" => {
            println!("celery@myhost: OK  (4 tasks, uptime: 3d 14h 22m)");
            println!();
            println!("1 node online.");
            0
        }
        "call" => {
            let task = args.get(1).map(|s| s.as_str()).unwrap_or("myapp.tasks.process_order");
            println!("Calling task: {}", task);
            println!("  Task ID: abc123-def456-ghi789-jkl012");
            0
        }
        "result" => {
            let task_id = args.get(1).map(|s| s.as_str()).unwrap_or("abc123-def456-ghi789-jkl012");
            println!("Task {}: SUCCESS", task_id);
            println!("  Result: {{\"status\": \"completed\", \"processed\": 42}}");
            0
        }
        "purge" => {
            println!("Purging all messages from all known task queues...");
            println!("  Purged 23 messages from queue 'celery'.");
            0
        }
        "flower" => {
            let port = args.windows(2).find(|w| w[0] == "--port")
                .map(|w| w[1].as_str()).unwrap_or("5555");
            println!("Flower monitoring at http://localhost:{}", port);
            println!("  Broker: amqp://guest:**@localhost:5672//");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: celery <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_celery(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_celery};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_celery(vec!["--help".to_string()]), 0);
        assert_eq!(run_celery(vec!["-h".to_string()]), 0);
        let _ = run_celery(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_celery(vec![]);
    }
}
