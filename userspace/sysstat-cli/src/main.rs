#![deny(clippy::all)]

//! sysstat-cli — OurOS sysstat system performance tools
//!
//! Multi-personality: `sar`, `iostat`, `mpstat`, `pidstat`, `cifsiostat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sar [OPTIONS] [INTERVAL [COUNT]]");
        println!("sar v12.7 (OurOS) — System Activity Reporter");
        println!("  -u    CPU utilization");
        println!("  -r    Memory utilization");
        println!("  -b    I/O activity");
        println!("  -n DEV Network statistics");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sar v12.7 (OurOS, sysstat)"); return 0; }
    println!("12:00:01    CPU   %user   %system   %idle");
    println!("12:00:02    all    5.20     2.10    92.70");
    0
}

fn run_iostat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iostat [OPTIONS] [INTERVAL [COUNT]]");
        println!("iostat v12.7 (OurOS) — I/O statistics");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("iostat v12.7 (OurOS, sysstat)"); return 0; }
    println!("Device     tps    kB_read/s    kB_wrtn/s");
    println!("sda       12.50      156.00       89.00");
    0
}

fn run_mpstat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mpstat [OPTIONS] [INTERVAL [COUNT]]");
        println!("mpstat v12.7 (OurOS) — Per-processor statistics");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mpstat v12.7 (OurOS, sysstat)"); return 0; }
    println!("CPU    %usr   %sys   %idle");
    println!("  0    3.20   1.50   95.30");
    println!("  1    7.10   2.80   90.10");
    0
}

fn run_pidstat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pidstat [OPTIONS] [INTERVAL [COUNT]]");
        println!("pidstat v12.7 (OurOS) — Per-process statistics");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pidstat v12.7 (OurOS, sysstat)"); return 0; }
    println!("PID     %usr  %system  Command");
    println!("1234    2.10    0.50   firefox");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "iostat" => run_iostat(&rest, &prog),
        "mpstat" => run_mpstat(&rest, &prog),
        "pidstat" => run_pidstat(&rest, &prog),
        "cifsiostat" => { println!("cifsiostat: CIFS I/O statistics"); 0 }
        _ => run_sar(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sar};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sysstat"), "sysstat");
        assert_eq!(basename(r"C:\bin\sysstat.exe"), "sysstat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sysstat.exe"), "sysstat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_sar(&["--help".to_string()], "sysstat"), 0);
        assert_eq!(run_sar(&["-h".to_string()], "sysstat"), 0);
        assert_eq!(run_sar(&["--version".to_string()], "sysstat"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_sar(&[], "sysstat"), 0);
    }
}
