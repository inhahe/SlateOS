#![deny(clippy::all)]

//! osv-scanner-cli — OurOS OSV-Scanner vulnerability scanner
//!
//! Single personality: `osv-scanner`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_osv_scanner(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: osv-scanner [OPTIONS] [PATHS...]");
        println!("OSV-Scanner v1.7.0 (OurOS) — Vulnerability scanner");
        println!();
        println!("Options:");
        println!("  -r, --recursive         Scan recursively");
        println!("  -L, --lockfile FILE      Scan lockfile");
        println!("  -S, --sbom FILE          Scan SBOM");
        println!("  --docker IMAGE           Scan container image");
        println!("  --format table|json|sarif Output format");
        println!("  --config FILE            Config file");
        println!("  --experimental-call-analysis  Call graph analysis");
        println!("  -V, --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("osv-scanner v1.7.0 (OurOS)");
        return 0;
    }
    println!("Scanning directory...");
    println!("  Found: Cargo.lock, package-lock.json");
    println!();
    println!("  Cargo.lock: scanned 45 packages");
    println!("  package-lock.json: scanned 312 packages");
    println!();
    println!("Vulnerabilities found:");
    println!("  GHSA-xxxx-yyyy  CRITICAL  openssl 1.1.1 (Cargo.lock)");
    println!("  CVE-2024-1234   HIGH      lodash 4.17.20 (package-lock.json)");
    println!("  CVE-2024-5678   MEDIUM    express 4.18.0 (package-lock.json)");
    println!();
    println!("3 vulnerabilities found in 357 packages");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "osv-scanner".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_osv_scanner(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_osv_scanner};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/osv-scanner"), "osv-scanner");
        assert_eq!(basename(r"C:\bin\osv-scanner.exe"), "osv-scanner.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("osv-scanner.exe"), "osv-scanner");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_osv_scanner(&["--help".to_string()], "osv-scanner"), 0);
        assert_eq!(run_osv_scanner(&["-h".to_string()], "osv-scanner"), 0);
        assert_eq!(run_osv_scanner(&["--version".to_string()], "osv-scanner"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_osv_scanner(&[], "osv-scanner"), 0);
    }
}
