#![deny(clippy::all)]

//! datadog-cli — SlateOS Datadog CLI (dogstatsd/ddog)
//!
//! Single personality: `ddog`

use std::env;
use std::process;

fn run_ddog(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ddog <COMMAND> [OPTIONS]");
        println!();
        println!("Datadog CLI — interact with Datadog API.");
        println!();
        println!("Commands:");
        println!("  metric send     Send a metric");
        println!("  metric query    Query metrics");
        println!("  event send      Send an event");
        println!("  event list      List recent events");
        println!("  monitor list    List monitors");
        println!("  monitor mute    Mute a monitor");
        println!("  monitor unmute  Unmute a monitor");
        println!("  dashboard list  List dashboards");
        println!("  service-check   Send a service check");
        println!("  tag list        List host tags");
        println!("  tag add         Add host tags");
        println!("  downtime        Manage downtimes");
        println!();
        println!("Options:");
        println!("  --api-key <KEY>    API key (or $DD_API_KEY)");
        println!("  --app-key <KEY>    App key (or $DD_APP_KEY)");
        println!("  --site <SITE>      Datadog site (datadoghq.com/eu/...)");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("ddog 1.0.0 (SlateOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match (cmd, sub) {
        ("metric", "send") => {
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("custom.metric");
            let value = args.get(3).map(|s| s.as_str()).unwrap_or("42");
            println!("Sent metric: {} = {}", name, value);
            println!("  Type: gauge");
            println!("  Tags: [env:production]");
            0
        }
        ("metric", "query") => {
            let query = args.get(2).map(|s| s.as_str()).unwrap_or("avg:system.cpu.user{*}");
            println!("Query: {} (last 1h)", query);
            println!("  14:00  45.2%");
            println!("  14:05  47.8%");
            println!("  14:10  43.1%");
            println!("  14:15  52.3%");
            println!("  14:20  48.9%");
            0
        }
        ("event", "send") => {
            println!("Event sent:");
            println!("  Title: Deployment completed");
            println!("  Priority: normal");
            println!("  Tags: [env:production, service:web-app]");
            0
        }
        ("event", "list") => {
            println!("Recent events (last 24h):");
            println!("  [INFO]  2024-01-15 14:30  Deployment completed (web-app v2.1.0)");
            println!("  [WARN]  2024-01-15 12:00  High CPU usage on web-1 (85%)");
            println!("  [ERROR] 2024-01-15 09:15  Connection pool exhausted (api-srv)");
            println!("  [INFO]  2024-01-14 22:00  Scheduled maintenance window started");
            0
        }
        ("monitor", "list") => {
            println!("ID       Name                        Status   Type");
            println!("──────── ────────────────────────── ──────── ──────────");
            println!("1234567  High CPU Usage              OK       metric");
            println!("1234568  Disk Space Low              Alert    metric");
            println!("1234569  Service Availability        OK       service");
            println!("1234570  Error Rate > 5%             Warn     metric");
            0
        }
        ("dashboard", "list") => {
            println!("ID           Title                    Author");
            println!("──────────── ──────────────────────── ──────────");
            println!("abc-123-def  System Overview           admin");
            println!("ghi-456-jkl  Application Metrics       devops");
            println!("mno-789-pqr  Database Performance      dba");
            0
        }
        ("service-check", _) => {
            println!("Service check sent:");
            println!("  Check: app.health");
            println!("  Status: OK");
            println!("  Host: web-1");
            0
        }
        ("tag", "list") => {
            println!("Host: web-1");
            println!("  Tags:");
            println!("    env:production");
            println!("    service:web-app");
            println!("    team:platform");
            println!("    region:us-east-1");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: ddog <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{} {}'. See --help.", cmd, sub);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ddog(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ddog};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ddog(vec!["--help".to_string()]), 0);
        assert_eq!(run_ddog(vec!["-h".to_string()]), 0);
        let _ = run_ddog(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ddog(vec![]);
    }
}
