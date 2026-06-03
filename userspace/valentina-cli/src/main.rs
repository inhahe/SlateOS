#![deny(clippy::all)]

//! valentina-cli — OurOS Valentina Studio database IDE
//!
//! Single personality: `valentina-studio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_valentina(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: valentina-studio [OPTIONS]");
        println!("valentina-studio v13.5 (OurOS) — Universal database IDE");
        println!();
        println!("Options:");
        println!("  --project FILE  Open project file");
        println!("  --version       Show version");
        println!();
        println!("Supports: MySQL, PostgreSQL, SQLite, MS SQL, MariaDB,");
        println!("  Valentina DB");
        println!();
        println!("Features: Visual schema editor, SQL editor with autocomplete,");
        println!("  data transfer, report designer, diagram editor");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("valentina-studio v13.5 (OurOS)"); return 0; }
    println!("valentina-studio: database IDE started");
    println!("  Connections: 2 registered");
    println!("  Projects: 1 recent");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "valentina-studio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_valentina(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_valentina};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/valentina"), "valentina");
        assert_eq!(basename(r"C:\bin\valentina.exe"), "valentina.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("valentina.exe"), "valentina");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_valentina(&["--help".to_string()], "valentina"), 0);
        assert_eq!(run_valentina(&["-h".to_string()], "valentina"), 0);
        assert_eq!(run_valentina(&["--version".to_string()], "valentina"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_valentina(&[], "valentina"), 0);
    }
}
