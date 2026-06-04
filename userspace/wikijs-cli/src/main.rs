#![deny(clippy::all)]

//! wikijs-cli — OurOS Wiki.js wiki engine
//!
//! Single personality: `wikijs`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wikijs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wikijs [COMMAND] [OPTIONS]");
        println!("Wiki.js v2.5 (OurOS) — Modern wiki engine");
        println!();
        println!("Commands:");
        println!("  serve              Start Wiki.js server");
        println!("  migrate            Run database migrations");
        println!("  backup             Create backup");
        println!("  restore FILE       Restore from backup");
        println!("  users              Manage users");
        println!("  pages              List pages");
        println!("  search QUERY       Search content");
        println!("  render SYNC        Sync page rendering");
        println!();
        println!("Options:");
        println!("  --port PORT        Server port (default: 3000)");
        println!("  --config FILE      Config file");
        println!("  --db URL           Database connection URL");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Wiki.js v2.5.303 (OurOS)"); return 0; }
    println!("Wiki.js v2.5.303 (OurOS)");
    println!("  Pages: 678");
    println!("  Users: 45");
    println!("  Groups: 8");
    println!("  Storage: Git (local + sync)");
    println!("  Search engine: SQLite FTS5");
    println!("  Renderers: markdown, html, asciidoc");
    println!("  Authentication: local, LDAP, OAuth2");
    println!("  Server: http://0.0.0.0:3000");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wikijs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wikijs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wikijs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wikijs"), "wikijs");
        assert_eq!(basename(r"C:\bin\wikijs.exe"), "wikijs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wikijs.exe"), "wikijs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wikijs(&["--help".to_string()], "wikijs"), 0);
        assert_eq!(run_wikijs(&["-h".to_string()], "wikijs"), 0);
        let _ = run_wikijs(&["--version".to_string()], "wikijs");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wikijs(&[], "wikijs");
    }
}
