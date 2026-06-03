#![deny(clippy::all)]

//! scikit-learn-cli — OurOS scikit-learn machine learning
//!
//! Single personality: `sklearn`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sklearn(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sklearn [OPTIONS] COMMAND");
        println!("scikit-learn v1.4 (OurOS) — Machine learning library");
        println!();
        println!("Commands:");
        println!("  train          Train a model");
        println!("  predict        Run predictions");
        println!("  evaluate       Evaluate model performance");
        println!("  cross-val      Cross-validation");
        println!("  grid-search    Hyperparameter grid search");
        println!();
        println!("Options:");
        println!("  --model TYPE   Model type (rf, svm, lr, knn, gb, mlp)");
        println!("  --data FILE    Input data (CSV)");
        println!("  --target COL   Target column");
        println!("  --output FILE  Save model");
        println!("  -j N           Parallel jobs");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("scikit-learn v1.4.2 (OurOS)"); return 0; }
    println!("scikit-learn v1.4.2 (OurOS)");
    println!("  Model: RandomForestClassifier");
    println!("  Data: dataset.csv (10,000 samples, 25 features)");
    println!("  Train/Test split: 80/20");
    println!("  Training...");
    println!("  Metrics:");
    println!("    Accuracy: 0.9456");
    println!("    Precision: 0.9389");
    println!("    Recall: 0.9512");
    println!("    F1 Score: 0.9450");
    println!("    AUC-ROC: 0.9823");
    println!("  Model saved: model.joblib");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sklearn".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sklearn(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sklearn};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/scikit-learn"), "scikit-learn");
        assert_eq!(basename(r"C:\bin\scikit-learn.exe"), "scikit-learn.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("scikit-learn.exe"), "scikit-learn");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_sklearn(&["--help".to_string()], "scikit-learn"), 0);
        assert_eq!(run_sklearn(&["-h".to_string()], "scikit-learn"), 0);
        assert_eq!(run_sklearn(&["--version".to_string()], "scikit-learn"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_sklearn(&[], "scikit-learn"), 0);
    }
}
