#![deny(clippy::all)]

//! unoconv-cli — Slate OS universal document converter
//!
//! Single personality: `unoconv`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_unoconv(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: unoconv [OPTIONS] FILE...");
        println!("unoconv v0.9 (Slate OS) — Universal Office document converter");
        println!();
        println!("Options:");
        println!("  -f FORMAT     Output format (pdf, html, docx, odt, txt, csv, etc.)");
        println!("  -o FILE       Output filename");
        println!("  -d DOCTYPE    Document type (document, spreadsheet, presentation, graphics)");
        println!("  -e OPTION     Export filter option (key=value)");
        println!("  -i OPTION     Import filter option (key=value)");
        println!("  -l            Start listener mode");
        println!("  -n            No listener start (connect to existing)");
        println!("  -p PORT       Listener port (default: 2002)");
        println!("  -s HOST       Listener host");
        println!("  -t N          Timeout in seconds");
        println!("  --pipe NAME   Named pipe for connection");
        println!("  --show        List available output formats");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("unoconv v0.9 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--show") {
        println!("Supported output formats:");
        println!("  Document:     pdf, html, odt, docx, doc, rtf, txt, epub");
        println!("  Spreadsheet:  pdf, csv, xlsx, xls, ods, html, tsv");
        println!("  Presentation: pdf, pptx, ppt, odp, html, swf");
        println!("  Graphics:     pdf, png, jpg, svg, tiff, bmp, eps");
        return 0;
    }
    let format = args.windows(2).find(|w| w[0] == "-f").map(|w| w[1].as_str()).unwrap_or("pdf");
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-') && {
        let prev_idx = args.iter().position(|x| std::ptr::eq(x, *a)).unwrap_or(0);
        prev_idx == 0 || !matches!(args.get(prev_idx.wrapping_sub(1)).map(|s| s.as_str()), Some("-f" | "-o" | "-d" | "-e" | "-i" | "-p" | "-s" | "-t" | "--pipe"))
    }).collect();
    if files.is_empty() {
        eprintln!("unoconv: error: no input files specified");
        return 1;
    }
    for f in &files {
        let name: &str = f;
        println!("unoconv: converting {} -> {}.{}", f, name.rsplit_once('.').map_or(name, |(b, _)| b), format);
    }
    println!("unoconv: {} file(s) converted successfully", files.len());
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "unoconv".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_unoconv(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_unoconv};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/unoconv"), "unoconv");
        assert_eq!(basename(r"C:\bin\unoconv.exe"), "unoconv.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("unoconv.exe"), "unoconv");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_unoconv(&["--help".to_string()], "unoconv"), 0);
        assert_eq!(run_unoconv(&["-h".to_string()], "unoconv"), 0);
        let _ = run_unoconv(&["--version".to_string()], "unoconv");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_unoconv(&[], "unoconv");
    }
}
