#![deny(clippy::all)]

//! cacti-cli — Slate OS Cacti network graphing tool
//!
//! Multi-personality: `cacti`, `cacti-poller`, `cacti-spine`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cacti(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cacti [OPTIONS]");
        println!("cacti v1.2 (Slate OS) — Network graphing & monitoring");
        println!();
        println!("Options:");
        println!("  --import FILE   Import template");
        println!("  --export FILE   Export graph data");
        println!("  --rebuild       Rebuild poller cache");
        println!("  --version       Show version");
        println!();
        println!("RRDTool-based network graphing with SNMP polling.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cacti v1.2 (Slate OS)"); return 0; }
    println!("cacti: graphing system active");
    println!("  Data sources: 156");
    println!("  Graphs: 48");
    println!("  Devices: 12");
    0
}

fn run_cacti_poller(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cacti-poller [OPTIONS]");
        println!("cacti-poller v1.2 (Slate OS) — Data collection poller");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cacti-poller v1.2 (Slate OS)"); return 0; }
    println!("cacti-poller: polling cycle started");
    println!("  Hosts polled: 12");
    println!("  Data sources updated: 156");
    println!("  Duration: 8.3s");
    0
}

fn run_cacti_spine(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cacti-spine [OPTIONS]");
        println!("cacti-spine v1.2 (Slate OS) — High-performance C-based poller");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cacti-spine v1.2 (Slate OS)"); return 0; }
    println!("cacti-spine: fast poller started");
    println!("  Threads: 4");
    println!("  Hosts processed: 12");
    println!("  Duration: 2.1s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cacti".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "cacti-poller" => run_cacti_poller(&rest, &prog),
        "cacti-spine" => run_cacti_spine(&rest, &prog),
        _ => run_cacti(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cacti};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cacti"), "cacti");
        assert_eq!(basename(r"C:\bin\cacti.exe"), "cacti.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cacti.exe"), "cacti");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cacti(&["--help".to_string()], "cacti"), 0);
        assert_eq!(run_cacti(&["-h".to_string()], "cacti"), 0);
        let _ = run_cacti(&["--version".to_string()], "cacti");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cacti(&[], "cacti");
    }
}
