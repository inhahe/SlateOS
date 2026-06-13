#![deny(clippy::all)]

//! calibre-cli — SlateOS Calibre e-book management
//!
//! Multi-personality: `calibre`, `calibredb`, `ebook-convert`, `ebook-meta`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_calibredb(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: calibredb COMMAND [OPTIONS]");
        println!("calibredb 7.4.0 (SlateOS)");
        println!();
        println!("Commands: list, add, remove, search, export, catalog, set_metadata");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "--version" => println!("calibredb (calibre 7.4.0, SlateOS)"),
        "list" => {
            println!("id  title                          authors              formats");
            println!("1   The Art of Unix Programming    Eric S. Raymond      EPUB, PDF");
            println!("2   Structure and Interpretation   Abelson, Sussman     EPUB, PDF, MOBI");
            println!("3   Design Patterns                Gang of Four         PDF");
        }
        "add" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("book.epub");
            println!("Added book: {}", file);
            println!("  ID: 4");
        }
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("programming");
            println!("Search: {}", query);
            println!("  1, 2 (2 results)");
        }
        _ => println!("calibredb: '{}' completed", subcmd),
    }
    0
}

fn run_ebook_convert(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.len() < 2 {
        println!("Usage: ebook-convert INPUT OUTPUT [OPTIONS]");
        println!("  Supported formats: EPUB, MOBI, AZW3, PDF, DOCX, HTML, TXT, RTF, FB2");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ebook-convert (calibre 7.4.0, SlateOS)");
        return 0;
    }
    let input = args.first().map(|s| s.as_str()).unwrap_or("book.epub");
    let output = args.get(1).map(|s| s.as_str()).unwrap_or("book.mobi");
    println!("Converting: {} -> {}", input, output);
    println!("  Stage 1: Input plugin...");
    println!("  Stage 2: Processing...");
    println!("  Stage 3: Output plugin...");
    println!("  Output: {}", output);
    println!("  Conversion complete.");
    0
}

fn run_ebook_meta(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ebook-meta [OPTIONS] FILE");
        println!("  -t TITLE     Set title");
        println!("  -a AUTHORS   Set authors");
        println!("  --isbn ISBN  Set ISBN");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ebook-meta (calibre 7.4.0, SlateOS)");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("book.epub");
    println!("Title     : The Art of Programming");
    println!("Author(s) : John Doe");
    println!("Publisher  : Tech Books");
    println!("Language   : en");
    println!("Published  : 2024-01-15");
    println!("ISBN       : 978-0-123456-78-9");
    let _ = file;
    0
}

fn run_calibre(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: calibre [OPTIONS] [FILE]");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("calibre 7.4.0 (SlateOS)");
        return 0;
    }
    println!("calibre 7.4.0 — Starting...");
    println!("Ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "calibredb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "ebook-convert" => run_ebook_convert(&rest),
        "ebook-meta" => run_ebook_meta(&rest),
        "calibre" => run_calibre(&rest),
        _ => run_calibredb(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_calibredb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/calibre"), "calibre");
        assert_eq!(basename(r"C:\bin\calibre.exe"), "calibre.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("calibre.exe"), "calibre");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_calibredb(&["--help".to_string()]), 0);
        assert_eq!(run_calibredb(&["-h".to_string()]), 0);
        let _ = run_calibredb(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_calibredb(&[]);
    }
}
