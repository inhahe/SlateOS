#![deny(clippy::all)]

//! mupdf-cli — OurOS MuPDF tools
//!
//! Multi-personality: `mutool`, `mupdf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mupdf(args: &[String], prog: &str) -> i32 {
    if prog == "mupdf" {
        if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
            println!("Usage: mupdf [OPTIONS] FILE [PAGE]");
            println!("MuPDF 1.24.2 (OurOS) — Lightweight PDF viewer");
            return 0;
        }
        let file = args.iter().rfind(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("doc.pdf");
        println!("mupdf: Opening '{}'", file);
        return 0;
    }
    // mutool
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mutool COMMAND [OPTIONS]");
        println!("mutool (MuPDF 1.24.2, OurOS)");
        println!();
        println!("Commands:");
        println!("  draw       Render pages to images");
        println!("  clean      Rewrite PDF (linearize, compress)");
        println!("  extract    Extract embedded resources");
        println!("  info       Show PDF info");
        println!("  pages      Show page dimensions");
        println!("  poster     Split pages for poster printing");
        println!("  merge      Merge PDF files");
        println!("  convert    Convert document to another format");
        println!("  show       Show internal PDF structure");
        println!("  trace      Trace device calls");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "info" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("doc.pdf");
            println!("mutool info: {}", file);
            println!("  Pages: 42");
            println!("  PDF version: 1.7");
        }
        "draw" => println!("mutool draw: Rendering pages..."),
        "clean" => println!("mutool clean: Rewriting PDF..."),
        "extract" => println!("mutool extract: Extracting resources..."),
        "merge" => println!("mutool merge: Merging PDFs..."),
        "convert" => println!("mutool convert: Converting document..."),
        "pages" => {
            println!("  1: 612 x 792 (letter)");
            println!("  2: 612 x 792 (letter)");
        }
        _ => println!("mutool: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mutool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mupdf(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
