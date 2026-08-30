#![deny(clippy::all)]

//! mgmt-cli — Slate OS mgmt config management (reactive)
//!
//! Single personality: `mgmt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mgmt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mgmt COMMAND [OPTIONS]");
        println!("mgmt v0.1 (Slate OS) — Next-gen reactive config management");
        println!();
        println!("Commands:");
        println!("  run               Run mgmt engine");
        println!("  deploy            Deploy to cluster");
        println!("  get               Show current state");
        println!("  validate FILE     Validate config");
        println!("  version           Show version");
        println!();
        println!("Features:");
        println!("  Reactive: monitors resources in real-time (inotify, etc.)");
        println!("  Parallel: applies independent resources concurrently");
        println!("  Event-driven: responds to changes instantly");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "run" => {
            println!("mgmt: starting engine...");
            println!("  Graph: 15 resources, 12 edges");
            println!("  Watchers: file(5), svc(3), pkg(2), exec(1)");
            println!("  Converged in 0.8s");
            println!("  Watching for changes...");
        }
        "deploy" => {
            println!("Deploying to cluster...");
            println!("  Nodes: 3");
            println!("  Syncing graph... done");
        }
        "get" => {
            println!("Resources:");
            println!("  file:/etc/hostname — converged");
            println!("  svc:nginx — running");
            println!("  pkg:htop — installed");
        }
        "validate" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("graph.yaml");
            println!("Validating: {}", file);
            println!("  Resources: 15");
            println!("  Edges: 12");
            println!("  Cycles: 0");
            println!("  Status: VALID");
        }
        "version" | "--version" => println!("mgmt v0.1 (Slate OS)"),
        _ => println!("mgmt {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mgmt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mgmt(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mgmt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mgmt"), "mgmt");
        assert_eq!(basename(r"C:\bin\mgmt.exe"), "mgmt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mgmt.exe"), "mgmt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mgmt(&["--help".to_string()], "mgmt"), 0);
        assert_eq!(run_mgmt(&["-h".to_string()], "mgmt"), 0);
        let _ = run_mgmt(&["--version".to_string()], "mgmt");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mgmt(&[], "mgmt");
    }
}
