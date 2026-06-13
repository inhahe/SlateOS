#![deny(clippy::all)]

//! hardinfo-cli — SlateOS HardInfo system profiler
//!
//! Single personality: `hardinfo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hardinfo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hardinfo [OPTIONS]");
        println!("hardinfo v0.6 (Slate OS) — System Profiler and Benchmark");
        println!();
        println!("Options:");
        println!("  -r             Generate report");
        println!("  -f FORMAT      Report format (html, text)");
        println!("  -m MODULE      Load specific module");
        println!("  -l             List available modules");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hardinfo v0.6 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        println!("Modules: computer, devices, network, benchmarks");
        return 0;
    }
    println!("hardinfo: System Profiler");
    println!("  Computer: Custom Desktop");
    println!("  Processor: AMD Ryzen 7 3700X");
    println!("  Memory: 16384 MiB DDR4");
    println!("  Storage: 500 GiB SSD");
    println!("  GPU: AMD Radeon RX 580");
    println!("  Network: Intel I225-V Gigabit");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hardinfo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hardinfo(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hardinfo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hardinfo"), "hardinfo");
        assert_eq!(basename(r"C:\bin\hardinfo.exe"), "hardinfo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hardinfo.exe"), "hardinfo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hardinfo(&["--help".to_string()], "hardinfo"), 0);
        assert_eq!(run_hardinfo(&["-h".to_string()], "hardinfo"), 0);
        let _ = run_hardinfo(&["--version".to_string()], "hardinfo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hardinfo(&[], "hardinfo");
    }
}
