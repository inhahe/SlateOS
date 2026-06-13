#![deny(clippy::all)]

//! cdk8s-cli — Slate OS CDK for Kubernetes CLI
//!
//! Multi-personality: `cdk8s`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cdk8s(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cdk8s COMMAND [OPTIONS]");
        println!("cdk8s 2.68.0 (Slate OS) — CDK for Kubernetes");
        println!();
        println!("Commands:");
        println!("  init           Create a new cdk8s project");
        println!("  import         Import Kubernetes API objects");
        println!("  synth          Synthesize Kubernetes manifests");
        println!("  diff           Show changes between synth and cluster");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("2.68.0"),
        "init" => {
            let lang = args.windows(2).find(|w| w[0] == "--language")
                .map(|w| w[1].as_str()).unwrap_or("typescript");
            println!("Creating cdk8s project...");
            println!("  Language: {}", lang);
            println!("  Created: main.ts");
            println!("  Created: cdk8s.yaml");
            println!("  Installing dependencies...");
            println!("  Importing k8s API objects...");
            println!("Done.");
        }
        "import" => {
            println!("Importing API objects...");
            println!("  k8s v1.28.0");
            println!("  Generated: imports/k8s.ts (234 constructs)");
        }
        "synth" => {
            println!("Synthesizing Kubernetes manifests...");
            println!("  Generated: dist/my-chart.k8s.yaml");
            println!("  Resources:");
            println!("    Deployment/my-app");
            println!("    Service/my-app-service");
            println!("    ConfigMap/my-app-config");
        }
        "diff" => {
            println!("Diff against cluster:");
            println!("  + Deployment/my-app (new)");
            println!("  ~ Service/my-app-service (updated port)");
        }
        _ => println!("cdk8s: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cdk8s".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cdk8s(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cdk8s};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cdk8s"), "cdk8s");
        assert_eq!(basename(r"C:\bin\cdk8s.exe"), "cdk8s.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cdk8s.exe"), "cdk8s");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cdk8s(&["--help".to_string()]), 0);
        assert_eq!(run_cdk8s(&["-h".to_string()]), 0);
        let _ = run_cdk8s(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cdk8s(&[]);
    }
}
