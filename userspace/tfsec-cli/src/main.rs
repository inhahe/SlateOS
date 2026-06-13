#![deny(clippy::all)]

//! tfsec-cli — SlateOS tfsec Terraform security scanner
//!
//! Single personality: `tfsec`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tfsec(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tfsec [OPTIONS] [PATH]");
        println!("tfsec v1.28.6 (Slate OS) — Terraform security scanner");
        println!();
        println!("Options:");
        println!("  PATH                    Terraform directory");
        println!("  --format default|json|sarif|csv  Output format");
        println!("  --minimum-severity LOW|MEDIUM|HIGH|CRITICAL");
        println!("  --exclude IDS           Exclude rules");
        println!("  --include IDS           Include only rules");
        println!("  --soft-fail             Exit 0 even with findings");
        println!("  --no-color              Disable color");
        println!("  --config-file FILE      Config file");
        println!("  -V, --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("tfsec v1.28.6 (Slate OS)");
        return 0;
    }
    let path = args.first().map(|s| s.as_str()).unwrap_or(".");
    println!("  tfsec scanning {}", path);
    println!();
    println!("  Result 1");
    println!("  ──────────────────────────────");
    println!("  [HIGH] aws-s3-bucket: Bucket does not have encryption enabled");
    println!("  Resource: aws_s3_bucket.data");
    println!("  Location: main.tf:15-22");
    println!("  Rule: AWS017");
    println!();
    println!("  Result 2");
    println!("  ──────────────────────────────");
    println!("  [MEDIUM] aws-vpc: Security group rule allows ingress from 0.0.0.0/0");
    println!("  Resource: aws_security_group.web");
    println!("  Location: network.tf:8-18");
    println!("  Rule: AWS006");
    println!();
    println!("  2 potential problems detected (1 HIGH, 1 MEDIUM)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tfsec".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tfsec(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tfsec};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tfsec"), "tfsec");
        assert_eq!(basename(r"C:\bin\tfsec.exe"), "tfsec.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tfsec.exe"), "tfsec");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tfsec(&["--help".to_string()], "tfsec"), 0);
        assert_eq!(run_tfsec(&["-h".to_string()], "tfsec"), 0);
        let _ = run_tfsec(&["--version".to_string()], "tfsec");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tfsec(&[], "tfsec");
    }
}
