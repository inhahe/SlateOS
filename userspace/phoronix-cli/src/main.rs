#![deny(clippy::all)]

//! phoronix-cli — OurOS Phoronix Test Suite
//!
//! Single personality: `phoronix-test-suite`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_phoronix(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: phoronix-test-suite <command> [OPTIONS]");
        println!("phoronix-test-suite v10.8 (OurOS) — Automated benchmarking");
        println!();
        println!("Commands:");
        println!("  benchmark TEST    Run benchmark");
        println!("  install TEST      Install test");
        println!("  list-tests        List available tests");
        println!("  list-installed    List installed tests");
        println!("  result-file       Manage results");
        println!("  system-info       Show system info");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("phoronix-test-suite v10.8 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("system-info") => {
            println!("Phoronix Test Suite System Info:");
            println!("  OS: OurOS 1.0");
            println!("  Kernel: 0.1.0-ouros (x86_64)");
            println!("  CPU: AMD Ryzen 7 3700X @ 3.60GHz (8 Cores)");
            println!("  RAM: 16384 MB");
            println!("  GPU: AMD Radeon RX 580 8GB");
            println!("  Disk: 500GB SSD");
        }
        Some("list-tests") => {
            println!("Available test suites:");
            println!("  pts/cpu       CPU benchmarks");
            println!("  pts/disk      Disk I/O benchmarks");
            println!("  pts/memory    Memory benchmarks");
            println!("  pts/graphics  GPU benchmarks");
            println!("  pts/network   Network benchmarks");
        }
        _ => {
            println!("phoronix-test-suite: automated benchmark framework");
            println!("  Use 'list-tests' to see available benchmarks");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "phoronix-test-suite".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_phoronix(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_phoronix};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/phoronix"), "phoronix");
        assert_eq!(basename(r"C:\bin\phoronix.exe"), "phoronix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("phoronix.exe"), "phoronix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_phoronix(&["--help".to_string()], "phoronix"), 0);
        assert_eq!(run_phoronix(&["-h".to_string()], "phoronix"), 0);
        let _ = run_phoronix(&["--version".to_string()], "phoronix");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_phoronix(&[], "phoronix");
    }
}
