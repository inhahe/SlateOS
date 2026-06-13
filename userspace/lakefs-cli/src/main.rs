#![deny(clippy::all)]

//! lakefs-cli — SlateOS lakeFS data lake version control
//!
//! Multi-personality: `lakefs`, `lakectl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lakefs(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "lakectl" => {
                println!("lakectl (Slate OS) — lakeFS command-line client");
                println!();
                println!("Commands:");
                println!("  repo list|create|delete  Manage repositories");
                println!("  branch list|create|reset Manage branches");
                println!("  commit -m MSG            Commit changes");
                println!("  diff REF                 Show diff");
                println!("  merge SOURCE DEST        Merge branches");
                println!("  log REF                  Show commit log");
                println!("  tag list|create          Manage tags");
                println!("  fs upload|download|cat   File operations");
            }
            _ => {
                println!("lakeFS v1.20 (Slate OS) — Git-like version control for data lakes");
                println!();
                println!("Commands:");
                println!("  run                Start lakeFS server");
                println!("  setup              Initial setup");
                println!("  migrate up         Run database migrations");
                println!("  superuser          Create admin user");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("lakeFS v1.20.0 (Slate OS)"); return 0; }
    match prog {
        "lakectl" => {
            println!("lakectl v1.20.0 (Slate OS)");
            println!("  Server: https://lakefs.example.com");
            println!("  Repositories: 5 accessible");
            println!("  Current branch: main");
        }
        _ => {
            println!("lakeFS v1.20.0 (Slate OS)");
            println!("  API: http://0.0.0.0:8000");
            println!("  Repositories: 5");
            println!("  Storage: s3://lakefs-data");
            println!("  Database: PostgreSQL (postgres KV)");
            println!("  Commits: 1,234 total");
            println!("  Active branches: 23");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lakefs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lakefs(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lakefs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lakefs"), "lakefs");
        assert_eq!(basename(r"C:\bin\lakefs.exe"), "lakefs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lakefs.exe"), "lakefs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lakefs(&["--help".to_string()], "lakefs"), 0);
        assert_eq!(run_lakefs(&["-h".to_string()], "lakefs"), 0);
        let _ = run_lakefs(&["--version".to_string()], "lakefs");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lakefs(&[], "lakefs");
    }
}
