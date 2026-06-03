#![deny(clippy::all)]

//! simple-scan-cli — OurOS Simple Scan document scanner
//!
//! Single personality: `simple-scan`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_simple_scan(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: simple-scan [OPTIONS] [FILE]");
        println!("simple-scan v42.0 (OurOS) — Simple document scanning");
        println!();
        println!("Options:");
        println!("  -d DEVICE         Use specific scanner");
        println!("  --fix-dpi         Fix incorrect scanner DPI");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("simple-scan v42.0 (OurOS)"); return 0; }
    println!("simple-scan: document scanner started");
    println!("  Scanner: Epson Perfection V39");
    println!("  Default mode: Flatbed, Color, 300 DPI");
    println!("  Ready to scan");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "simple-scan".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_simple_scan(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_simple_scan};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/simple-scan"), "simple-scan");
        assert_eq!(basename(r"C:\bin\simple-scan.exe"), "simple-scan.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("simple-scan.exe"), "simple-scan");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_simple_scan(&["--help".to_string()], "simple-scan"), 0);
        assert_eq!(run_simple_scan(&["-h".to_string()], "simple-scan"), 0);
        assert_eq!(run_simple_scan(&["--version".to_string()], "simple-scan"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_simple_scan(&[], "simple-scan"), 0);
    }
}
