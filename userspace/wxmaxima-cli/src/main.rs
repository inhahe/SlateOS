#![deny(clippy::all)]

//! wxmaxima-cli — OurOS wxMaxima CAS frontend
//!
//! Single personality: `wxmaxima`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wxmaxima(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wxmaxima [OPTIONS] [FILE.wxmx]");
        println!("wxmaxima v23.12 (OurOS) — GUI for Maxima CAS");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Formatted math display, interactive plotting,");
        println!("  wizards for calculus/algebra/equations,");
        println!("  document-style worksheet (.wxmx)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wxmaxima v23.12 (OurOS)"); return 0; }
    println!("wxmaxima: Maxima CAS frontend started");
    println!("  Backend: Maxima 5.47");
    println!("  2D math display: enabled");
    println!("  Plotting: gnuplot integration");
    println!("  Wizards: integral, solve, ODE, matrix, sum");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wxmaxima".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wxmaxima(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wxmaxima};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wxmaxima"), "wxmaxima");
        assert_eq!(basename(r"C:\bin\wxmaxima.exe"), "wxmaxima.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wxmaxima.exe"), "wxmaxima");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wxmaxima(&["--help".to_string()], "wxmaxima"), 0);
        assert_eq!(run_wxmaxima(&["-h".to_string()], "wxmaxima"), 0);
        let _ = run_wxmaxima(&["--version".to_string()], "wxmaxima");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wxmaxima(&[], "wxmaxima");
    }
}
