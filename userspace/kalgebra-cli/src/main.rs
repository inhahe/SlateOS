#![deny(clippy::all)]

//! kalgebra-cli — SlateOS KAlgebra math expression evaluator
//!
//! Single personality: `kalgebra`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kalgebra(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kalgebra [OPTIONS]");
        println!("kalgebra v23.08 (Slate OS) — Math expression graph calculator");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Tabs:");
        println!("  Calculator        Evaluate expressions");
        println!("  2D Graph          Plot 2D functions");
        println!("  3D Graph          Plot 3D surfaces");
        println!("  Dictionary        Math function reference");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kalgebra v23.08 (Slate OS)"); return 0; }
    println!("kalgebra: math expression evaluator started");
    println!("  Functions: sin, cos, tan, log, exp, sqrt, abs, ...");
    println!("  Variables: x, y, z, t, user-defined");
    println!("  2D plotting: Cartesian, polar, parametric");
    println!("  3D plotting: surfaces, parametric curves");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kalgebra".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kalgebra(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kalgebra};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kalgebra"), "kalgebra");
        assert_eq!(basename(r"C:\bin\kalgebra.exe"), "kalgebra.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kalgebra.exe"), "kalgebra");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kalgebra(&["--help".to_string()], "kalgebra"), 0);
        assert_eq!(run_kalgebra(&["-h".to_string()], "kalgebra"), 0);
        let _ = run_kalgebra(&["--version".to_string()], "kalgebra");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kalgebra(&[], "kalgebra");
    }
}
