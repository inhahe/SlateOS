#![deny(clippy::all)]

//! grafana — OurOS observability and visualization platform
//!
//! Multi-personality: `grafana-server` (default), `grafana-cli`

use std::env;
use std::process;

fn run_grafana_server(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: grafana-server [options]");
        println!();
        println!("Options:");
        println!("  --config <file>      Configuration file path");
        println!("  --homepath <path>    Path to Grafana install/home");
        println!("  --pidfile <file>     Path to PID file");
        println!("  --packaging <val>    deb, rpm, docker, brew");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Version 10.4.0 (OurOS)");
        println!("Commit: abc1234");
        println!("Branch: main");
        return 0;
    }
    println!("INFO [2025-05-22 10:00:00] Starting Grafana version=10.4.0");
    println!("INFO [2025-05-22 10:00:00] Config loaded path=/etc/grafana/grafana.ini");
    println!("INFO [2025-05-22 10:00:01] HTTP Server Listen addr=0.0.0.0:3000 protocol=http");
    println!("INFO [2025-05-22 10:00:01] Grafana is ready.");
    0
}

fn run_grafana_cli(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: grafana-cli [global options] command [options]");
            println!();
            println!("Commands:");
            println!("  plugins    Manage plugins");
            println!("  admin      Admin commands");
            println!("  --version  Show version");
            0
        }
        "--version" => { println!("Grafana CLI version 10.4.0 (OurOS)"); 0 }
        "plugins" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list-remote");
            match sub {
                "install" => {
                    let plugin = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("plugin");
                    println!("Installing {}...", plugin);
                    println!("Installed {} successfully.", plugin);
                    println!("Restart Grafana after installing plugins.");
                }
                "ls" | "list-remote" => {
                    println!("grafana-clock-panel");
                    println!("grafana-piechart-panel");
                    println!("grafana-worldmap-panel");
                    println!("grafana-image-renderer");
                }
                "uninstall" | "remove" => {
                    let plugin = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("plugin");
                    println!("Removing {}...", plugin);
                    println!("Plugin {} removed.", plugin);
                }
                "update-all" => println!("All plugins up to date (simulated)"),
                _ => println!("plugins {}: (simulated)", sub),
            }
            0
        }
        "admin" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "reset-admin-password" => println!("Admin password has been changed (simulated)"),
                "data-migration" => println!("Running data migration (simulated)"),
                "secret-scan" => println!("No secrets found (simulated)"),
                _ => println!("admin {}: (simulated)", sub),
            }
            0
        }
        other => { eprintln!("grafana-cli: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("grafana-server");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "grafana-cli" => run_grafana_cli(rest),
        _ => run_grafana_server(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_grafana_server};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_grafana_server(vec!["--help".to_string()]), 0);
        assert_eq!(run_grafana_server(vec!["-h".to_string()]), 0);
        assert_eq!(run_grafana_server(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_grafana_server(vec![]), 0);
    }
}
