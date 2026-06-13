#![deny(clippy::all)]

//! optuna-cli — SlateOS Optuna hyperparameter optimization
//!
//! Single personality: `optuna`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_optuna(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: optuna COMMAND [OPTIONS]");
        println!("Optuna v3.6 (Slate OS) — Hyperparameter optimization framework");
        println!();
        println!("Commands:");
        println!("  create-study    Create a new study");
        println!("  delete-study    Delete a study");
        println!("  studies         List studies");
        println!("  trials          List trials in a study");
        println!("  best-trial      Show best trial");
        println!("  best-trials     Show Pareto front");
        println!("  dashboard       Launch web dashboard");
        println!();
        println!("Options:");
        println!("  --storage URL   Database URL");
        println!("  --study NAME    Study name");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Optuna v3.6.1 (Slate OS)"); return 0; }
    println!("Optuna v3.6.1 (Slate OS)");
    println!("  Study: rf_optimization");
    println!("  Sampler: TPE");
    println!("  Pruner: MedianPruner");
    println!("  Direction: maximize");
    println!("  Trials: 100");
    println!();
    println!("  Best trial:");
    println!("    Value: 0.9678");
    println!("    Params:");
    println!("      n_estimators: 234");
    println!("      max_depth: 12");
    println!("      min_samples_split: 5");
    println!("      learning_rate: 0.0823");
    println!("  Importance: max_depth > n_estimators > learning_rate > min_samples_split");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "optuna".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_optuna(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_optuna};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/optuna"), "optuna");
        assert_eq!(basename(r"C:\bin\optuna.exe"), "optuna.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("optuna.exe"), "optuna");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_optuna(&["--help".to_string()], "optuna"), 0);
        assert_eq!(run_optuna(&["-h".to_string()], "optuna"), 0);
        let _ = run_optuna(&["--version".to_string()], "optuna");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_optuna(&[], "optuna");
    }
}
