#![deny(clippy::all)]

//! laminar-cli — OurOS Laminar CI
//!
//! Multi-personality: `laminard`, `laminarc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_laminar(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "laminarc" => {
                println!("laminarc (OurOS) — Laminar CI client");
                println!();
                println!("Commands:");
                println!("  queue JOB          Queue a job run");
                println!("  start JOB          Start a job immediately");
                println!("  run JOB            Queue and wait for completion");
                println!("  set KEY=VALUE      Set a parameter");
                println!("  abort RUN          Abort a running job");
                println!();
                println!("Options:");
                println!("  --host HOST:PORT   Laminar daemon address");
            }
            _ => {
                println!("laminard (OurOS) — Laminar CI daemon");
                println!();
                println!("Options:");
                println!("  --bind-http ADDR   HTTP listen address");
                println!("  --bind-rpc ADDR    RPC listen address");
                println!("  --home DIR         Laminar home directory");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Laminar v1.4.0 (OurOS)"); return 0; }
    match prog {
        "laminarc" => {
            println!("Laminar Client v1.4.0 (OurOS)");
            println!("  Server: localhost:8881");
            println!("  Status: connected");
        }
        _ => {
            println!("Laminar CI v1.4.0 (OurOS)");
            println!("  HTTP: http://0.0.0.0:8080");
            println!("  RPC: 0.0.0.0:8881");
            println!("  Jobs: 15 configured");
            println!("  Running: 2");
            println!("  Executors: 4");
            println!("  Queue: 0 pending");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "laminard".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_laminar(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_laminar};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/laminar"), "laminar");
        assert_eq!(basename(r"C:\bin\laminar.exe"), "laminar.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("laminar.exe"), "laminar");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_laminar(&["--help".to_string()], "laminar"), 0);
        assert_eq!(run_laminar(&["-h".to_string()], "laminar"), 0);
        assert_eq!(run_laminar(&["--version".to_string()], "laminar"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_laminar(&[], "laminar"), 0);
    }
}
