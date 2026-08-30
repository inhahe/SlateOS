#![deny(clippy::all)]

//! rundeck-cli — Slate OS Rundeck job scheduler
//!
//! Single personality: `rundeck`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rundeck(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rundeck [COMMAND] [OPTIONS]");
        println!("Rundeck v5.3 (Slate OS) — Operations automation & job scheduler");
        println!();
        println!("Commands:");
        println!("  jobs list|run|export  Manage jobs");
        println!("  executions list|get   View executions");
        println!("  projects list|create  Manage projects");
        println!("  nodes list            List nodes");
        println!("  keys list|create      Manage key storage");
        println!("  tokens list|create    API tokens");
        println!("  scheduler takeover    Cluster scheduler takeover");
        println!();
        println!("Options:");
        println!("  --url URL          Rundeck server URL");
        println!("  --token TOKEN      API token");
        println!("  --project NAME     Project name");
        println!("  --format json|yaml Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Rundeck v5.3.0 (Slate OS)"); return 0; }
    println!("Rundeck v5.3.0 (Slate OS)");
    println!("  Projects: 5");
    println!("  Jobs: 67 defined");
    println!("  Nodes: 23");
    println!("  Running: 4 executions");
    println!("  Scheduled: 12 upcoming");
    println!("  Cluster mode: active");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rundeck".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rundeck(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rundeck};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rundeck"), "rundeck");
        assert_eq!(basename(r"C:\bin\rundeck.exe"), "rundeck.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rundeck.exe"), "rundeck");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rundeck(&["--help".to_string()], "rundeck"), 0);
        assert_eq!(run_rundeck(&["-h".to_string()], "rundeck"), 0);
        let _ = run_rundeck(&["--version".to_string()], "rundeck");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rundeck(&[], "rundeck");
    }
}
