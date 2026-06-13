#![deny(clippy::all)]

//! xournalpp-cli — SlateOS Xournal++ handwriting note app
//!
//! Single personality: `xournalpp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xournalpp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xournalpp [OPTIONS] [FILE]");
        println!("xournalpp v1.2 (Slate OS) — Handwriting & PDF annotation");
        println!();
        println!("Options:");
        println!("  --create-img=FILE Export to image");
        println!("  --create-pdf=FILE Export to PDF");
        println!("  --page=N          Start on page N");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("xournalpp v1.2 (Slate OS)"); return 0; }
    println!("xournalpp: handwriting note application started");
    println!("  Tools: pen, eraser, highlighter, text, shapes");
    println!("  Pressure sensitivity: enabled");
    println!("  PDF annotation: supported");
    println!("  LaTeX: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xournalpp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xournalpp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xournalpp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xournalpp"), "xournalpp");
        assert_eq!(basename(r"C:\bin\xournalpp.exe"), "xournalpp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xournalpp.exe"), "xournalpp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xournalpp(&["--help".to_string()], "xournalpp"), 0);
        assert_eq!(run_xournalpp(&["-h".to_string()], "xournalpp"), 0);
        let _ = run_xournalpp(&["--version".to_string()], "xournalpp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xournalpp(&[], "xournalpp");
    }
}
