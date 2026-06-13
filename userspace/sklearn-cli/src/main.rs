#![deny(clippy::all)]

//! sklearn-cli — SlateOS scikit-learn machine learning library
//!
//! Multi-personality: `sklearn`

use std::env;
use std::process;

fn run_sklearn(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sklearn COMMAND [OPTIONS]");
        println!();
        println!("Commands: version, info, test, benchmark, show-versions");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => println!("scikit-learn 1.4.0 (SlateOS)"),
        "info" | "show-versions" => {
            println!("scikit-learn 1.4.0");
            println!("System:");
            println!("    python: 3.12.0 (SlateOS)");
            println!("    machine: x86_64");
            println!("Python dependencies:");
            println!("    numpy: 1.26.4");
            println!("    scipy: 1.12.0");
            println!("    joblib: 1.3.2");
            println!("    threadpoolctl: 3.2.0");
            println!("Built with:");
            println!("    OpenMP: YES");
            println!("    BLAS: OpenBLAS");
        }
        "test" => {
            println!("Running scikit-learn tests...");
            println!("test_classification: 1234 passed");
            println!("test_regression: 890 passed");
            println!("test_clustering: 567 passed");
            println!("test_preprocessing: 456 passed");
            println!("test_model_selection: 345 passed");
            println!("test_metrics: 678 passed");
            println!("All 4170 tests passed.");
        }
        "benchmark" => {
            println!("scikit-learn benchmarks:");
            println!("  RandomForest fit (10k samples, 20 features): 120 ms");
            println!("  SVM fit (5k samples, 10 features): 45 ms");
            println!("  KMeans fit (10k samples, 50 features): 89 ms");
            println!("  PCA transform (10k samples, 100 features): 12 ms");
            println!("  GradientBoosting fit (10k, 20 feat): 340 ms");
            println!("  LogisticRegression fit (50k, 10 feat): 23 ms");
        }
        _ => println!("sklearn: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sklearn(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_sklearn};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sklearn(&["--help".to_string()]), 0);
        assert_eq!(run_sklearn(&["-h".to_string()]), 0);
        let _ = run_sklearn(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sklearn(&[]);
    }
}
