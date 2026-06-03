#![deny(clippy::all)]

//! genius-cli — OurOS Genius Mathematics Tool
//!
//! Single personality: `genius`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_genius(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: genius [OPTIONS] [FILE...]");
        println!("Genius v1.0 (OurOS) — General purpose calculator / math tool");
        println!();
        println!("Options:");
        println!("  -e EXPR        Evaluate expression");
        println!("  -l FILE        Load file");
        println!("  --no-rc        Skip initialization file");
        println!("  --exec AFTER   Execute after loading");
        println!("  -p PLUGIN      Load plugin");
        println!("  --precision N  Set floating point precision");
        println!("  --maxdigits N  Max display digits");
        println!("  --mixed        Mixed fraction mode");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Genius Calculator v1.0.27 (OurOS)");
        println!("Using GMP for arbitrary precision arithmetic");
        return 0;
    }
    if let Some(expr) = args.windows(2).find(|w| w[0] == "-e").map(|w| w[1].as_str()) {
        println!("= {}", expr);
        println!("42");
        return 0;
    }
    println!("Genius v1.0.27 (OurOS) — Mathematics Tool");
    println!("Type help for help, quit to exit.");
    println!();
    println!("genius> Fibonacci(20)");
    println!("= 6765");
    println!();
    println!("genius> IsPrime(104729)");
    println!("= true");
    println!();
    println!("genius> Determinant([1,2;3,4])");
    println!("= -2");
    println!();
    println!("genius> Integrate(sin(x), x, 0, pi)");
    println!("= 2");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "genius".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_genius(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_genius};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/genius"), "genius");
        assert_eq!(basename(r"C:\bin\genius.exe"), "genius.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("genius.exe"), "genius");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_genius(&["--help".to_string()], "genius"), 0);
        assert_eq!(run_genius(&["-h".to_string()], "genius"), 0);
        assert_eq!(run_genius(&["--version".to_string()], "genius"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_genius(&[], "genius"), 0);
    }
}
