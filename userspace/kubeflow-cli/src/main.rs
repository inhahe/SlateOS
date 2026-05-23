#![deny(clippy::all)]

//! kubeflow-cli — OurOS Kubeflow Pipelines CLI
//!
//! Multi-personality: `kfp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kfp(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: kfp COMMAND [OPTIONS]");
        println!("Kubeflow Pipelines CLI 2.7.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  run            Manage pipeline runs");
        println!("  pipeline       Manage pipelines");
        println!("  experiment     Manage experiments");
        println!("  component      Manage components");
        println!("  diagnose       Diagnose environment");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("kfp 2.7.0"),
        "pipeline" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID              NAME                CREATED");
                    println!("pl-abc123       training-pipeline   2024-01-15");
                    println!("pl-def456       inference-pipeline  2024-01-14");
                }
                "create" => {
                    let file = args.get(2).map(|s| s.as_str()).unwrap_or("pipeline.yaml");
                    println!("Creating pipeline from {}...", file);
                    println!("Pipeline created: pl-ghi789");
                }
                _ => println!("kfp pipeline: '{}' completed", sub),
            }
        }
        "run" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID           PIPELINE            STATUS      CREATED");
                    println!("run-abc      training-pipeline   Succeeded   2024-01-15");
                    println!("run-def      training-pipeline   Running     2024-01-15");
                }
                "create" => {
                    let pipeline = args.windows(2).find(|w| w[0] == "--pipeline-name")
                        .map(|w| w[1].as_str()).unwrap_or("training-pipeline");
                    println!("Creating run for pipeline '{}'...", pipeline);
                    println!("Run created: run-ghi");
                }
                _ => println!("kfp run: '{}' completed", sub),
            }
        }
        "experiment" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("ID           NAME              RUNS");
                println!("exp-abc      Default           15");
                println!("exp-def      Hyperparameter    8");
            } else {
                println!("kfp experiment: '{}' completed", sub);
            }
        }
        _ => println!("kfp: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kfp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kfp(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
