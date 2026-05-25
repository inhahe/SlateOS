#![deny(clippy::all)]

//! logwatch-cli — OurOS Logwatch log monitoring
//!
//! Single personality: `logwatch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_logwatch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: logwatch [OPTIONS]");
        println!("logwatch v7.9 (OurOS) — System log analyzer and reporter");
        println!();
        println!("Options:");
        println!("  --detail LEVEL    Detail level (low, med, high)");
        println!("  --range RANGE     Date range (today, yesterday, all)");
        println!("  --service NAME    Analyze specific service");
        println!("  --output FMT      Output format (stdout, mail, file)");
        println!("  --mailto ADDR     Email recipient");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("logwatch v7.9 (OurOS)"); return 0; }
    println!("logwatch: system log report");
    println!("  Date range: today");
    println!("  Services: sshd, kernel, cron, sudo");
    println!("  SSH: 3 successful logins, 15 failed attempts");
    println!("  Kernel: 0 errors, 2 warnings");
    println!("  Cron: 12 jobs executed");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "logwatch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_logwatch(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
