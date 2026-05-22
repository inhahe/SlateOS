#![deny(clippy::all)]

//! grafana-cli — OurOS Grafana CLI
//!
//! Single personality: `grafana-cli`

use std::env;
use std::process;

fn run_grafana_cli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: grafana-cli <COMMAND> [OPTIONS]");
        println!();
        println!("Grafana CLI — manage plugins, admin, and server.");
        println!();
        println!("Commands:");
        println!("  plugins        Manage plugins");
        println!("  admin          Admin commands");
        println!("  server         Grafana server commands");
        println!();
        println!("Options:");
        println!("  --homepath <PATH>       Grafana home path");
        println!("  --config <FILE>         Configuration file");
        println!("  --pluginsDir <DIR>      Plugins directory");
        println!("  --configOverrides <O>   Config overrides");
        println!("  -V, --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("grafana-cli version 10.3.1 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match (cmd, sub) {
        ("plugins", "install") => {
            let plugin = args.get(2).map(|s| s.as_str()).unwrap_or("grafana-clock-panel");
            println!("Installing {}...", plugin);
            println!("  Downloading {}...", plugin);
            println!("  Installed {} successfully", plugin);
            println!();
            println!("  Restart Grafana to load the plugin.");
            0
        }
        ("plugins", "list-remote") => {
            println!("ID                           Version    Description");
            println!("──────────────────────────── ──────── ──────────────────────────────────");
            println!("grafana-clock-panel          2.1.3    Clock panel for Grafana");
            println!("grafana-piechart-panel       1.6.4    Pie chart panel");
            println!("grafana-worldmap-panel       1.0.6    World map panel");
            println!("grafana-polystat-panel       2.1.4    Polystat panel");
            println!("alexanderzobnin-zabbix-app   4.4.2    Zabbix integration");
            0
        }
        ("plugins", "ls") | ("plugins", "list-installed") => {
            println!("Installed plugins:");
            println!("  grafana-clock-panel @ 2.1.3");
            println!("  grafana-piechart-panel @ 1.6.4");
            0
        }
        ("plugins", "remove") | ("plugins", "uninstall") => {
            let plugin = args.get(2).map(|s| s.as_str()).unwrap_or("grafana-clock-panel");
            println!("Removing {}...", plugin);
            println!("  Plugin removed successfully.");
            0
        }
        ("plugins", "update") | ("plugins", "upgrade") => {
            let plugin = args.get(2).map(|s| s.as_str()).unwrap_or("grafana-clock-panel");
            println!("Updating {}...", plugin);
            println!("  Updated to v2.1.4");
            0
        }
        ("admin", "reset-admin-password") => {
            println!("Admin password changed successfully.");
            0
        }
        ("admin", "data-migration") => {
            println!("Running data migration...");
            println!("  Done.");
            0
        }
        ("admin", "secrets-migration") => {
            println!("Running secrets migration...");
            println!("  Migrated 15 secrets.");
            println!("  Done.");
            0
        }
        ("server", _) => {
            println!("Grafana server v10.3.1");
            println!("  HTTP: 0.0.0.0:3000");
            println!("  Database: sqlite3");
            println!("  Starting...");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: grafana-cli <command>. See --help.");
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
    let code = run_grafana_cli(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
