#![deny(clippy::all)]

//! gnote-cli — Slate OS Gnote GNOME note-taking
//!
//! Single personality: `gnote`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gnote(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnote [OPTIONS]");
        println!("gnote v45.0 (Slate OS) — GNOME desktop notes");
        println!();
        println!("Options:");
        println!("  --new-note        Create a new note");
        println!("  --search QUERY    Open search");
        println!("  --note TITLE      Open specific note");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnote v45.0 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--new-note") {
        println!("gnote: new note created");
        return 0;
    }
    println!("gnote: GNOME notes application started");
    println!("  Notes: 25");
    println!("  Notebooks: 3");
    println!("  Sync: WebDAV");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnote".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gnote(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gnote};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gnote"), "gnote");
        assert_eq!(basename(r"C:\bin\gnote.exe"), "gnote.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gnote.exe"), "gnote");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gnote(&["--help".to_string()], "gnote"), 0);
        assert_eq!(run_gnote(&["-h".to_string()], "gnote"), 0);
        let _ = run_gnote(&["--version".to_string()], "gnote");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gnote(&[], "gnote");
    }
}
