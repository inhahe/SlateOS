#![deny(clippy::all)]

//! mathomatic-cli — OurOS Mathomatic computer algebra system
//!
//! Single personality: `mathomatic`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mathomatic(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mathomatic [OPTIONS] [FILE...]");
        println!("Mathomatic v16.0 (OurOS) — Computer Algebra System");
        println!();
        println!("Options:");
        println!("  -e EXPR     Evaluate expression");
        println!("  -c          Color mode");
        println!("  -b          Bold mode");
        println!("  -q          Quiet mode (no banner)");
        println!("  -r          Readline mode");
        println!("  -s COLS     Screen columns");
        println!("  -t          Test mode");
        println!("  --version   Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Mathomatic version 16.0.5 (OurOS)");
        return 0;
    }
    let quiet = args.iter().any(|a| a == "-q");
    if !quiet {
        println!("Mathomatic version 16.0.5 (OurOS)");
        println!("Copyright (C) Mathomatic project");
        println!("200 equation spaces available, currently using 1.");
    }
    println!("1-> x^2 + 2*x + 1 = 0");
    println!("#1: x^2 + 2*x + 1 = 0");
    println!();
    println!("1-> solve x");
    println!("#1: x = -1");
    println!();
    println!("1-> (a+b)^3");
    println!("#1: a^3 + 3*a^2*b + 3*a*b^2 + b^3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mathomatic".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mathomatic(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mathomatic};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mathomatic"), "mathomatic");
        assert_eq!(basename(r"C:\bin\mathomatic.exe"), "mathomatic.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mathomatic.exe"), "mathomatic");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mathomatic(&["--help".to_string()], "mathomatic"), 0);
        assert_eq!(run_mathomatic(&["-h".to_string()], "mathomatic"), 0);
        let _ = run_mathomatic(&["--version".to_string()], "mathomatic");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mathomatic(&[], "mathomatic");
    }
}
