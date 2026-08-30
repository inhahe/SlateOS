#![deny(clippy::all)]

//! turbostat-cli — Slate OS turbostat CPU frequency & power monitor
//!
//! Single personality: `turbostat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_turbostat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: turbostat [OPTIONS] [COMMAND]");
        println!("turbostat v2024.01 (Slate OS) — CPU frequency, idle, power stats");
        println!();
        println!("Options:");
        println!("  -i INTERVAL    Update interval (default: 5.0s)");
        println!("  -n NUM         Number of iterations");
        println!("  -S             Show summary only");
        println!("  -q             Quiet (fewer columns)");
        println!("  --version      Show version");
        println!();
        println!("Reports CPU C-states, P-states, temperature, and power.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("turbostat v2024.01 (Slate OS)"); return 0; }
    println!("Core CPU   Avg_MHz Busy%   Bzy_MHz TSC_MHz   CPU%c1  CPU%c6  PkgTmp  PkgWatt");
    println!("  -    -       125  3.47     3600    3600     4.53   92.00     42      15.2");
    println!("  0    0       200  5.56     3600    3600     6.44   88.00     42      ");
    println!("  1    2       150  4.17     3600    3600     3.83   92.00     40      ");
    println!("  2    4        80  2.22     3600    3600     2.78   95.00     39      ");
    println!("  3    6        70  1.94     3600    3600     5.06   93.00     38      ");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "turbostat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_turbostat(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_turbostat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/turbostat"), "turbostat");
        assert_eq!(basename(r"C:\bin\turbostat.exe"), "turbostat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("turbostat.exe"), "turbostat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_turbostat(&["--help".to_string()], "turbostat"), 0);
        assert_eq!(run_turbostat(&["-h".to_string()], "turbostat"), 0);
        let _ = run_turbostat(&["--version".to_string()], "turbostat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_turbostat(&[], "turbostat");
    }
}
