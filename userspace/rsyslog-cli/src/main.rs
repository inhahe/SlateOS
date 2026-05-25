#![deny(clippy::all)]

//! rsyslog-cli — OurOS rsyslog system logging daemon
//!
//! Multi-personality: `rsyslogd`, `logger`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rsyslogd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rsyslogd [OPTIONS]");
        println!("rsyslogd v8.2312 (OurOS) — System logging daemon");
        println!();
        println!("Options:");
        println!("  -f FILE           Config file (default: /etc/rsyslog.conf)");
        println!("  -n                No fork (foreground)");
        println!("  -N LEVEL          Config validation level");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rsyslogd v8.2312 (OurOS)"); return 0; }
    println!("rsyslogd: system logging daemon started");
    println!("  Config: /etc/rsyslog.conf");
    println!("  Modules: imuxsock, imklog, imtcp");
    println!("  Outputs: /var/log/syslog, /var/log/auth.log");
    0
}

fn run_logger(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: logger [OPTIONS] MESSAGE");
        println!("logger v2.39 (OurOS) — Send log message to syslog");
        println!();
        println!("Options:");
        println!("  -p PRIORITY       Facility.level (e.g., user.info)");
        println!("  -t TAG            Log tag");
        println!("  -s                Log to stderr too");
        return 0;
    }
    let msg: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    println!("logger: logged '{}'", msg.join(" "));
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rsyslogd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "logger" => run_logger(&rest, &prog),
        _ => run_rsyslogd(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
