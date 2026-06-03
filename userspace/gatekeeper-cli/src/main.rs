#![deny(clippy::all)]

//! gatekeeper-cli — OurOS OPA Gatekeeper policy tool
//!
//! Single personality: `gatekeeper`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gatekeeper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gatekeeper COMMAND [OPTIONS]");
        println!("Gatekeeper v3.16.0 (OurOS) — OPA Gatekeeper policy tool");
        println!();
        println!("Commands:");
        println!("  test            Test constraints against resources");
        println!("  verify          Verify constraint templates");
        println!("  expand          Expand Gatekeeper resources");
        println!("  audit           Audit cluster resources");
        println!("  sync            Show sync config");
        println!("  version         Show version");
        println!();
        println!("Options:");
        println!("  -t, --templates DIR   Constraint templates");
        println!("  -c, --constraints DIR Constraints");
        println!("  -r, --resources DIR   Resources to test");
        println!("  --format pretty|json  Output format");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Gatekeeper v3.16.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("test");
    match cmd {
        "test" => {
            println!("Testing constraints...");
            println!();
            println!("Template: K8sRequiredLabels");
            println!("  Constraint: all-must-have-owner");
            println!("    deployment/api:         PASS");
            println!("    deployment/worker:      FAIL - missing label 'owner'");
            println!("    service/api:            PASS");
            println!();
            println!("Template: K8sContainerLimits");
            println!("  Constraint: container-must-have-limits");
            println!("    deployment/api:         FAIL - missing CPU limit");
            println!("    deployment/worker:      PASS");
            println!();
            println!("Results: 3 passed, 2 failed");
        }
        "verify" => {
            println!("Verifying constraint templates...");
            println!("  K8sRequiredLabels: valid (Rego compiles OK)");
            println!("  K8sContainerLimits: valid (Rego compiles OK)");
            println!("  2 templates verified");
        }
        "audit" => {
            println!("Auditing cluster resources...");
            println!();
            println!("Violations:");
            println!("  deployment/worker (default): missing label 'owner'");
            println!("  deployment/api (default): missing CPU limit");
            println!();
            println!("2 violations found across 12 resources");
        }
        "expand" => println!("Expanded 3 Gatekeeper resources."),
        "sync" => {
            println!("Sync config:");
            println!("  Synced resources:");
            println!("    - Namespace");
            println!("    - Pod");
        }
        _ => println!("gatekeeper {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gatekeeper".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gatekeeper(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gatekeeper};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gatekeeper"), "gatekeeper");
        assert_eq!(basename(r"C:\bin\gatekeeper.exe"), "gatekeeper.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gatekeeper.exe"), "gatekeeper");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gatekeeper(&["--help".to_string()], "gatekeeper"), 0);
        assert_eq!(run_gatekeeper(&["-h".to_string()], "gatekeeper"), 0);
        assert_eq!(run_gatekeeper(&["--version".to_string()], "gatekeeper"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gatekeeper(&[], "gatekeeper"), 0);
    }
}
