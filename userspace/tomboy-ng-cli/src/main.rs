#![deny(clippy::all)]

//! tomboy-ng-cli — OurOS Tomboy-ng desktop notes
//!
//! Single personality: `tomboy-ng`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tomboy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tomboy-ng [OPTIONS]");
        println!("tomboy-ng v0.36 (OurOS) — Desktop note-taking");
        println!();
        println!("Options:");
        println!("  --open-note TITLE Open specific note");
        println!("  --import FILE     Import note");
        println!("  --config-dir DIR  Config directory");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("tomboy-ng v0.36 (OurOS)"); return 0; }
    println!("tomboy-ng: desktop notes started");
    println!("  Notes: 35");
    println!("  Notebooks: 4");
    println!("  Format: XML");
    println!("  Sync: file-based");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tomboy-ng".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tomboy(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tomboy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tomboy-ng"), "tomboy-ng");
        assert_eq!(basename(r"C:\bin\tomboy-ng.exe"), "tomboy-ng.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tomboy-ng.exe"), "tomboy-ng");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tomboy(&["--help".to_string()], "tomboy-ng"), 0);
        assert_eq!(run_tomboy(&["-h".to_string()], "tomboy-ng"), 0);
        let _ = run_tomboy(&["--version".to_string()], "tomboy-ng");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tomboy(&[], "tomboy-ng");
    }
}
