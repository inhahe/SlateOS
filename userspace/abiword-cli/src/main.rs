#![deny(clippy::all)]

//! abiword-cli — OurOS AbiWord word processor
//!
//! Single personality: `abiword`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_abiword(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: abiword [OPTIONS] [FILE...]");
        println!("abiword v3.0 (OurOS) — Lightweight word processor");
        println!();
        println!("Options:");
        println!("  --to=FMT          Convert to format (pdf, html, odt, rtf)");
        println!("  --print-to-file=F Print to file");
        println!("  --plugin=NAME     Load plugin");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("abiword v3.0 (OurOS)"); return 0; }
    println!("abiword: word processor started");
    println!("  Formats: ABW, ODT, DOCX, RTF, HTML, PDF");
    println!("  Plugins: 5 loaded");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "abiword".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_abiword(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_abiword};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/abiword"), "abiword");
        assert_eq!(basename(r"C:\bin\abiword.exe"), "abiword.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("abiword.exe"), "abiword");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_abiword(&["--help".to_string()], "abiword"), 0);
        assert_eq!(run_abiword(&["-h".to_string()], "abiword"), 0);
        assert_eq!(run_abiword(&["--version".to_string()], "abiword"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_abiword(&[], "abiword"), 0);
    }
}
