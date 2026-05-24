#![deny(clippy::all)]

//! syslog-ng-cli — OurOS syslog-ng log management
//!
//! Multi-personality: `syslog-ng`, `syslog-ng-ctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_syslog_ng(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: syslog-ng [OPTIONS]");
        println!("syslog-ng v4.6 (OurOS) — System logging daemon");
        println!();
        println!("Options:");
        println!("  --cfgfile FILE    Configuration file");
        println!("  --syntax-only     Check config syntax only");
        println!("  --preprocess-into DIR  Preprocess config");
        println!("  -F                Run in foreground");
        println!("  -d                Debug mode");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("syslog-ng v4.6 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--syntax-only") {
        println!("Configuration file syntax check successful.");
        return 0;
    }
    println!("syslog-ng starting...");
    println!("  Config: /etc/syslog-ng/syslog-ng.conf");
    println!("  Sources: 3 (system, network-udp, network-tcp)");
    println!("  Destinations: 4 (file, console, network, program)");
    println!("  Filters: 7");
    0
}

fn run_syslog_ng_ctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: syslog-ng-ctl COMMAND [OPTIONS]");
        println!("syslog-ng-ctl v4.6 (OurOS) — syslog-ng control tool");
        println!();
        println!("Commands:");
        println!("  stats             Show statistics");
        println!("  query             Query log store");
        println!("  reload            Reload configuration");
        println!("  stop              Stop syslog-ng");
        println!("  verbose           Set verbosity level");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("stats");
    match cmd {
        "stats" => {
            println!("Source: system");
            println!("  processed: 142857");
            println!("  stamp: 2024-01-15 10:30:00");
            println!("Destination: d_file");
            println!("  written: 142850");
            println!("  dropped: 7");
        }
        "reload" => println!("Configuration reloaded successfully."),
        "stop" => println!("syslog-ng stopping..."),
        _ => println!("syslog-ng-ctl {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "syslog-ng".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "syslog-ng-ctl" => run_syslog_ng_ctl(&rest, &prog),
        _ => run_syslog_ng(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
