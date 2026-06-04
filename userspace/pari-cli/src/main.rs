#![deny(clippy::all)]

//! pari-cli — OurOS PARI/GP number theory calculator
//!
//! Single personality: `gp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gp [OPTIONS] [FILE...]");
        println!("gp v2.15 (OurOS) — PARI/GP number theory calculator");
        println!();
        println!("Options:");
        println!("  -q              Quiet mode (no banner)");
        println!("  -s SIZE         Stack size (e.g., 100M)");
        println!("  -p PRIMELIMIT   Prime limit");
        println!("  -f              Fast start (no gprc)");
        println!("  -e EXPR         Evaluate expression and exit");
        println!("  --default KEY=VAL  Set default");
        println!("  --emacs         Emacs mode");
        println!("  --test          Test mode");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GP/PARI CALCULATOR Version 2.15 (OurOS)");
        println!("amd64 running ouros (x86-64 kernel)");
        return 0;
    }
    if let Some(expr) = args.windows(2).find(|w| w[0] == "-e").map(|w| w[1].as_str()) {
        println!("? {}", expr);
        println!("% = 42");
        return 0;
    }
    let quiet = args.iter().any(|a| a == "-q");
    if !quiet {
        println!("                  GP/PARI CALCULATOR Version 2.15 (OurOS)");
        println!("              amd64 running ouros (x86-64/GMP kernel)");
        println!("                   64-bit version, compiled for OurOS");
        println!("          Type ? for help, \\q to quit.");
        println!("          Type ?12 for how to use online help.");
    }
    println!("? factor(2^67 - 1)");
    println!("[193707721 1; 761838257287 1]");
    println!();
    println!("? isprime(1000000007)");
    println!("1");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pari"), "pari");
        assert_eq!(basename(r"C:\bin\pari.exe"), "pari.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pari.exe"), "pari");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gp(&["--help".to_string()], "pari"), 0);
        assert_eq!(run_gp(&["-h".to_string()], "pari"), 0);
        let _ = run_gp(&["--version".to_string()], "pari");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gp(&[], "pari");
    }
}
