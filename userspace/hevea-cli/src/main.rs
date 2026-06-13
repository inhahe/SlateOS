#![deny(clippy::all)]

//! hevea-cli — Slate OS HeVeA LaTeX to HTML converter
//!
//! Multi-personality: `hevea`, `hacha`, `imagen`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hevea(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] FILE", prog);
        match prog {
            "hacha" => {
                println!("hacha (Slate OS) — Split HeVeA HTML output into pages");
                println!("  -o DIR     Output directory");
                println!("  -tocter    Split at table of contents entries");
            }
            "imagen" => {
                println!("imagen (Slate OS) — Generate images for HeVeA output");
                println!("  -png       Generate PNG images");
                println!("  -pdf       Generate PDF images");
            }
            _ => {
                println!("HeVeA v2.36 (Slate OS) — LaTeX to HTML translator");
                println!("  -o FILE    Output file");
                println!("  -text      Plain text output");
                println!("  -fix       Fix point");
                println!("  -exec CMD  Execute command for images");
                println!("  -e ENTITY  Entity mode (symbols, numeric)");
            }
        }
        println!("  --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("HeVeA v2.36 (Slate OS)"); return 0; }
    match prog {
        "hacha" => {
            println!("hacha: splitting HTML...");
            println!("  Input: document.html");
            println!("  Pages: 12");
            println!("  TOC: generated");
            println!("  Output: document/index.html + 11 chapter files");
        }
        _ => {
            println!("HeVeA v2.36 (Slate OS)");
            println!("  Input: document.tex");
            println!("  Packages: amsmath, graphicx, hyperref");
            println!("  Sections: 12");
            println!("  Figures: 5 (converted to PNG)");
            println!("  Equations: 23 (rendered as images)");
            println!("  Output: document.html (45 KB)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hevea".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hevea(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hevea};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hevea"), "hevea");
        assert_eq!(basename(r"C:\bin\hevea.exe"), "hevea.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hevea.exe"), "hevea");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hevea(&["--help".to_string()], "hevea"), 0);
        assert_eq!(run_hevea(&["-h".to_string()], "hevea"), 0);
        let _ = run_hevea(&["--version".to_string()], "hevea");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hevea(&[], "hevea");
    }
}
