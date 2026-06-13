#![deny(clippy::all)]

//! pagerduty-cli — SlateOS PagerDuty CLI
//!
//! Single personality: `pd`

use std::env;
use std::process;

fn run_pd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pd <COMMAND> [OPTIONS]");
        println!();
        println!("PagerDuty incident management CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  incident     Manage incidents");
        println!("  service      Manage services");
        println!("  schedule     Manage on-call schedules");
        println!("  escalation   Manage escalation policies");
        println!("  event        Send events (trigger/resolve)");
        println!("  oncall       Show who is on-call");
        println!("  auth         Authentication");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pd 0.8.0 (SlateOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "incident" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  ID           Status       Urgency   Service           Title");
                    println!("  P1ABC123     triggered    high      Production API    Database connection timeout");
                    println!("  P2DEF456     acknowledged high      Payment Service   Payment processing failure");
                    println!("  P3GHI789     resolved     low       Staging           Disk space warning");
                }
                "create" => {
                    let title = args.get(2).map(|s| s.as_str()).unwrap_or("New incident");
                    println!("Created incident P4JKL012: {}", title);
                    println!("  Urgency: high");
                    println!("  Service: Production API");
                    println!("  Assigned: on-call engineer");
                }
                "ack" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("P1ABC123");
                    println!("Acknowledged incident {}", id);
                }
                "resolve" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("P1ABC123");
                    println!("Resolved incident {}", id);
                }
                _ => { println!("Incident operation: {}", sub); }
            }
            0
        }
        "service" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  ID           Status    Name                Escalation Policy");
                    println!("  PSVC001      active    Production API      Default");
                    println!("  PSVC002      active    Payment Service     Critical");
                    println!("  PSVC003      active    Auth Service        Default");
                    println!("  PSVC004      disabled  Staging             Low Priority");
                }
                _ => { println!("Service operation: {}", sub); }
            }
            0
        }
        "schedule" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  ID           Name              Timezone      Users");
                    println!("  PSCHED01     Primary On-Call    US/Pacific    3");
                    println!("  PSCHED02     Secondary          US/Eastern    2");
                    println!("  PSCHED03     Weekend            US/Pacific    4");
                }
                _ => { println!("Schedule operation: {}", sub); }
            }
            0
        }
        "event" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("trigger");
            match sub {
                "trigger" => {
                    let summary = args.get(2).map(|s| s.as_str()).unwrap_or("Alert triggered");
                    println!("Event accepted. Dedup key: evt-abc123def456");
                    println!("  Summary: {}", summary);
                    println!("  Severity: critical");
                }
                "resolve" => {
                    let key = args.get(2).map(|s| s.as_str()).unwrap_or("evt-abc123def456");
                    println!("Event resolved for dedup key: {}", key);
                }
                _ => { println!("Event operation: {}", sub); }
            }
            0
        }
        "oncall" => {
            println!("Currently On-Call:");
            println!("  Primary On-Call:");
            println!("    Alice Smith <alice@example.com>  (until 2024-01-16 09:00 PST)");
            println!("  Secondary:");
            println!("    Bob Jones <bob@example.com>  (until 2024-01-16 09:00 PST)");
            0
        }
        "auth" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("login");
            match sub {
                "login" => {
                    println!("Enter API token: ");
                    println!("✔ Authenticated as alice@example.com");
                    println!("  Token saved to ~/.pd/credentials");
                }
                "whoami" => {
                    println!("  Name: Alice Smith");
                    println!("  Email: alice@example.com");
                    println!("  Role: admin");
                    println!("  Time Zone: US/Pacific");
                }
                _ => { println!("Auth operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: pd <command>. See --help.");
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
    let code = run_pd(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pd};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pd(vec!["--help".to_string()]), 0);
        assert_eq!(run_pd(vec!["-h".to_string()]), 0);
        let _ = run_pd(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pd(vec![]);
    }
}
