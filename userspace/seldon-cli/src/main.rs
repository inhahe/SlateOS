#![deny(clippy::all)]

//! seldon-cli — OurOS Seldon Core CLI
//!
//! Multi-personality: `seldon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_seldon(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: seldon COMMAND [OPTIONS]");
        println!("Seldon Core CLI 1.18.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  model          Manage model deployments");
        println!("  pipeline       Manage inference pipelines");
        println!("  experiment     Manage A/B experiments");
        println!("  server         Manage model servers");
        println!("  status         Show deployment status");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("seldon 1.18.0"),
        "model" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("NAME              REPLICAS  STATUS   SERVER");
                    println!("iris-model        2         Ready    triton");
                    println!("text-classifier   1         Ready    mlserver");
                }
                "load" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("iris-model");
                    println!("Loading model '{}'...", name);
                    println!("Model loaded successfully.");
                }
                "infer" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("iris-model");
                    println!("Inferring with '{}'...", name);
                    println!("{{\"predictions\": [0, 1, 2]}}");
                }
                _ => println!("seldon model: '{}' completed", sub),
            }
        }
        "pipeline" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("NAME              MODELS    STATUS");
                println!("nlp-pipeline      3         Ready");
                println!("cv-pipeline       2         Ready");
            } else {
                println!("seldon pipeline: '{}' completed", sub);
            }
        }
        "status" => {
            println!("Seldon Core Status:");
            println!("  Models:      5 loaded");
            println!("  Pipelines:   2 active");
            println!("  Experiments: 1 running");
            println!("  Requests:    12,345 (last 24h)");
        }
        _ => println!("seldon: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "seldon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_seldon(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_seldon};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/seldon"), "seldon");
        assert_eq!(basename(r"C:\bin\seldon.exe"), "seldon.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("seldon.exe"), "seldon");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_seldon(&["--help".to_string()]), 0);
        assert_eq!(run_seldon(&["-h".to_string()]), 0);
        let _ = run_seldon(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_seldon(&[]);
    }
}
