#![deny(clippy::all)]

//! databricks-cli — OurOS Databricks Data Intelligence Platform
//!
//! Single personality: `databricks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_db(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: databricks [OPTIONS] [SUBCMD]");
        println!("Databricks Data Intelligence Platform (OurOS) — Lakehouse + AI");
        println!();
        println!("Options:");
        println!("  --profile PROF         Configuration profile");
        println!("  workspace import       Import notebook");
        println!("  jobs run-now JOB_ID    Trigger job run");
        println!("  clusters create        Create compute cluster");
        println!("  --workspace            Open web workspace");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Databricks CLI v0.232.1 (OurOS)"); return 0; }
    println!("Databricks Data Intelligence Platform (OurOS)");
    println!("  Foundation: Apache Spark + Delta Lake + MLflow + Unity Catalog");
    println!("  Lakehouse: data warehouse + data lake unified architecture");
    println!("  Languages: Python, SQL, Scala, R, Java in notebooks");
    println!("  Photon: native vectorized C++ query engine (DBR > 9.x)");
    println!("  ML: AutoML, Feature Store, Model Registry, Mosaic AI (LLM platform)");
    println!("  Clouds: AWS, Azure (Azure Databricks first-party), GCP");
    println!("  Workflows: Jobs, Delta Live Tables (declarative pipelines)");
    println!("  Unity Catalog: unified governance — data, ML models, dashboards");
    println!("  License: pay-as-you-go DBUs by cluster size + cloud infra");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "databricks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_db(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_db};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/databricks"), "databricks");
        assert_eq!(basename(r"C:\bin\databricks.exe"), "databricks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("databricks.exe"), "databricks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_db(&["--help".to_string()], "databricks"), 0);
        assert_eq!(run_db(&["-h".to_string()], "databricks"), 0);
        assert_eq!(run_db(&["--version".to_string()], "databricks"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_db(&[], "databricks"), 0);
    }
}
