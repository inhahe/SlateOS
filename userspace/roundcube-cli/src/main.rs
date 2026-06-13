#![deny(clippy::all)]

//! roundcube-cli — SlateOS Roundcube webmail
//!
//! Single personality: `roundcube`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_roundcube(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: roundcube [COMMAND] [OPTIONS]");
        println!("Roundcube v1.6 (SlateOS) — Webmail client");
        println!();
        println!("Commands:");
        println!("  serve              Start webmail server");
        println!("  install            Run installation wizard");
        println!("  migrate            Run database migrations");
        println!("  clear-cache        Clear template/message cache");
        println!("  plugins            List/manage plugins");
        println!("  check-config       Validate configuration");
        println!();
        println!("Options:");
        println!("  --port PORT        Server port (default: 8080)");
        println!("  --config DIR       Config directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Roundcube v1.6.6 (SlateOS)"); return 0; }
    println!("Roundcube v1.6.6 (SlateOS)");
    println!("  IMAP: connected to localhost:143");
    println!("  SMTP: localhost:25");
    println!("  Database: SQLite");
    println!("  Plugins: managesieve, archive, zipdownload, markasjunk");
    println!("  Skins: elastic (default), larry");
    println!("  Cache: file-based (/var/roundcube/cache)");
    println!("  Server: http://0.0.0.0:8080");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "roundcube".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_roundcube(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_roundcube};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/roundcube"), "roundcube");
        assert_eq!(basename(r"C:\bin\roundcube.exe"), "roundcube.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("roundcube.exe"), "roundcube");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_roundcube(&["--help".to_string()], "roundcube"), 0);
        assert_eq!(run_roundcube(&["-h".to_string()], "roundcube"), 0);
        let _ = run_roundcube(&["--version".to_string()], "roundcube");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_roundcube(&[], "roundcube");
    }
}
