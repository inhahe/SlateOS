#![deny(clippy::all)]

//! argo-workflows-cli — Slate OS Argo Workflows
//!
//! Single personality: `argo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_argo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: argo [COMMAND] [OPTIONS]");
        println!("Argo Workflows v3.5 (Slate OS) — Kubernetes workflow engine");
        println!();
        println!("Commands:");
        println!("  submit FILE        Submit workflow");
        println!("  list               List workflows");
        println!("  get NAME           Get workflow details");
        println!("  logs NAME          View workflow logs");
        println!("  watch NAME         Watch workflow progress");
        println!("  delete NAME        Delete workflow");
        println!("  retry NAME         Retry failed workflow");
        println!("  template list      List workflow templates");
        println!("  cron list          List cron workflows");
        println!("  server             Start Argo server");
        println!();
        println!("Options:");
        println!("  --namespace NS     Kubernetes namespace");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Argo Workflows v3.5.8 (Slate OS)"); return 0; }
    println!("Argo Workflows v3.5.8 (Slate OS)");
    println!("  Workflows: 45 (12 running, 33 completed)");
    println!("  Templates: 8");
    println!("  Cron workflows: 3");
    println!("  Server: https://0.0.0.0:2746");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "argo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_argo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_argo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/argo-workflows"), "argo-workflows");
        assert_eq!(basename(r"C:\bin\argo-workflows.exe"), "argo-workflows.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("argo-workflows.exe"), "argo-workflows");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_argo(&["--help".to_string()], "argo-workflows"), 0);
        assert_eq!(run_argo(&["-h".to_string()], "argo-workflows"), 0);
        let _ = run_argo(&["--version".to_string()], "argo-workflows");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_argo(&[], "argo-workflows");
    }
}
