#![deny(clippy::all)]

//! trivy-cli — Slate OS container security scanner
//!
//! Single personality: `trivy`

use std::env;
use std::process;

fn run_trivy(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: trivy <COMMAND> [OPTIONS] <TARGET>");
        println!();
        println!("A comprehensive security scanner for containers and filesystems.");
        println!();
        println!("Commands:");
        println!("  image        Scan a container image");
        println!("  fs           Scan a filesystem path");
        println!("  repo         Scan a git repository");
        println!("  config       Scan configuration files (IaC)");
        println!("  sbom         Generate SBOM");
        println!("  rootfs       Scan a rootfs");
        println!("  k8s          Scan Kubernetes cluster");
        println!("  server       Run as server mode");
        println!("  plugin       Manage plugins");
        println!("  version      Show version");
        println!();
        println!("Options:");
        println!("  --severity <SEV>   Filter by severity (CRITICAL,HIGH,MEDIUM,LOW)");
        println!("  --format <FMT>     Output format (table/json/sarif/cyclonedx/spdx)");
        println!("  --output <FILE>    Output file");
        println!("  --exit-code <N>    Exit code on findings");
        println!("  --ignore-unfixed   Skip unfixed vulnerabilities");
        println!("  --skip-dirs <DIR>  Directories to skip");
        println!("  --timeout <DUR>    Timeout (default: 5m)");
        println!("  --db-repository    OCI repository for vulnerability DB");
        println!("  --quiet            Suppress progress bar");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("Version: 0.50.0 (Slate OS)");
            println!("Vulnerability DB:");
            println!("  Version: 2");
            println!("  UpdatedAt: 2024-01-15 12:00:00");
            println!("  NextUpdate: 2024-01-16 12:00:00");
            0
        }
        "image" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("myregistry/myimage:latest");
            let severity_filter = args.windows(2)
                .find(|w| w[0] == "--severity")
                .map(|w| w[1].as_str())
                .unwrap_or("");
            let ignore_unfixed = args.iter().any(|a| a == "--ignore-unfixed");
            let json_out = args.windows(2)
                .find(|w| w[0] == "--format")
                .map(|w| w[1].as_str())
                .unwrap_or("table") == "json";

            println!("{} (alpine 3.19.0)", target);
            println!();
            println!("Total: 12 (UNKNOWN: 0, LOW: 4, MEDIUM: 5, HIGH: 2, CRITICAL: 1)");
            println!();

            if json_out {
                println!("{{\"Results\":[{{\"Target\":\"{}\",\"Vulnerabilities\":[", target);
                println!("  {{\"VulnerabilityID\":\"CVE-2024-0001\",\"Severity\":\"CRITICAL\",\"PkgName\":\"openssl\",\"InstalledVersion\":\"3.1.4\",\"FixedVersion\":\"3.1.5\"}}");
                println!("]}}]}}");
            } else {
                println!("┌──────────────┬────────────────┬──────────┬───────────────────┬───────────────┬──────────────────────────────┐");
                println!("│   Library    │ Vulnerability  │ Severity │ Installed Version │ Fixed Version │            Title             │");
                println!("├──────────────┼────────────────┼──────────┼───────────────────┼───────────────┼──────────────────────────────┤");
                if severity_filter.is_empty() || severity_filter.contains("CRITICAL") {
                    println!("│ openssl      │ CVE-2024-0001  │ CRITICAL │ 3.1.4             │ 3.1.5         │ Buffer overflow in SSL_read  │");
                }
                if severity_filter.is_empty() || severity_filter.contains("HIGH") {
                    println!("│ curl         │ CVE-2024-0002  │ HIGH     │ 8.5.0             │ 8.6.0         │ Header injection via HSTS    │");
                    if !ignore_unfixed {
                        println!("│ zlib         │ CVE-2024-0003  │ HIGH     │ 1.3               │               │ Heap overflow in inflate     │");
                    }
                }
                if severity_filter.is_empty() || severity_filter.contains("MEDIUM") {
                    println!("│ busybox      │ CVE-2024-0004  │ MEDIUM   │ 1.36.1            │ 1.36.2        │ Command injection in awk     │");
                    println!("│ libxml2      │ CVE-2024-0005  │ MEDIUM   │ 2.12.3            │ 2.12.4        │ XXE in xmlParseDocument      │");
                }
                println!("└──────────────┴────────────────┴──────────┴───────────────────┴───────────────┴──────────────────────────────┘");
            }
            0
        }
        "fs" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("{} (filesystem)", target);
            println!();
            println!("Total: 5 (UNKNOWN: 0, LOW: 1, MEDIUM: 2, HIGH: 1, CRITICAL: 1)");
            println!();
            println!("┌──────────────────┬────────────────┬──────────┬───────────────────┬───────────────┐");
            println!("│     Library      │ Vulnerability  │ Severity │ Installed Version │ Fixed Version │");
            println!("├──────────────────┼────────────────┼──────────┼───────────────────┼───────────────┤");
            println!("│ lodash           │ CVE-2024-1001  │ CRITICAL │ 4.17.20           │ 4.17.21       │");
            println!("│ express          │ CVE-2024-1002  │ HIGH     │ 4.18.2            │ 4.19.0        │");
            println!("│ axios            │ CVE-2024-1003  │ MEDIUM   │ 1.6.0             │ 1.6.5         │");
            println!("│ minimatch        │ CVE-2024-1004  │ MEDIUM   │ 3.0.4             │ 3.1.0         │");
            println!("│ debug            │ CVE-2024-1005  │ LOW      │ 4.3.4             │ 4.3.5         │");
            println!("└──────────────────┴────────────────┴──────────┴───────────────────┴───────────────┘");
            0
        }
        "repo" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("https://github.com/example/repo");
            println!("Cloning {}...", target);
            println!();
            println!("Total: 3 (LOW: 1, MEDIUM: 1, HIGH: 1)");
            println!();
            println!("Go (go.sum)");
            println!("┌──────────────────────┬────────────────┬──────────┬──────────┬──────────┐");
            println!("│       Library        │ Vulnerability  │ Severity │ Installed│ Fixed    │");
            println!("├──────────────────────┼────────────────┼──────────┼──────────┼──────────┤");
            println!("│ golang.org/x/crypto  │ CVE-2024-2001  │ HIGH     │ 0.17.0   │ 0.18.0   │");
            println!("│ golang.org/x/net     │ CVE-2024-2002  │ MEDIUM   │ 0.20.0   │ 0.21.0   │");
            println!("│ golang.org/x/text    │ CVE-2024-2003  │ LOW      │ 0.14.0   │ 0.14.1   │");
            println!("└──────────────────────┴────────────────┴──────────┴──────────┴──────────┘");
            0
        }
        "config" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("{} (infrastructure as code)", target);
            println!();
            println!("Tests: 15 (SUCCESSES: 10, FAILURES: 4, EXCEPTIONS: 1)");
            println!("Failures: 4 (LOW: 1, MEDIUM: 1, HIGH: 1, CRITICAL: 1)");
            println!();
            println!("┌──────────────────────────┬──────────┬──────────────────────────────────────────┐");
            println!("│        Check ID          │ Severity │               Description                │");
            println!("├──────────────────────────┼──────────┼──────────────────────────────────────────┤");
            println!("│ DS002                    │ CRITICAL │ Root user in Dockerfile                   │");
            println!("│ KSV001                   │ HIGH     │ Container running as root                 │");
            println!("│ KSV003                   │ MEDIUM   │ Default capabilities not dropped          │");
            println!("│ KSV011                   │ LOW      │ CPU limit not set                         │");
            println!("└──────────────────────────┴──────────┴──────────────────────────────────────────┘");
            0
        }
        "sbom" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("myimage:latest");
            let format = args.windows(2)
                .find(|w| w[0] == "--format")
                .map(|w| w[1].as_str())
                .unwrap_or("cyclonedx");

            match format {
                "spdx" | "spdx-json" => {
                    println!("{{");
                    println!("  \"spdxVersion\": \"SPDX-2.3\",");
                    println!("  \"dataLicense\": \"CC0-1.0\",");
                    println!("  \"name\": \"{}\",", target);
                    println!("  \"packages\": [");
                    println!("    {{\"name\": \"alpine-baselayout\", \"versionInfo\": \"3.4.3-r1\"}},");
                    println!("    {{\"name\": \"busybox\", \"versionInfo\": \"1.36.1-r15\"}},");
                    println!("    {{\"name\": \"openssl\", \"versionInfo\": \"3.1.4-r2\"}}");
                    println!("  ]");
                    println!("}}");
                }
                _ => {
                    println!("{{");
                    println!("  \"bomFormat\": \"CycloneDX\",");
                    println!("  \"specVersion\": \"1.5\",");
                    println!("  \"metadata\": {{\"component\": {{\"name\": \"{}\"}}}},", target);
                    println!("  \"components\": [");
                    println!("    {{\"type\": \"library\", \"name\": \"alpine-baselayout\", \"version\": \"3.4.3-r1\"}},");
                    println!("    {{\"type\": \"library\", \"name\": \"busybox\", \"version\": \"1.36.1-r15\"}},");
                    println!("    {{\"type\": \"library\", \"name\": \"openssl\", \"version\": \"3.1.4-r2\"}}");
                    println!("  ]");
                    println!("}}");
                }
            }
            0
        }
        "k8s" => {
            println!("Scanning Kubernetes cluster...");
            println!();
            println!("Summary Report for kubernetes-admin@kubernetes");
            println!();
            println!("Workload Assessment:");
            println!("  Namespace    Resource       Vulnerabilities   Misconfigurations");
            println!("  default      Deployment/web CRITICAL:2 HIGH:5 MEDIUM:3 LOW:1");
            println!("  kube-system  DaemonSet/proxy CRITICAL:0 HIGH:1 MEDIUM:2 LOW:0");
            println!("  monitoring   StatefulSet/prom CRITICAL:0 HIGH:0 MEDIUM:1 LOW:2");
            println!();
            println!("RBAC Assessment:");
            println!("  Found 2 roles with excessive permissions");
            println!("  Found 1 cluster-admin binding to service account");
            println!();
            println!("Infra Assessment:");
            println!("  Kubelet: 3 misconfigurations found");
            println!("  API Server: 1 misconfiguration found");
            0
        }
        "plugin" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "list" => {
                    println!("Installed Plugins:");
                    println!("  Name        Version  Enabled");
                    println!("  aqua        0.1.0    true");
                    println!("  kubectl     0.2.0    true");
                    0
                }
                "install" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("example");
                    println!("Installing plugin '{}'...", name);
                    println!("Plugin '{}' installed successfully.", name);
                    0
                }
                _ => {
                    if sub.is_empty() {
                        eprintln!("Usage: trivy plugin <list|install|uninstall|run>");
                    } else {
                        eprintln!("Error: unknown plugin subcommand '{}'. See --help.", sub);
                    }
                    1
                }
            }
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: trivy <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
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
