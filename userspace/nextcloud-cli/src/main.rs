#![deny(clippy::all)]

//! nextcloud-cli — OurOS Nextcloud file sync & collaboration
//!
//! Single personality: `nextcloud`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nextcloud(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nextcloud [COMMAND] [OPTIONS]");
        println!("Nextcloud v29.0 (OurOS) — Self-hosted file sync & share");
        println!();
        println!("Commands:");
        println!("  occ maintenance:mode   Toggle maintenance mode");
        println!("  occ app:list           List installed apps");
        println!("  occ user:list          List users");
        println!("  occ files:scan         Scan filesystem");
        println!("  occ config:list        List configuration");
        println!("  occ upgrade            Upgrade Nextcloud");
        println!("  occ db:convert-type    Convert database type");
        println!();
        println!("Options:");
        println!("  --data-dir DIR     Data directory");
        println!("  --config-dir DIR   Config directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Nextcloud v29.0.4 (OurOS)"); return 0; }
    println!("Nextcloud v29.0.4 (OurOS)");
    println!("  Users: 45");
    println!("  Files: 123,456");
    println!("  Storage used: 234 GiB");
    println!("  Apps: 23 enabled");
    println!("  Server: https://cloud.example.com");
    println!("  Database: PostgreSQL");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nextcloud".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nextcloud(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nextcloud};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nextcloud"), "nextcloud");
        assert_eq!(basename(r"C:\bin\nextcloud.exe"), "nextcloud.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nextcloud.exe"), "nextcloud");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nextcloud(&["--help".to_string()], "nextcloud"), 0);
        assert_eq!(run_nextcloud(&["-h".to_string()], "nextcloud"), 0);
        assert_eq!(run_nextcloud(&["--version".to_string()], "nextcloud"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nextcloud(&[], "nextcloud"), 0);
    }
}
