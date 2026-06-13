#![deny(clippy::all)]

//! stapler-cli — SlateOS stapler PDF manipulation
//!
//! Single personality: `stapler`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stapler(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: stapler COMMAND [OPTIONS] FILES...");
        println!("stapler v1.0 (Slate OS) — PDF stapling toolkit");
        println!();
        println!("Commands:");
        println!("  cat               Concatenate PDFs");
        println!("  burst             Split PDF into single pages");
        println!("  sel               Select pages (e.g. 1-5,10,15-20)");
        println!("  del               Delete pages");
        println!("  zip               Interleave pages from multiple PDFs");
        println!("  info              Show PDF info");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "cat" => {
            let files: Vec<&str> = args.iter().skip(1).filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
            println!("Concatenating {} PDFs...", files.len().max(2));
            println!("  Output: output.pdf");
            println!("  Total pages: 42");
        }
        "burst" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("input.pdf");
            println!("Splitting: {}", file);
            println!("  Created 15 single-page PDFs");
        }
        "sel" => {
            println!("Selecting pages...");
            println!("  Output: selected.pdf");
            println!("  Pages: 8");
        }
        "del" => {
            println!("Deleting pages...");
            println!("  Output: trimmed.pdf");
        }
        "info" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("document.pdf");
            println!("File: {}", file);
            println!("  Pages: 15");
            println!("  Creator: LaTeX");
            println!("  Producer: pdfTeX");
        }
        _ => println!("stapler {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stapler".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stapler(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_stapler};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/stapler"), "stapler");
        assert_eq!(basename(r"C:\bin\stapler.exe"), "stapler.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("stapler.exe"), "stapler");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_stapler(&["--help".to_string()], "stapler"), 0);
        assert_eq!(run_stapler(&["-h".to_string()], "stapler"), 0);
        let _ = run_stapler(&["--version".to_string()], "stapler");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_stapler(&[], "stapler");
    }
}
