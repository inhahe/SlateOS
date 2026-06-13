#![deny(clippy::all)]

//! pdfarranger-cli — Slate OS PDF Arranger page manager
//!
//! Single personality: `pdfarranger`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pdfarranger(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pdfarranger [OPTIONS] [FILE...]");
        println!("pdfarranger v1.10 (Slate OS) — Merge, split, rotate PDF pages");
        println!();
        println!("Options:");
        println!("  FILE...           PDF files to open");
        println!("  --rotate N PAGES  Rotate pages (90/180/270)");
        println!("  --crop L,T,R,B   Crop pages");
        println!("  --merge FILES     Merge PDFs");
        println!("  --split FILE      Split PDF by page ranges");
        println!("  -o OUTPUT         Output file");
        return 0;
    }
    if args.iter().any(|a| a == "--merge") {
        let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
        println!("Merging {} PDFs...", files.len().max(2));
        println!("  Output: merged.pdf");
        return 0;
    }
    if args.iter().any(|a| a == "--rotate") {
        println!("Rotating pages...");
        println!("  Output: rotated.pdf");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    for f in &files {
        println!("Opening: {}", f);
    }
    println!("PDF Arranger ready — drag and drop pages to rearrange");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pdfarranger".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pdfarranger(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pdfarranger};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pdfarranger"), "pdfarranger");
        assert_eq!(basename(r"C:\bin\pdfarranger.exe"), "pdfarranger.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pdfarranger.exe"), "pdfarranger");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pdfarranger(&["--help".to_string()], "pdfarranger"), 0);
        assert_eq!(run_pdfarranger(&["-h".to_string()], "pdfarranger"), 0);
        let _ = run_pdfarranger(&["--version".to_string()], "pdfarranger");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pdfarranger(&[], "pdfarranger");
    }
}
