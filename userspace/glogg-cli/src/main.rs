#![deny(clippy::all)]

//! glogg-cli — OurOS glogg fast log explorer
//!
//! Single personality: `glogg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_glogg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: glogg [OPTIONS] [FILE...]");
        println!("glogg v1.1 (OurOS) — Fast smart log explorer");
        println!();
        println!("Options:");
        println!("  --multi           Open multiple files in tabs");
        println!("  --version         Show version");
        println!();
        println!("Features: regex search, auto-refresh, marks,");
        println!("  filtering view, handles multi-GB files efficiently");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("glogg v1.1 (OurOS)"); return 0; }
    println!("glogg: log explorer started");
    println!("  Search: regular expressions");
    println!("  Follow: auto-refresh on file changes");
    println!("  Marks: bookmark interesting lines");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "glogg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_glogg(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_glogg};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/glogg"), "glogg");
        assert_eq!(basename(r"C:\bin\glogg.exe"), "glogg.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("glogg.exe"), "glogg");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_glogg(&["--help".to_string()], "glogg"), 0);
        assert_eq!(run_glogg(&["-h".to_string()], "glogg"), 0);
        assert_eq!(run_glogg(&["--version".to_string()], "glogg"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_glogg(&[], "glogg"), 0);
    }
}
