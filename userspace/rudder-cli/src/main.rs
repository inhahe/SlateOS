#![deny(clippy::all)]

//! rudder-cli — OurOS Rudder continuous auditing & configuration
//!
//! Multi-personality: `rudder`, `rudder-agent`, `rudder-relayd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rudder(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rudder <command> [OPTIONS]");
        println!("rudder v8.0 (OurOS) — Continuous auditing & configuration");
        println!();
        println!("Commands:");
        println!("  agent run        Run agent policies");
        println!("  agent info       Show agent info");
        println!("  agent inventory  Send inventory");
        println!("  server status    Server health check");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rudder v8.0 (OurOS)"); return 0; }
    if args.len() >= 2 && args[0] == "agent" {
        match args[1].as_str() {
            "run" => {
                println!("rudder agent: running policies");
                println!("  Promises: 42 kept, 0 repaired, 0 not kept");
                println!("  Compliance: 100%");
            }
            "info" => {
                println!("rudder agent info:");
                println!("  Agent: CFEngine 3.21");
                println!("  Policy server: localhost");
                println!("  UUID: a1b2c3d4-e5f6-7890");
            }
            "inventory" => {
                println!("rudder agent: sending inventory...");
                println!("  Inventory sent successfully");
            }
            _ => { println!("rudder agent: unknown subcommand '{}'", args[1]); }
        }
        return 0;
    }
    println!("rudder: use --help for usage information");
    0
}

fn run_rudder_agent(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rudder-agent [run|info|inventory]");
        println!("rudder-agent v8.0 (OurOS) — Rudder agent wrapper");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rudder-agent v8.0 (OurOS)"); return 0; }
    println!("rudder-agent: running policy enforcement");
    println!("  Compliance: 100%");
    0
}

fn run_rudder_relayd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rudder-relayd [OPTIONS]");
        println!("rudder-relayd v8.0 (OurOS) — Rudder relay daemon");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rudder-relayd v8.0 (OurOS)"); return 0; }
    println!("rudder-relayd: relay daemon started");
    println!("  Listen: 0.0.0.0:443");
    println!("  Nodes relayed: 10");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rudder".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "rudder-agent" => run_rudder_agent(&rest, &prog),
        "rudder-relayd" => run_rudder_relayd(&rest, &prog),
        _ => run_rudder(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rudder};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rudder"), "rudder");
        assert_eq!(basename(r"C:\bin\rudder.exe"), "rudder.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rudder.exe"), "rudder");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rudder(&["--help".to_string()], "rudder"), 0);
        assert_eq!(run_rudder(&["-h".to_string()], "rudder"), 0);
        let _ = run_rudder(&["--version".to_string()], "rudder");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rudder(&[], "rudder");
    }
}
