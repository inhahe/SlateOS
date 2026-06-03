#![deny(clippy::all)]

//! outline-cli — OurOS Outline team wiki
//!
//! Single personality: `outline`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_outline(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: outline [COMMAND] [OPTIONS]");
        println!("Outline v0.75 (OurOS) — Team knowledge base and wiki");
        println!();
        println!("Commands:");
        println!("  serve              Start Outline server");
        println!("  migrate            Run database migrations");
        println!("  seed               Seed initial data");
        println!("  export             Export all documents");
        println!("  import FILE        Import documents (markdown/zip)");
        println!("  users              Manage users");
        println!("  collections        List collections");
        println!();
        println!("Options:");
        println!("  --port PORT        Server port (default: 3000)");
        println!("  --config FILE      Config file");
        println!("  --database-url URL PostgreSQL connection");
        println!("  --redis-url URL    Redis connection");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Outline v0.75.2 (OurOS)"); return 0; }
    println!("Outline v0.75.2 (OurOS)");
    println!("  Collections: 18");
    println!("  Documents: 1,234");
    println!("  Users: 56");
    println!("  Teams: 3");
    println!("  Storage: S3 compatible");
    println!("  Search: PostgreSQL FTS");
    println!("  Auth: OIDC, SAML, Slack");
    println!("  Server: http://0.0.0.0:3000");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "outline".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_outline(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_outline};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/outline"), "outline");
        assert_eq!(basename(r"C:\bin\outline.exe"), "outline.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("outline.exe"), "outline");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_outline(&["--help".to_string()], "outline"), 0);
        assert_eq!(run_outline(&["-h".to_string()], "outline"), 0);
        assert_eq!(run_outline(&["--version".to_string()], "outline"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_outline(&[], "outline"), 0);
    }
}
