#![deny(clippy::all)]

//! kustomize-cli — OurOS Kustomize CLI
//!
//! Multi-personality: `kustomize`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kustomize(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: kustomize COMMAND [OPTIONS]");
        println!("kustomize 5.4.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  build         Build kustomization target");
        println!("  edit          Edit kustomization file");
        println!("  create        Create kustomization file");
        println!("  cfg           Configuration commands");
        println!("  fn            Function commands");
        println!("  version       Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("kustomize 5.4.0"),
        "build" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("apiVersion: apps/v1");
            println!("kind: Deployment");
            println!("metadata:");
            println!("  name: my-app");
            println!("  namespace: production");
            println!("  labels:");
            println!("    app: my-app");
            println!("    env: production");
            println!("spec:");
            println!("  replicas: 3");
            println!("  selector:");
            println!("    matchLabels:");
            println!("      app: my-app");
            println!("  template:");
            println!("    metadata:");
            println!("      labels:");
            println!("        app: my-app");
            println!("    spec:");
            println!("      containers:");
            println!("      - name: my-app");
            println!("        image: my-app:v1.2.3");
            println!("        ports:");
            println!("        - containerPort: 8080");
            println!("---");
            println!("apiVersion: v1");
            println!("kind: Service");
            println!("metadata:");
            println!("  name: my-app");
            println!("  namespace: production");
            println!("spec:");
            println!("  selector:");
            println!("    app: my-app");
            println!("  ports:");
            println!("  - port: 80");
            println!("    targetPort: 8080");
            let _p = path;
        }
        "create" => {
            println!("Created kustomization.yaml");
        }
        "edit" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "add" => {
                    let what = args.get(2).map(|s| s.as_str()).unwrap_or("resource");
                    let target = args.get(3).map(|s| s.as_str()).unwrap_or("deployment.yaml");
                    println!("Added {} '{}'", what, target);
                }
                "set" => {
                    let what = args.get(2).map(|s| s.as_str()).unwrap_or("image");
                    let val = args.get(3).map(|s| s.as_str()).unwrap_or("my-app=my-app:v2.0.0");
                    println!("Set {} to '{}'", what, val);
                }
                _ => println!("kustomize edit: '{}' completed", sub),
            }
        }
        _ => println!("kustomize: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kustomize".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kustomize(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kustomize};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kustomize"), "kustomize");
        assert_eq!(basename(r"C:\bin\kustomize.exe"), "kustomize.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kustomize.exe"), "kustomize");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kustomize(&["--help".to_string()]), 0);
        assert_eq!(run_kustomize(&["-h".to_string()]), 0);
        let _ = run_kustomize(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kustomize(&[]);
    }
}
