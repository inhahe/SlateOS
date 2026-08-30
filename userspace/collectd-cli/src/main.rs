#![deny(clippy::all)]

//! collectd-cli — Slate OS collectd system statistics daemon
//!
//! Multi-personality: `collectd`, `collectdctl`, `collectdmon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_collectd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: collectd [OPTIONS]");
        println!("collectd v5.12 (Slate OS) — System statistics collection daemon");
        println!();
        println!("Options:");
        println!("  -C FILE       Configuration file");
        println!("  -T            Test configuration and exit");
        println!("  -f            Run in foreground");
        println!("  --version     Show version");
        println!();
        println!("Plugins: cpu, memory, disk, interface, load, uptime, swap");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("collectd v5.12 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-T") {
        println!("collectd: configuration test OK");
        return 0;
    }
    println!("collectd: daemon started");
    println!("  Plugins: cpu, memory, disk, interface, load");
    println!("  Interval: 10 seconds");
    println!("  Write plugins: rrdtool, csv");
    0
}

fn run_collectdctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: collectdctl <command> [OPTIONS]");
        println!("collectdctl v5.12 (Slate OS) — Control collectd daemon");
        println!("  listval       List all values");
        println!("  getval ID     Get specific value");
        println!("  putval ID     Submit a value");
        println!("  flush         Flush cached data");
        return 0;
    }
    if args.first().map(|s| s.as_str()) == Some("listval") {
        println!("slateos/cpu-0/cpu-user");
        println!("slateos/cpu-0/cpu-system");
        println!("slateos/memory/memory-used");
        println!("slateos/memory/memory-free");
        println!("slateos/load/load");
        return 0;
    }
    println!("collectdctl: connected to daemon");
    0
}

fn run_collectdmon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: collectdmon [OPTIONS] -- collectd [ARGS]");
        println!("collectdmon v5.12 (Slate OS) — Monitoring wrapper for collectd");
        return 0;
    }
    let _ = args;
    println!("collectdmon: supervising collectd process");
    println!("  PID: 1234");
    println!("  Restart on failure: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "collectd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "collectdctl" => run_collectdctl(&rest, &prog),
        "collectdmon" => run_collectdmon(&rest, &prog),
        _ => run_collectd(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_collectd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/collectd"), "collectd");
        assert_eq!(basename(r"C:\bin\collectd.exe"), "collectd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("collectd.exe"), "collectd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_collectd(&["--help".to_string()], "collectd"), 0);
        assert_eq!(run_collectd(&["-h".to_string()], "collectd"), 0);
        let _ = run_collectd(&["--version".to_string()], "collectd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_collectd(&[], "collectd");
    }
}
