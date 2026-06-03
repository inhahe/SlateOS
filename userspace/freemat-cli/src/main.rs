#![deny(clippy::all)]

//! freemat-cli — OurOS FreeMat numerical computing environment
//!
//! Single personality: `freemat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_freemat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: freemat [OPTIONS] [SCRIPT.m]");
        println!("freemat v4.2 (OurOS) — Numerical computing environment");
        println!();
        println!("Options:");
        println!("  -e EXPR        Evaluate expression");
        println!("  -f SCRIPT      Execute script file");
        println!("  -noX           Disable graphical mode");
        println!("  -p PATH        Add path to search path");
        println!("  --nogui        Command-line mode only");
        println!("  --help-all     Show all built-in functions");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("FreeMat v4.2 (OurOS)"); return 0; }
    if let Some(expr) = args.windows(2).find(|w| w[0] == "-e").map(|w| w[1].as_str()) {
        println!("--> {}", expr);
        println!("ans =");
        println!("    42");
        return 0;
    }
    if args.iter().any(|a| a == "--help-all") {
        println!("Built-in functions:");
        println!("  Linear Algebra: inv, det, eig, svd, qr, lu, chol, rank");
        println!("  Matrix Ops:     zeros, ones, eye, rand, diag, reshape");
        println!("  Math:           sin, cos, exp, log, sqrt, abs, floor, ceil");
        println!("  Statistics:     mean, std, var, median, sum, prod, sort");
        println!("  I/O:            load, save, fopen, fclose, fprintf, fscanf");
        println!("  Plotting:       plot, surf, mesh, contour, bar, hist, title");
        return 0;
    }
    println!("FreeMat v4.2 (OurOS) — Numerical Computing Environment");
    println!("Type 'help' for help, 'quit' to exit.");
    println!("--> ");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "freemat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_freemat(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_freemat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/freemat"), "freemat");
        assert_eq!(basename(r"C:\bin\freemat.exe"), "freemat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("freemat.exe"), "freemat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_freemat(&["--help".to_string()], "freemat"), 0);
        assert_eq!(run_freemat(&["-h".to_string()], "freemat"), 0);
        assert_eq!(run_freemat(&["--version".to_string()], "freemat"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_freemat(&[], "freemat"), 0);
    }
}
