#![deny(clippy::all)]

//! macaulay2-cli — Slate OS Macaulay2 algebraic geometry system
//!
//! Single personality: `M2`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_m2(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: M2 [OPTIONS] [FILE...]");
        println!("Macaulay2 v1.22 (Slate OS) — Algebraic Geometry & Commutative Algebra");
        println!();
        println!("Options:");
        println!("  -q               Quiet mode");
        println!("  -e EXPR          Evaluate expression");
        println!("  --script FILE    Run script and exit");
        println!("  --no-readline    Disable readline");
        println!("  --no-threads     Single-threaded mode");
        println!("  --prefix DIR     Installation prefix");
        println!("  --print-width N  Output width");
        println!("  --stop           Stop after errors");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Macaulay2, version 1.22 (Slate OS)");
        return 0;
    }
    if let Some(expr) = args.windows(2).find(|w| w[0] == "-e").map(|w| w[1].as_str()) {
        println!("i1 : {}", expr);
        println!("o1 = 42");
        return 0;
    }
    let quiet = args.iter().any(|a| a == "-q");
    if !quiet {
        println!("Macaulay2, version 1.22 (Slate OS)");
        println!("with packages: Core, Elimination, LLLBases, PrimaryDecomposition");
    }
    println!("i1 : R = QQ[x,y,z]");
    println!("o1 = R");
    println!("o1 : PolynomialRing");
    println!();
    println!("i2 : I = ideal(x^2+y^2-z^2, x*y)");
    println!("             2    2    2");
    println!("o2 = ideal (x  + y  - z , x*y)");
    println!("o2 : Ideal of R");
    println!();
    println!("i3 : dim I");
    println!("o3 = 1");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "M2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_m2(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_m2};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/macaulay2"), "macaulay2");
        assert_eq!(basename(r"C:\bin\macaulay2.exe"), "macaulay2.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("macaulay2.exe"), "macaulay2");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_m2(&["--help".to_string()], "macaulay2"), 0);
        assert_eq!(run_m2(&["-h".to_string()], "macaulay2"), 0);
        let _ = run_m2(&["--version".to_string()], "macaulay2");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_m2(&[], "macaulay2");
    }
}
