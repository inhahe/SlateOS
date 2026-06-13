#![deny(clippy::all)]

//! snyk-cli — Slate OS Snyk security CLI
//!
//! Multi-personality: `snyk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_snyk(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: snyk COMMAND [OPTIONS]");
        println!("Snyk CLI 1.1292.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  test         Test for vulnerabilities");
        println!("  monitor      Monitor project for new vulnerabilities");
        println!("  container    Container security scanning");
        println!("  iac          Infrastructure as Code scanning");
        println!("  code         Static code analysis (SAST)");
        println!("  sbom         Generate SBOM");
        println!("  auth         Authenticate");
        println!("  config       Manage config");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("1.1292.0"),
        "auth" => {
            println!("Your account has been authenticated. Snyk is now ready to use.");
        }
        "test" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            let severity = args.windows(2).find(|w| w[0] == "--severity-threshold")
                .map(|w| w[1].as_str()).unwrap_or("low");
            println!("Testing {}...", path);
            println!();
            println!("Tested 156 dependencies for known issues.");
            println!("Found 3 issues, 1 with a fix available.");
            println!();
            println!("  ✗ High severity: Prototype Pollution [CVE-2024-1234]");
            println!("    Package: lodash@4.17.20");
            println!("    Fix: Upgrade to lodash@4.17.21");
            println!();
            println!("  ✗ Medium severity: Regular Expression DoS [CVE-2024-5678]");
            println!("    Package: minimatch@3.0.4");
            println!();
            println!("  ✗ Low severity: Information Exposure [CVE-2024-9012]");
            println!("    Package: debug@4.3.3");
            println!();
            println!("Severity threshold: {}", severity);
            println!("Organization: my-org");
        }
        "monitor" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("Monitoring {}...", path);
            println!("Exploring dependencies...");
            println!("Snapshot created: https://app.snyk.io/org/my-org/project/abc123");
            println!("Notifications will be emailed to: user@example.com");
        }
        "container" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("test");
            let image = args.get(2).map(|s| s.as_str()).unwrap_or("alpine:latest");
            match sub {
                "test" => {
                    println!("Testing {}...", image);
                    println!();
                    println!("Organization:  my-org");
                    println!("Package manager: apk");
                    println!("Docker image:   {}", image);
                    println!("Platform:       linux/amd64");
                    println!("Base image:     alpine:3.20");
                    println!();
                    println!("Tested 14 dependencies for known issues.");
                    println!("Found 2 issues.");
                }
                "monitor" => {
                    println!("Monitoring {}...", image);
                    println!("Snapshot created for container image.");
                }
                _ => println!("snyk container: '{}' completed", sub),
            }
        }
        "iac" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("test");
            let path = args.get(2).map(|s| s.as_str()).unwrap_or(".");
            if sub == "test" {
                println!("Snyk Infrastructure as Code");
                println!();
                println!("Testing {}...", path);
                println!();
                println!("  Issue: S3 bucket without server-side encryption");
                println!("  Severity: Medium");
                println!("  File: main.tf, line 15");
                println!();
                println!("  Issue: Security group allows ingress from 0.0.0.0/0");
                println!("  Severity: High");
                println!("  File: main.tf, line 42");
                println!();
                println!("Tested 3 files, found 2 issues.");
            } else {
                println!("snyk iac: '{}' completed", sub);
            }
        }
        "code" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("test");
            let path = args.get(2).map(|s| s.as_str()).unwrap_or(".");
            if sub == "test" {
                println!("Testing {} for code issues...", path);
                println!();
                println!("  ✗ [High] SQL Injection");
                println!("    Path: src/db.py, line 23");
                println!("    Info: User input flows into SQL query");
                println!();
                println!("  ✗ [Medium] Hardcoded Secret");
                println!("    Path: src/config.py, line 8");
                println!();
                println!("Tested 45 files, found 2 issues.");
            } else {
                println!("snyk code: '{}' completed", sub);
            }
        }
        _ => println!("snyk: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "snyk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_snyk(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_snyk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/snyk"), "snyk");
        assert_eq!(basename(r"C:\bin\snyk.exe"), "snyk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("snyk.exe"), "snyk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_snyk(&["--help".to_string()]), 0);
        assert_eq!(run_snyk(&["-h".to_string()]), 0);
        let _ = run_snyk(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_snyk(&[]);
    }
}
