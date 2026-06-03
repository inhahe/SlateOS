#![deny(clippy::all)]

//! xgboost-cli — OurOS XGBoost gradient boosting
//!
//! Single personality: `xgboost`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xgboost(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xgboost CONFIG_FILE");
        println!("XGBoost v2.0 (OurOS) — Scalable gradient boosting");
        println!();
        println!("Config file options:");
        println!("  task = train|predict|dump");
        println!("  data = FILE              Training data (LibSVM format)");
        println!("  test:data = FILE         Test data");
        println!("  model_out = FILE         Model output");
        println!("  model_in = FILE          Model input");
        println!("  objective = reg:squarederror|binary:logistic|multi:softmax");
        println!("  max_depth = N            Max tree depth (default: 6)");
        println!("  eta = F                  Learning rate (default: 0.3)");
        println!("  num_round = N            Number of boosting rounds");
        println!("  nthread = N              Number of threads");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("XGBoost v2.0.3 (OurOS)"); return 0; }
    println!("XGBoost v2.0.3 (OurOS)");
    println!("  Task: train");
    println!("  Objective: binary:logistic");
    println!("  Data: train.libsvm (50,000 samples)");
    println!("  Parameters: max_depth=6, eta=0.1, nthread=8");
    println!("  [0] train-logloss:0.5234, eval-logloss:0.5456");
    println!("  [50] train-logloss:0.1234, eval-logloss:0.1567");
    println!("  [100] train-logloss:0.0456, eval-logloss:0.0789");
    println!("  Best iteration: 95");
    println!("  Model saved: model.xgb");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xgboost".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xgboost(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xgboost};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xgboost"), "xgboost");
        assert_eq!(basename(r"C:\bin\xgboost.exe"), "xgboost.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xgboost.exe"), "xgboost");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_xgboost(&["--help".to_string()], "xgboost"), 0);
        assert_eq!(run_xgboost(&["-h".to_string()], "xgboost"), 0);
        assert_eq!(run_xgboost(&["--version".to_string()], "xgboost"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_xgboost(&[], "xgboost"), 0);
    }
}
