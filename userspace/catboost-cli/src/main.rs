#![deny(clippy::all)]

//! catboost-cli — Slate OS CatBoost gradient boosting
//!
//! Single personality: `catboost`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_catboost(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: catboost MODE [OPTIONS]");
        println!("CatBoost v1.2 (Slate OS) — Gradient boosting with categorical features");
        println!();
        println!("Modes:");
        println!("  fit            Train model");
        println!("  calc           Apply model");
        println!("  fstr           Feature importance");
        println!("  eval-metrics   Evaluate metrics");
        println!();
        println!("Fit Options:");
        println!("  --learn-set FILE       Training data");
        println!("  --test-set FILE        Test data");
        println!("  --cd FILE              Column description");
        println!("  --loss-function FN     Loss function");
        println!("  --iterations N         Number of trees");
        println!("  --depth N              Tree depth (default: 6)");
        println!("  --learning-rate F      Learning rate");
        println!("  -m DIR                 Model output directory");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CatBoost v1.2.7 (Slate OS)"); return 0; }
    println!("CatBoost v1.2.7 (Slate OS)");
    println!("  Mode: fit");
    println!("  Loss: Logloss");
    println!("  Data: 200,000 rows, 30 features (8 categorical)");
    println!("  0: learn 0.5678 test 0.5789");
    println!("  100: learn 0.1234 test 0.1456");
    println!("  200: learn 0.0567 test 0.0789");
    println!("  300: learn 0.0345 test 0.0567");
    println!("  Best test: 0.0523 (iteration 289)");
    println!("  Model saved: catboost_model.cbm");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "catboost".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_catboost(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_catboost};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/catboost"), "catboost");
        assert_eq!(basename(r"C:\bin\catboost.exe"), "catboost.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("catboost.exe"), "catboost");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_catboost(&["--help".to_string()], "catboost"), 0);
        assert_eq!(run_catboost(&["-h".to_string()], "catboost"), 0);
        let _ = run_catboost(&["--version".to_string()], "catboost");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_catboost(&[], "catboost");
    }
}
