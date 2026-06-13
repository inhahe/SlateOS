#![deny(clippy::all)]

//! knime-cli — SlateOS KNIME Analytics Platform
//!
//! Single personality: `knime`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_knime(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: knime [OPTIONS] [WORKFLOW]");
        println!("KNIME Analytics Platform 5.3 (Slate OS) — Open-source data analytics");
        println!();
        println!("Options:");
        println!("  -nosplash              No splash screen");
        println!("  -application APP       org.knime.product.KNIME_BATCH_APPLICATION");
        println!("  -workflowDir DIR       Workflow directory");
        println!("  -reset                 Reset workflow before running");
        println!("  --hub                  KNIME Hub (community/business workflows)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("KNIME Analytics Platform 5.3.2 (Slate OS)"); return 0; }
    println!("KNIME Analytics Platform 5.3.2 (Slate OS)");
    println!("  Architecture: Eclipse RCP + 6000+ visual programming nodes");
    println!("  Format: .knwf (workflow), node repository structured by category");
    println!("  Languages: Java nodes + Python/R/Bash/JavaScript scripting nodes");
    println!("  Integrations: H2O, Keras/TensorFlow, scikit-learn, XGBoost, Spark, Hadoop");
    println!("  Verticals: bioinformatics, cheminformatics, finance, marketing, NLP");
    println!("  KNIME Hub: community space + KNIME Business Hub (enterprise)");
    println!("  KNIME Server: now KNIME Business Hub for production deployment");
    println!("  Editions: Analytics Platform (free open-source GPL), Business Hub (commercial)");
    println!("  License: GPL v3 (open source), commercial for Hub/Server features");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "knime".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_knime(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_knime};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/knime"), "knime");
        assert_eq!(basename(r"C:\bin\knime.exe"), "knime.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("knime.exe"), "knime");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_knime(&["--help".to_string()], "knime"), 0);
        assert_eq!(run_knime(&["-h".to_string()], "knime"), 0);
        let _ = run_knime(&["--version".to_string()], "knime");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_knime(&[], "knime");
    }
}
