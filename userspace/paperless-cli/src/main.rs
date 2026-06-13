#![deny(clippy::all)]

//! paperless-cli — SlateOS Paperless-ngx document management
//!
//! Single personality: `paperless`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_paperless(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: paperless [COMMAND] [OPTIONS]");
        println!("Paperless-ngx v2.4 (SlateOS) — Document management system");
        println!();
        println!("Commands:");
        println!("  consume PATH       Import documents from path");
        println!("  search QUERY       Full-text search");
        println!("  list               List documents");
        println!("  export DIR         Export all documents");
        println!("  tag add/rm DOC TAG Manage tags");
        println!("  correspondent      Manage correspondents");
        println!("  manage             Run management commands");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file");
        println!("  --data-dir DIR     Data directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Paperless-ngx v2.4.3 (SlateOS)"); return 0; }
    println!("Paperless-ngx v2.4.3 (SlateOS)");
    println!("  Documents: 12,456");
    println!("  Tags: 78");
    println!("  Correspondents: 145");
    println!("  Document types: 23");
    println!("  Storage: 8.7 GB");
    println!("  OCR engine: Tesseract 5.3");
    println!("  Full-text index: 45,678 pages indexed");
    println!("  Consumption dir: /var/paperless/consume");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "paperless".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_paperless(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_paperless};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/paperless"), "paperless");
        assert_eq!(basename(r"C:\bin\paperless.exe"), "paperless.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("paperless.exe"), "paperless");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_paperless(&["--help".to_string()], "paperless"), 0);
        assert_eq!(run_paperless(&["-h".to_string()], "paperless"), 0);
        let _ = run_paperless(&["--version".to_string()], "paperless");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_paperless(&[], "paperless");
    }
}
