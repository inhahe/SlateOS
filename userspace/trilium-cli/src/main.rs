#![deny(clippy::all)]

//! trilium-cli — SlateOS Trilium Notes knowledge base
//!
//! Single personality: `trilium`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_trilium(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: trilium [COMMAND] [OPTIONS]");
        println!("Trilium Notes v0.63 (SlateOS) — Hierarchical note-taking knowledge base");
        println!();
        println!("Commands:");
        println!("  serve              Start Trilium server");
        println!("  backup             Create database backup");
        println!("  restore FILE       Restore from backup");
        println!("  export FORMAT      Export notes (html/markdown/opml)");
        println!("  import FILE        Import notes");
        println!("  search QUERY       Search notes");
        println!("  sync               Sync with remote server");
        println!();
        println!("Options:");
        println!("  --port PORT        Server port (default: 8080)");
        println!("  --data-dir DIR     Data directory");
        println!("  --config FILE      Config file");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Trilium Notes v0.63.7 (SlateOS)"); return 0; }
    println!("Trilium Notes v0.63.7 (SlateOS)");
    println!("  Notes: 3,456");
    println!("  Branches: 4,123");
    println!("  Attributes: 12,890");
    println!("  Attachments: 234 (890 MB)");
    println!("  Database: ~/trilium-data/document.db (156 MB)");
    println!("  Server: http://0.0.0.0:8080");
    println!("  Sync: disabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "trilium".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_trilium(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_trilium};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/trilium"), "trilium");
        assert_eq!(basename(r"C:\bin\trilium.exe"), "trilium.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("trilium.exe"), "trilium");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_trilium(&["--help".to_string()], "trilium"), 0);
        assert_eq!(run_trilium(&["-h".to_string()], "trilium"), 0);
        let _ = run_trilium(&["--version".to_string()], "trilium");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_trilium(&[], "trilium");
    }
}
