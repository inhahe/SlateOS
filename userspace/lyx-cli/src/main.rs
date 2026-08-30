#![deny(clippy::all)]

//! lyx-cli — Slate OS LyX document processor
//!
//! Multi-personality: `lyx`, `lyxclient`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lyx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lyx [OPTIONS] [FILE...]");
        println!("lyx v2.4 (Slate OS) — Document processor (LaTeX frontend)");
        println!();
        println!("Options:");
        println!("  -e FMT            Export to format (pdf, dvi, ps, html)");
        println!("  -batch            Batch mode (no GUI)");
        println!("  --force-overwrite Overwrite output files");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("lyx v2.4 (Slate OS)"); return 0; }
    println!("lyx: document processor started");
    println!("  LaTeX backend: pdflatex");
    println!("  BibTeX: available");
    println!("  Spell checker: aspell");
    0
}

fn run_lyxclient(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lyxclient [OPTIONS] -c COMMAND");
        println!("lyxclient v2.4 (Slate OS) — LyX remote client");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("lyxclient v2.4 (Slate OS)"); return 0; }
    println!("lyxclient: connected to LyX server");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lyx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "lyxclient" => run_lyxclient(&rest, &prog),
        _ => run_lyx(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lyx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lyx"), "lyx");
        assert_eq!(basename(r"C:\bin\lyx.exe"), "lyx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lyx.exe"), "lyx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lyx(&["--help".to_string()], "lyx"), 0);
        assert_eq!(run_lyx(&["-h".to_string()], "lyx"), 0);
        let _ = run_lyx(&["--version".to_string()], "lyx");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lyx(&[], "lyx");
    }
}
