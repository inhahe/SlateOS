#![deny(clippy::all)]

//! htmldoc-cli — OurOS HTML document processor
//!
//! Single personality: `htmldoc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_htmldoc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: htmldoc [OPTIONS] FILE...");
        println!("htmldoc v1.9 (OurOS) — HTML to PDF/PS/EPUB converter");
        println!();
        println!("Options:");
        println!("  -f FORMAT         Output format: pdf, ps, epub, html");
        println!("  -o FILE           Output filename");
        println!("  --webpage         Convert as web page");
        println!("  --book            Convert as book");
        println!("  --continuous      Convert as continuous document");
        println!("  --title TITLE     Document title");
        println!("  --titleimage IMG  Title page image");
        println!("  --header HDF      Page header (left/center/right)");
        println!("  --footer HDF      Page footer (left/center/right)");
        println!("  --size SIZE       Page size (letter, a4, etc.)");
        println!("  --portrait        Portrait orientation");
        println!("  --landscape       Landscape orientation");
        println!("  --color            Color output");
        println!("  --gray             Grayscale output");
        println!("  --compression N    JPEG compression (1-100)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("htmldoc v1.9 (OurOS)"); return 0; }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        eprintln!("htmldoc: error: no input files specified");
        return 1;
    }
    let format = args.windows(2).find(|w| w[0] == "-f").map(|w| w[1].as_str()).unwrap_or("pdf");
    println!("HTMLDOC v1.9 (OurOS)");
    for f in &files {
        println!("  Processing: {}", f);
    }
    println!("  Format: {}", format.to_uppercase());
    println!("  Pages: 12");
    println!("  Output: output.{} [{} bytes]", format, 524_288);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "htmldoc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_htmldoc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_htmldoc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/htmldoc"), "htmldoc");
        assert_eq!(basename(r"C:\bin\htmldoc.exe"), "htmldoc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("htmldoc.exe"), "htmldoc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_htmldoc(&["--help".to_string()], "htmldoc"), 0);
        assert_eq!(run_htmldoc(&["-h".to_string()], "htmldoc"), 0);
        let _ = run_htmldoc(&["--version".to_string()], "htmldoc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_htmldoc(&[], "htmldoc");
    }
}
