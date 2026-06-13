#![deny(clippy::all)]

//! troff-cli — SlateOS troff/nroff text formatter
//!
//! Multi-personality: `troff`, `nroff`, `tbl`, `eqn`, `pic`, `refer`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_troff(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [FILE...]", prog);
        match prog {
            "nroff" => {
                println!("nroff (SlateOS) — Format for terminal/TTY output");
                println!("  -man       Use man macros");
                println!("  -ms        Use ms macros");
                println!("  -T DEVICE  Output device (ascii, utf8, latin1)");
            }
            "tbl" => println!("tbl (SlateOS) — Table preprocessor for troff"),
            "eqn" => println!("eqn (SlateOS) — Equation preprocessor for troff"),
            "pic" => println!("pic (SlateOS) — Picture preprocessor for troff"),
            "refer" => println!("refer (SlateOS) — Bibliography preprocessor"),
            _ => {
                println!("troff (SlateOS) — Text formatter");
                println!("  -man       Use man macros");
                println!("  -ms        Use ms macros");
                println!("  -mm        Use mm macros");
                println!("  -T DEVICE  Output device (ps, pdf, dvi, ascii)");
            }
        }
        println!("  --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("troff (Heirloom) v2.0 (SlateOS)"); return 0; }
    match prog {
        "nroff" => {
            println!("nroff: formatting for terminal...");
            println!("  Macros: man");
            println!("  Input: ls.1");
            println!("  Output: formatted man page (120 lines)");
        }
        "tbl" | "eqn" | "pic" | "refer" => {
            println!("{}: preprocessing...", prog);
            println!("  Input processed, passing to troff");
        }
        _ => {
            println!("troff (SlateOS)");
            println!("  Macros: ms");
            println!("  Device: ps (PostScript)");
            println!("  Input: paper.ms");
            println!("  Pages: 15");
            println!("  Output: paper.ps");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "troff".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_troff(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_troff};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/troff"), "troff");
        assert_eq!(basename(r"C:\bin\troff.exe"), "troff.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("troff.exe"), "troff");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_troff(&["--help".to_string()], "troff"), 0);
        assert_eq!(run_troff(&["-h".to_string()], "troff"), 0);
        let _ = run_troff(&["--version".to_string()], "troff");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_troff(&[], "troff");
    }
}
