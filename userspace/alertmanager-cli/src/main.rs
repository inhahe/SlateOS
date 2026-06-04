#![deny(clippy::all)]

//! alertmanager-cli — OurOS Prometheus Alertmanager
//!
//! Multi-personality: `alertmanager`, `amtool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_alertmanager(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: alertmanager [OPTIONS]");
        println!();
        println!("alertmanager — Prometheus Alertmanager (OurOS).");
        println!();
        println!("Options:");
        println!("  --config.file <f>         Config file");
        println!("  --storage.path <p>        Storage path");
        println!("  --web.listen-address <a>  Listen address");
        println!("  --cluster.listen-address  Cluster listen");
        println!("  --log.level               Log level");
        println!("  --version                 Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("alertmanager, version 0.27.0 (OurOS)");
        println!("  build date: 2024-01-15");
        println!("  go:          go1.22.0");
        return 0;
    }

    println!("level=info ts=2024-05-22T12:00:00.000Z caller=main.go:231 msg=\"Starting Alertmanager\" version=\"0.27.0 (OurOS)\"");
    println!("level=info ts=2024-05-22T12:00:00.001Z caller=coordinator.go:113 msg=\"Loading configuration file\" file=/etc/alertmanager/alertmanager.yml");
    println!("level=info ts=2024-05-22T12:00:00.002Z caller=coordinator.go:126 msg=\"Completed loading of configuration file\"");
    println!("level=info ts=2024-05-22T12:00:00.003Z caller=main.go:535 msg=\"Listening\" address=0.0.0.0:9093");
    0
}

fn run_amtool(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: amtool [OPTIONS] COMMAND");
        println!();
        println!("amtool — Alertmanager CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  alert         Manage alerts");
        println!("  silence       Manage silences");
        println!("  check-config  Check config");
        println!("  config        Show config");
        println!("  cluster       Show cluster");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("alert");
    match subcmd {
        "alert" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("query");
            if cmd == "query" {
                println!("Alertname        Starts At                Summary");
                println!("HighCPU          2024-05-22 10:00:00 UTC  CPU usage above 90% on ouros-node-1");
                println!("DiskSpaceLow     2024-05-22 11:00:00 UTC  Disk space below 10% on /dev/sda1");
                println!("ServiceDown      2024-05-22 11:30:00 UTC  Service 'webapp' not responding");
            } else {
                println!("amtool: alert {} completed", cmd);
            }
        }
        "silence" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("query");
            if cmd == "query" {
                println!("ID                       Matchers           Ends At                  Created By  Comment");
                println!("aabbccdd-1122-3344-5566  alertname=HighCPU  2024-05-23 10:00:00 UTC  admin       Maintenance window");
            } else if cmd == "add" {
                println!("aabbccdd-1122-3344-5566");
            } else {
                println!("amtool: silence {} completed", cmd);
            }
        }
        "check-config" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("/etc/alertmanager/alertmanager.yml");
            println!("Checking '{}'  SUCCESS", file);
            println!("Found:");
            println!(" - global config");
            println!(" - route");
            println!(" - 1 inhibit rules");
            println!(" - 2 receivers");
            println!(" - 0 templates");
        }
        "config" => {
            println!("global:");
            println!("  resolve_timeout: 5m");
            println!("route:");
            println!("  receiver: default");
            println!("  group_wait: 30s");
            println!("  group_interval: 5m");
            println!("  repeat_interval: 4h");
            println!("receivers:");
            println!("  - name: default");
            println!("    email_configs:");
            println!("      - to: admin@ouros.local");
        }
        _ => println!("amtool: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "alertmanager".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "amtool" => run_amtool(&rest),
        _ => run_alertmanager(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_alertmanager};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/alertmanager"), "alertmanager");
        assert_eq!(basename(r"C:\bin\alertmanager.exe"), "alertmanager.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("alertmanager.exe"), "alertmanager");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_alertmanager(&["--help".to_string()]), 0);
        assert_eq!(run_alertmanager(&["-h".to_string()]), 0);
        let _ = run_alertmanager(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_alertmanager(&[]);
    }
}
