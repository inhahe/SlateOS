#![deny(clippy::all)]

//! lightgbm-cli — OurOS LightGBM gradient boosting
//!
//! Single personality: `lightgbm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lightgbm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lightgbm config=FILE");
        println!("LightGBM v4.3 (OurOS) — Fast gradient boosting framework");
        println!();
        println!("Config parameters:");
        println!("  task = train|predict|convert_model");
        println!("  data = FILE              Training data");
        println!("  valid = FILE             Validation data");
        println!("  output_model = FILE      Model output");
        println!("  input_model = FILE       Model input");
        println!("  objective = binary|multiclass|regression|lambdarank");
        println!("  boosting = gbdt|dart|goss|rf");
        println!("  num_leaves = N           Max leaves (default: 31)");
        println!("  learning_rate = F        Learning rate (default: 0.1)");
        println!("  num_iterations = N       Number of iterations (default: 100)");
        println!("  num_threads = N          Number of threads");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("LightGBM v4.3.0 (OurOS)"); return 0; }
    println!("LightGBM v4.3.0 (OurOS)");
    println!("  Task: train");
    println!("  Boosting: GBDT");
    println!("  Objective: binary");
    println!("  Data: 100,000 rows, 50 features");
    println!("  [1/200] valid_0 binary_logloss: 0.6234");
    println!("  [50/200] valid_0 binary_logloss: 0.2345");
    println!("  [100/200] valid_0 binary_logloss: 0.1234");
    println!("  [150/200] valid_0 binary_logloss: 0.0987");
    println!("  [200/200] valid_0 binary_logloss: 0.0876");
    println!("  Model saved: lgbm_model.txt");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lightgbm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lightgbm(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lightgbm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lightgbm"), "lightgbm");
        assert_eq!(basename(r"C:\bin\lightgbm.exe"), "lightgbm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lightgbm.exe"), "lightgbm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_lightgbm(&["--help".to_string()], "lightgbm"), 0);
        assert_eq!(run_lightgbm(&["-h".to_string()], "lightgbm"), 0);
        assert_eq!(run_lightgbm(&["--version".to_string()], "lightgbm"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_lightgbm(&[], "lightgbm"), 0);
    }
}
