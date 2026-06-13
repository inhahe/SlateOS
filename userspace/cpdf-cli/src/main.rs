#![deny(clippy::all)]

//! cpdf-cli — Slate OS cpdf PDF command-line tools
//!
//! Single personality: `cpdf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cpdf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-help") || args.is_empty() {
        println!("Usage: cpdf [OPTIONS] INPUT [-o OUTPUT]");
        println!("cpdf 2.7 (Slate OS) — Coherent PDF command-line tools");
        println!();
        println!("Operations:");
        println!("  -merge           Merge PDFs");
        println!("  -split           Split at page boundaries");
        println!("  -split-bookmarks N  Split at bookmark level N");
        println!("  -scale-page SX SY  Scale pages");
        println!("  -scale-to-fit WxH  Scale to fit size");
        println!("  -rotate N        Rotate pages");
        println!("  -rotateby N      Rotate by N degrees");
        println!("  -upright         Make pages upright");
        println!("  -crop BOX        Set crop box");
        println!("  -remove-crop     Remove crop box");
        println!("  -encrypt METHOD  Encrypt PDF");
        println!("  -decrypt         Decrypt PDF");
        println!("  -compress        Compress streams");
        println!("  -decompress      Decompress streams");
        println!("  -squeeze         Reduce file size");
        println!("  -blacktext       Make all text black");
        println!("  -blacklines      Make all lines black");
        println!("  -blackfills      Make all fills black");
        println!("  -draft           Draft quality");
        println!("  -info            Show PDF info");
        println!("  -pages           Count pages");
        println!("  -page-info       Page dimensions");
        println!("  -version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("cpdf Version 2.7 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-info") {
        let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("doc.pdf");
        println!("Filename: {}", file);
        println!("Pages: 42");
        println!("Title: Document Title");
        println!("PDF Version: 1.7");
        return 0;
    }
    if args.iter().any(|a| a == "-pages") {
        println!("42");
        return 0;
    }
    if args.iter().any(|a| a == "-page-info") {
        println!("Page 1: 612.000000 x 792.000000");
        println!("Page 2: 612.000000 x 792.000000");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("doc.pdf");
    println!("cpdf: Processing '{}'...", file);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cpdf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cpdf(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cpdf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cpdf"), "cpdf");
        assert_eq!(basename(r"C:\bin\cpdf.exe"), "cpdf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cpdf.exe"), "cpdf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cpdf(&["--help".to_string()], "cpdf"), 0);
        assert_eq!(run_cpdf(&["-h".to_string()], "cpdf"), 0);
        let _ = run_cpdf(&["--version".to_string()], "cpdf");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cpdf(&[], "cpdf");
    }
}
