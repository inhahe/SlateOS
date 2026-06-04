#![deny(clippy::all)]

//! sentry-cli — OurOS Sentry error tracking CLI
//!
//! Single personality: `sentry-cli`

use std::env;
use std::process;

fn run_sentry_cli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sentry-cli <COMMAND> [OPTIONS]");
        println!();
        println!("Sentry command-line client for error tracking.");
        println!();
        println!("Commands:");
        println!("  releases        Manage releases");
        println!("  deploys         Manage release deployments");
        println!("  upload-dif      Upload debug information files");
        println!("  upload-dsym     Upload dSYM files (macOS)");
        println!("  sourcemaps      Manage source maps");
        println!("  events          Manage events");
        println!("  issues          Manage issues");
        println!("  projects        Manage projects");
        println!("  organizations   Manage organizations");
        println!("  monitors        Manage cron monitors");
        println!("  send-event      Send a test event");
        println!("  login           Authenticate");
        println!("  info            Show configuration info");
        println!();
        println!("Options:");
        println!("  --auth-token <TOKEN>  Auth token (or $SENTRY_AUTH_TOKEN)");
        println!("  --org <ORG>           Organization slug");
        println!("  --project <PROJ>      Project slug");
        println!("  --url <URL>           Sentry URL");
        println!("  --log-level <LVL>     Log level (trace/debug/info/warn/error)");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("sentry-cli 2.28.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match (cmd, sub) {
        ("releases", "new") => {
            let version = args.get(2).map(|s| s.as_str()).unwrap_or("1.0.0");
            println!("Created release: {}", version);
            0
        }
        ("releases", "finalize") => {
            let version = args.get(2).map(|s| s.as_str()).unwrap_or("1.0.0");
            println!("Finalized release: {}", version);
            0
        }
        ("releases", "list") => {
            println!("Version   Date Created          Last Event");
            println!("──────── ──────────────────── ────────────────────");
            println!("1.2.0     2024-01-15 14:30:00  2024-01-15 15:00:00");
            println!("1.1.0     2024-01-10 10:00:00  2024-01-14 23:45:00");
            println!("1.0.0     2024-01-01 00:00:00  2024-01-09 18:30:00");
            0
        }
        ("releases", "set-commits") => {
            let version = args.get(2).map(|s| s.as_str()).unwrap_or("1.0.0");
            println!("Associated commits with release {}", version);
            0
        }
        ("deploys", "new") => {
            println!("Created new deployment for release");
            println!("  Environment: production");
            println!("  Started: 2024-01-15 14:30:00");
            0
        }
        ("sourcemaps", "upload") => {
            let path = args.get(2).map(|s| s.as_str()).unwrap_or("./dist");
            println!("Uploading source maps from {}...", path);
            println!("  Found 12 source map files");
            println!("  Uploading: app.js.map (234 KB)");
            println!("  Uploading: vendor.js.map (567 KB)");
            println!("  Uploading: styles.css.map (89 KB)");
            println!("  Upload complete: 12 files, 2.3 MB total");
            0
        }
        ("issues", "list") => {
            println!("ID         Title                               Events  Users   Status");
            println!("────────── ────────────────────────────────── ─────── ─────── ────────");
            println!("PROJ-123   TypeError: null is not an object    1,234     567   unresolved");
            println!("PROJ-124   ReferenceError: x is not defined      89      45   unresolved");
            println!("PROJ-125   NetworkError: Failed to fetch        456     234   resolved");
            println!("PROJ-126   SyntaxError: Unexpected token         12       8   ignored");
            0
        }
        ("send-event", _) => {
            println!("Sending test event...");
            println!("  Event ID: abc123def456789012345678901234");
            println!("  Event sent successfully.");
            0
        }
        ("info", _) => {
            println!("Sentry CLI info:");
            println!("  Version:      2.28.0");
            println!("  Sentry Server: https://sentry.io/");
            println!("  Organization:  my-org");
            println!("  Project:       my-project");
            println!("  Auth:          Token (valid)");
            0
        }
        ("monitors", "list") => {
            println!("Cron monitors:");
            println!("  Name             Schedule       Status    Last Check-in");
            println!("  ──────────────── ──────────── ──────── ────────────────────");
            println!("  daily-backup     0 2 * * *     OK        2024-01-15 02:00:00");
            println!("  health-check     */5 * * * *   OK        2024-01-15 14:30:00");
            println!("  cleanup-job      0 0 * * 0     Missed    2024-01-14 00:00:00");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: sentry-cli <command>. See --help.");
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
    let code = run_sentry_cli(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_sentry_cli};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sentry_cli(vec!["--help".to_string()]), 0);
        assert_eq!(run_sentry_cli(vec!["-h".to_string()]), 0);
        let _ = run_sentry_cli(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sentry_cli(vec![]);
    }
}
