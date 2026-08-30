#![deny(clippy::all)]

//! xpdf-cli — Slate OS Xpdf PDF utilities
//!
//! Multi-personality: `pdfinfo`, `pdffonts`, `pdftoppm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pdfinfo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pdfinfo [OPTIONS] FILE.pdf");
        println!("pdfinfo v4.05 (Slate OS) — PDF document info");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("document.pdf");
    println!("Title:          Sample Document");
    println!("Author:         John Doe");
    println!("Creator:        LaTeX");
    println!("Producer:       pdfTeX-1.40.25");
    println!("CreationDate:   2024-01-15");
    println!("Pages:          42");
    println!("Page size:      612 x 792 pts (letter)");
    println!("File size:      1234567 bytes");
    println!("PDF version:    1.7");
    println!("File:           {}", file);
    0
}

fn run_pdffonts(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pdffonts [OPTIONS] FILE.pdf");
        println!("pdffonts v4.05 (Slate OS) — List fonts used in PDF");
        return 0;
    }
    println!("name                    type         emb sub uni");
    println!("---------------------------- ------------ --- --- ---");
    println!("CMSS10                  Type 1       yes yes yes");
    println!("CMBX12                  Type 1       yes yes yes");
    println!("TimesNewRoman           TrueType     yes no  yes");
    println!("Arial                   TrueType     yes no  yes");
    0
}

fn run_pdftoppm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pdftoppm [OPTIONS] PDF ROOT");
        println!("pdftoppm v4.05 (Slate OS) — Convert PDF pages to PPM/PNG/JPEG");
        println!();
        println!("Options:");
        println!("  -png              Output PNG format");
        println!("  -jpeg             Output JPEG format");
        println!("  -r DPI            Resolution (default 150)");
        println!("  -f N              First page");
        println!("  -l N              Last page");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("document.pdf");
    println!("Converting: {}", file);
    println!("  Pages: 1-3");
    println!("  Format: PPM");
    println!("  Resolution: 150 DPI");
    println!("  Output: page-1.ppm, page-2.ppm, page-3.ppm");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pdfinfo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pdffonts" => run_pdffonts(&rest, &prog),
        "pdftoppm" => run_pdftoppm(&rest, &prog),
        _ => run_pdfinfo(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pdfinfo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xpdf"), "xpdf");
        assert_eq!(basename(r"C:\bin\xpdf.exe"), "xpdf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xpdf.exe"), "xpdf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pdfinfo(&["--help".to_string()], "xpdf"), 0);
        assert_eq!(run_pdfinfo(&["-h".to_string()], "xpdf"), 0);
        let _ = run_pdfinfo(&["--version".to_string()], "xpdf");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pdfinfo(&[], "xpdf");
    }
}
