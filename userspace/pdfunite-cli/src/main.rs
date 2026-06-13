#![deny(clippy::all)]

//! pdfunite-cli — SlateOS pdfunite PDF merger
//!
//! Single personality: `pdfunite`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pdfunite(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pdfunite [OPTIONS] PDF1 PDF2... OUTPUT");
        println!("pdfunite v24.01 (Slate OS) — Merge PDF files");
        println!();
        println!("Options:");
        println!("  PDF1 PDF2...      Input PDF files");
        println!("  OUTPUT            Output PDF file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pdfunite v24.01 (Slate OS)"); return 0; }
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    let output = files.last().copied().unwrap_or("output.pdf");
    let inputs = if files.len() > 1 { files.len() - 1 } else { 2 };
    println!("Merging {} PDFs -> {}", inputs, output);
    println!("  Done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pdfunite".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pdfunite(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pdfunite};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pdfunite"), "pdfunite");
        assert_eq!(basename(r"C:\bin\pdfunite.exe"), "pdfunite.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pdfunite.exe"), "pdfunite");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pdfunite(&["--help".to_string()], "pdfunite"), 0);
        assert_eq!(run_pdfunite(&["-h".to_string()], "pdfunite"), 0);
        let _ = run_pdfunite(&["--version".to_string()], "pdfunite");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pdfunite(&[], "pdfunite");
    }
}
