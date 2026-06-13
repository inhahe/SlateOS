#![deny(clippy::all)]

//! spss-cli — Slate OS IBM SPSS Statistics
//!
//! Single personality: `spss`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_spss(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: spss [OPTIONS] [FILE]");
        println!("IBM SPSS Statistics 30 (Slate OS) — Statistical analysis software");
        println!();
        println!("Options:");
        println!("  --data FILE            Open .sav data file");
        println!("  --syntax FILE          Run .sps syntax file");
        println!("  --output FILE          .spv output viewer file");
        println!("  --modeler              Launch SPSS Modeler (data mining)");
        println!("  --amos                 IBM SPSS Amos (structural equation modeling)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("IBM SPSS Statistics 30.0.0.0 (Slate OS)"); return 0; }
    println!("IBM SPSS Statistics 30.0.0.0 (Slate OS)");
    println!("  Editions: Base, Standard, Professional, Premium, Subscription");
    println!("  Format: .sav (data), .sps (syntax), .spv (output viewer)");
    println!("  Procedures: descriptives, regression, ANOVA, factor, cluster, survival,");
    println!("              non-parametric, time series, neural networks, decision trees");
    println!("  Programming: SPSS Syntax + Python/R integration via plug-ins");
    println!("  Companion products: SPSS Modeler (data mining), Amos (SEM), Statistics Server");
    println!("  Use cases: academic research, surveys, market research, social sciences");
    println!("  License: per-seat subscription (commercial + academic discount)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "spss".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_spss(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_spss};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/spss"), "spss");
        assert_eq!(basename(r"C:\bin\spss.exe"), "spss.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("spss.exe"), "spss");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_spss(&["--help".to_string()], "spss"), 0);
        assert_eq!(run_spss(&["-h".to_string()], "spss"), 0);
        let _ = run_spss(&["--version".to_string()], "spss");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_spss(&[], "spss");
    }
}
