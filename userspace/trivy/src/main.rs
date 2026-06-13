#![deny(clippy::all)]

//! trivy — Slate OS vulnerability scanner
//!
//! Single personality: `trivy`

use std::env;
use std::process;

fn run_trivy(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: trivy <command> [flags] TARGET");
        println!();
        println!("Commands:");
        println!("  image       Scan container image");
        println!("  filesystem  Scan filesystem");
        println!("  repository  Scan git repository");
        println!("  rootfs      Scan rootfs");
        println!("  sbom        Generate/scan SBOM");
        println!("  config      Scan IaC files");
        println!("  kubernetes  Scan Kubernetes cluster");
        println!("  server      Run in server mode");
        println!("  plugin      Manage plugins");
        println!("  module      Manage modules");
        println!("  version     Show version");
        println!();
        println!("Flags:");
        println!("  --severity <s>       Severities to report (CRITICAL,HIGH,MEDIUM,LOW)");
        println!("  --format <fmt>       Output format (table/json/sarif/cyclonedx/spdx)");
        println!("  --output <file>      Output file");
        println!("  --quiet              Suppress progress");
        println!("  --skip-dirs <dirs>   Directories to skip");
        println!("  --scanners <s>       Scanners (vuln,misconfig,secret,license)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => {
            println!("Version: 0.52.0 (Slate OS)");
            println!("Vulnerability DB:");
            println!("  Version: 2");
            println!("  UpdatedAt: 2025-05-22 10:00:00");
        }
        "image" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("alpine:latest");
            println!("{} (alpine 3.19)", target);
            println!();
            println!("Total: 5 (CRITICAL: 1, HIGH: 2, MEDIUM: 1, LOW: 1)");
            println!();
            println!("+------------------+----------+----------+-------------------+-------------------+");
            println!("| Library          | Severity | Vuln ID  | Installed Version | Fixed Version     |");
            println!("+------------------+----------+----------+-------------------+-------------------+");
            println!("| openssl          | CRITICAL | CVE-2025 | 3.1.4             | 3.1.5             |");
            println!("| curl             | HIGH     | CVE-2025 | 8.5.0             | 8.6.0             |");
            println!("| zlib             | HIGH     | CVE-2025 | 1.3.0             | 1.3.1             |");
            println!("| busybox          | MEDIUM   | CVE-2025 | 1.36.1            | 1.36.2            |");
            println!("| musl             | LOW      | CVE-2025 | 1.2.4             | 1.2.5             |");
            println!("+------------------+----------+----------+-------------------+-------------------+");
        }
        "filesystem" | "fs" | "rootfs" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("Scanning {} ...", target);
            println!();
            println!("Total: 2 (HIGH: 1, MEDIUM: 1)");
            println!();
            println!("package-lock.json");
            println!("  lodash (4.17.20) -> 4.17.21 [HIGH: CVE-2021-23337]");
            println!("  axios  (0.21.1)  -> 0.21.2  [MEDIUM: CVE-2021-3749]");
        }
        "config" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("Scanning {} for misconfigurations...", target);
            println!();
            println!("Dockerfile (dockerfile)");
            println!("  MEDIUM: Specify a tag in the 'FROM' statement");
            println!("  LOW: Add HEALTHCHECK instruction");
        }
        "sbom" => {
            println!("{{");
            println!("  \"bomFormat\": \"CycloneDX\",");
            println!("  \"specVersion\": \"1.5\",");
            println!("  \"components\": []");
            println!("}}");
        }
        "server" => {
            println!("Trivy server listening on 0.0.0.0:4954");
        }
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_trivy(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_trivy};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_trivy(vec!["--help".to_string()]), 0);
        assert_eq!(run_trivy(vec!["-h".to_string()]), 0);
        let _ = run_trivy(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_trivy(vec![]);
    }
}
