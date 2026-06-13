#![deny(clippy::all)]

//! env0-cli — SlateOS env0 environment automation
//!
//! Single personality: `env0`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_env0(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: env0 [COMMAND] [OPTIONS]");
        println!("env0 v2.0 (Slate OS) — Environment-as-a-Service platform");
        println!();
        println!("Commands:");
        println!("  environment list|create|destroy   Manage environments");
        println!("  deployment list|approve|cancel     Manage deployments");
        println!("  template list|create               Manage templates");
        println!("  project list|create                Manage projects");
        println!("  cost report                        Cost reporting");
        println!("  drift detect                       Drift detection");
        println!();
        println!("Options:");
        println!("  --api-key KEY      API key");
        println!("  --org-id ID        Organization ID");
        println!("  --output json|table Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("env0 v2.0.1 (Slate OS)"); return 0; }
    println!("env0 v2.0.1 (Slate OS)");
    println!("  Organizations: 1");
    println!("  Projects: 8");
    println!("  Environments: 23 active");
    println!("  Templates: 15");
    println!("  Deployments: 45 (last 7d)");
    println!("  Estimated cost: $1,234/mo");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "env0".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_env0(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_env0};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/env0"), "env0");
        assert_eq!(basename(r"C:\bin\env0.exe"), "env0.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("env0.exe"), "env0");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_env0(&["--help".to_string()], "env0"), 0);
        assert_eq!(run_env0(&["-h".to_string()], "env0"), 0);
        let _ = run_env0(&["--version".to_string()], "env0");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_env0(&[], "env0");
    }
}
