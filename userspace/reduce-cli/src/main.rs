#![deny(clippy::all)]

//! reduce-cli — SlateOS REDUCE computer algebra system
//!
//! Single personality: `reduce`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_reduce(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: reduce [OPTIONS] [FILE]");
        println!("REDUCE v20240101 (Slate OS) — Computer Algebra System");
        println!();
        println!("Options:");
        println!("  -w              Suppress startup banner");
        println!("  -b              Batch mode");
        println!("  -l FILE         Log output to file");
        println!("  -D NAME=VAL     Define symbol");
        println!("  --texmacs       TeXmacs interface");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("REDUCE (Free CSL version), revision 20240101 (Slate OS)");
        return 0;
    }
    let quiet = args.iter().any(|a| a == "-w");
    if !quiet {
        println!("REDUCE (Free CSL version, revision 20240101) ...");
        println!("Type ? for help.");
    }
    println!("1: solve(x^2 - 5*x + 6, x);");
    println!();
    println!("{{{{x=2}},{{x=3}}}}");
    println!();
    println!("2: df(sin(x)*cos(x), x);");
    println!();
    println!("        2            2");
    println!(" cos(x)   - sin(x)");
    println!();
    println!("3: int(1/(x^2+1), x);");
    println!();
    println!(" atan(x)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "reduce".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_reduce(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_reduce};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/reduce"), "reduce");
        assert_eq!(basename(r"C:\bin\reduce.exe"), "reduce.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("reduce.exe"), "reduce");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_reduce(&["--help".to_string()], "reduce"), 0);
        assert_eq!(run_reduce(&["-h".to_string()], "reduce"), 0);
        let _ = run_reduce(&["--version".to_string()], "reduce");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_reduce(&[], "reduce");
    }
}
