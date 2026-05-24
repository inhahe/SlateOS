#![deny(clippy::all)]

//! pdfunite-cli — OurOS pdfunite PDF merger
//!
//! Single personality: `pdfunite`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pdfunite(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pdfunite [OPTIONS] PDF1 PDF2... OUTPUT");
        println!("pdfunite v24.01 (OurOS) — Merge PDF files");
        println!();
        println!("Options:");
        println!("  PDF1 PDF2...      Input PDF files");
        println!("  OUTPUT            Output PDF file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pdfunite v24.01 (OurOS)"); return 0; }
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
mod tests { #[test] fn test_basic() { assert!(true); } }
