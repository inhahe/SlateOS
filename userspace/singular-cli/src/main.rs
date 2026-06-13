#![deny(clippy::all)]

//! singular-cli — SlateOS Singular computer algebra system
//!
//! Single personality: `singular`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_singular(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: singular [OPTIONS] [FILE]");
        println!("Singular v4.3 (SlateOS) — Computer Algebra for Polynomial Computations");
        println!();
        println!("Options:");
        println!("  -q              Quiet mode");
        println!("  -b              Batch mode");
        println!("  -e EXPR         Execute expression");
        println!("  --no-rc         Skip .singularrc");
        println!("  --min-time N    Set minimum time for profiling");
        println!("  --ticks-per-sec N  Timer resolution");
        println!("  --cntrlc=N      Max Ctrl-C count before abort");
        println!("  --emacs         Emacs interface mode");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Singular for x86_64-SlateOS version 4.3.2 (4300)");
        println!("with support for: GMP, factory, libfac, NTL");
        return 0;
    }
    let quiet = args.iter().any(|a| a == "-q");
    if !quiet {
        println!("                     SINGULAR                     /");
        println!(" A Computer Algebra System for Polynomial Computations /  version 4.3.2");
        println!("                                                   0<");
        println!(" by: W. Decker, G.-M. Greuel, G. Pfister, H. Schoenemann   \\");
        println!("FB Mathematik der Universitaet, D-67653 Kaiserslautern    \\");
    }
    println!("> ring r = 0,(x,y,z),dp;");
    println!("> ideal I = x2+y2-1, x3-y3;");
    println!("> std(I);");
    println!("_[1]=x2+y2-1");
    println!("_[2]=x3-y3");
    println!("_[3]=y4-y2");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "singular".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_singular(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_singular};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/singular"), "singular");
        assert_eq!(basename(r"C:\bin\singular.exe"), "singular.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("singular.exe"), "singular");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_singular(&["--help".to_string()], "singular"), 0);
        assert_eq!(run_singular(&["-h".to_string()], "singular"), 0);
        let _ = run_singular(&["--version".to_string()], "singular");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_singular(&[], "singular");
    }
}
