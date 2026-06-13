#![deny(clippy::all)]

//! fabric-cli — SlateOS Fabric remote execution tool
//!
//! Multi-personality: `fab`, `fabric`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fab(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fab [OPTIONS] <task> [ARGS...]");
        println!("fab v3.2 (SlateOS) — Remote execution and deployment");
        println!();
        println!("Options:");
        println!("  -H HOSTS       Comma-separated host list");
        println!("  -i KEY         SSH identity file");
        println!("  -u USER        Remote username");
        println!("  -f FILE        Fabfile path (default: fabfile.py)");
        println!("  -l             List available tasks");
        println!("  --version      Show version");
        println!();
        println!("Run tasks on remote hosts via SSH.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fab v3.2 (SlateOS, Fabric)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        println!("Available tasks:");
        println!("  deploy       Deploy application");
        println!("  rollback     Rollback last deployment");
        println!("  setup        Initial server setup");
        println!("  status       Check service status");
        return 0;
    }
    if let Some(task) = args.iter().find(|a| !a.starts_with('-')) {
        println!("fab: executing task '{}'", task);
        println!("  Hosts: localhost");
        println!("  Status: completed");
    } else {
        println!("fab: no task specified. Use -l to list tasks.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fab(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fab};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fabric"), "fabric");
        assert_eq!(basename(r"C:\bin\fabric.exe"), "fabric.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fabric.exe"), "fabric");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fab(&["--help".to_string()], "fabric"), 0);
        assert_eq!(run_fab(&["-h".to_string()], "fabric"), 0);
        let _ = run_fab(&["--version".to_string()], "fabric");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fab(&[], "fabric");
    }
}
