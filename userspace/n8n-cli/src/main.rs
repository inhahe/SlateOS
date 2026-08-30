#![deny(clippy::all)]

//! n8n-cli — Slate OS n8n workflow automation
//!
//! Single personality: `n8n`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_n8n(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: n8n [COMMAND] [OPTIONS]");
        println!("n8n v1.52 (Slate OS) — Workflow automation platform");
        println!();
        println!("Commands:");
        println!("  start              Start n8n");
        println!("  worker             Start worker mode");
        println!("  webhook            Start webhook mode");
        println!("  execute            Execute a workflow");
        println!("  export:workflow    Export workflows");
        println!("  import:workflow    Import workflows");
        println!("  export:credentials Export credentials");
        println!("  update:workflow    Update workflow");
        println!();
        println!("Options:");
        println!("  --port PORT        Listen port (default: 5678)");
        println!("  --tunnel           Use n8n tunnel");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("n8n v1.52.0 (Slate OS)"); return 0; }
    println!("n8n v1.52.0 (Slate OS)");
    println!("  Editor: http://0.0.0.0:5678");
    println!("  Workflows: 34 (28 active)");
    println!("  Executions: 2,345 (last 24h)");
    println!("  Credentials: 12");
    println!("  Nodes available: 450+");
    println!("  Webhooks: 8 active");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "n8n".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_n8n(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_n8n};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/n8n"), "n8n");
        assert_eq!(basename(r"C:\bin\n8n.exe"), "n8n.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("n8n.exe"), "n8n");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_n8n(&["--help".to_string()], "n8n"), 0);
        assert_eq!(run_n8n(&["-h".to_string()], "n8n"), 0);
        let _ = run_n8n(&["--version".to_string()], "n8n");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_n8n(&[], "n8n");
    }
}
