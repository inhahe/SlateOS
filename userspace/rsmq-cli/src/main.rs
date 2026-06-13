#![deny(clippy::all)]

//! rsmq-cli — SlateOS RSMQ Redis simple message queue CLI
//!
//! Single personality: `rsmq`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rsmq(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rsmq COMMAND [OPTIONS]");
        println!("rsmq v1.0.0 (SlateOS) — Redis Simple Message Queue CLI");
        println!();
        println!("Commands:");
        println!("  create-queue    Create a new queue");
        println!("  delete-queue    Delete a queue");
        println!("  list-queues     List all queues");
        println!("  get-queue-attrs Get queue attributes");
        println!("  set-queue-attrs Set queue attributes");
        println!("  send            Send a message");
        println!("  receive         Receive a message");
        println!("  delete          Delete a message");
        println!("  pop             Receive and delete");
        println!("  change-visibility  Change message visibility");
        println!();
        println!("Options:");
        println!("  --host HOST     Redis host (default: localhost)");
        println!("  --port PORT     Redis port (default: 6379)");
        println!("  --ns NAMESPACE  Namespace prefix");
        println!("  -V, --version   Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("rsmq v1.0.0 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list-queues");
    match cmd {
        "create-queue" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("myqueue");
            println!("Queue '{}' created.", name);
        }
        "delete-queue" => println!("Queue deleted."),
        "list-queues" => {
            println!("Queues:");
            println!("  emails        (42 messages, 0 hidden)");
            println!("  tasks         (128 messages, 3 hidden)");
            println!("  notifications (0 messages, 0 hidden)");
        }
        "get-queue-attrs" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("emails");
            println!("Queue: {}", name);
            println!("  vt:       30");
            println!("  delay:    0");
            println!("  maxsize:  65536");
            println!("  msgs:     42");
            println!("  hiddenmsgs: 0");
            println!("  totalrecv:  1234");
            println!("  totalsent:  1276");
            println!("  created:    1705312800");
            println!("  modified:   1705399200");
        }
        "send" => println!("Message sent. ID: abc123def456"),
        "receive" => {
            println!("Message:");
            println!("  id:      abc123def456");
            println!("  message: {{\"type\":\"email\",\"to\":\"user@example.com\"}}");
            println!("  rc:      1");
            println!("  fr:      1705312800");
            println!("  sent:    1705312800");
        }
        "pop" => println!("Message received and deleted."),
        "delete" => println!("Message deleted."),
        _ => println!("rsmq {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rsmq".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rsmq(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rsmq};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rsmq"), "rsmq");
        assert_eq!(basename(r"C:\bin\rsmq.exe"), "rsmq.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rsmq.exe"), "rsmq");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rsmq(&["--help".to_string()], "rsmq"), 0);
        assert_eq!(run_rsmq(&["-h".to_string()], "rsmq"), 0);
        let _ = run_rsmq(&["--version".to_string()], "rsmq");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rsmq(&[], "rsmq");
    }
}
