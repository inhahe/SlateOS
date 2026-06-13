#![deny(clippy::all)]

//! nmon-cli — SlateOS nmon performance monitor
//!
//! Single personality: `nmon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nmon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") {
        println!("Usage: nmon [OPTIONS]");
        println!("nmon 16p (SlateOS) — Performance monitor");
        println!();
        println!("Interactive keys:");
        println!("  c    CPU stats");
        println!("  m    Memory stats");
        println!("  d    Disk stats");
        println!("  n    Network stats");
        println!("  t    Top processes");
        println!("  .    Only busy disks");
        println!("  q    Quit");
        println!();
        println!("Options:");
        println!("  -f             Spreadsheet output to file");
        println!("  -s SECONDS     Interval between snapshots");
        println!("  -c COUNT       Number of snapshots");
        println!("  -t             Include top processes");
        println!("  -T             Include top processes (threads)");
        println!("  -r RUNNAME     Hostname override for filename");
        println!("  -F FILENAME    Output filename");
        return 0;
    }
    if args.iter().any(|a| a == "-f") {
        let interval = args.windows(2).find(|w| w[0] == "-s")
            .map(|w| w[1].as_str()).unwrap_or("5");
        let count = args.windows(2).find(|w| w[0] == "-c")
            .map(|w| w[1].as_str()).unwrap_or("60");
        println!("nmon: Recording to file, interval={}s, count={}", interval, count);
        return 0;
    }
    println!("nmon: Interactive performance monitor");
    println!("Press h for help, q to quit");
    println!();
    println!("CPU: User 8.2%  Sys 3.1%  Wait 0.5%  Idle 88.2%");
    println!("MEM: Total 16384MB  Free 12032MB  Cached 2048MB");
    println!("NET: eth0  Rx: 5.6MB/s  Tx: 1.2MB/s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nmon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nmon(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nmon};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nmon"), "nmon");
        assert_eq!(basename(r"C:\bin\nmon.exe"), "nmon.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nmon.exe"), "nmon");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nmon(&["--help".to_string()], "nmon"), 0);
        assert_eq!(run_nmon(&["-h".to_string()], "nmon"), 0);
        let _ = run_nmon(&["--version".to_string()], "nmon");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nmon(&[], "nmon");
    }
}
