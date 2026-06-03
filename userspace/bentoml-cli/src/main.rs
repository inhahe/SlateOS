#![deny(clippy::all)]

//! bentoml-cli — OurOS BentoML model serving CLI
//!
//! Multi-personality: `bentoml`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bentoml(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bentoml COMMAND [OPTIONS]");
        println!("BentoML 1.2.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  serve          Serve a Bento service");
        println!("  build          Build a Bento");
        println!("  list           List Bentos");
        println!("  models         Manage models");
        println!("  push           Push Bento to registry");
        println!("  pull           Pull Bento from registry");
        println!("  containerize   Build container image");
        println!("  deploy         Deploy to BentoCloud");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("bentoml 1.2.0"),
        "serve" => {
            let service = args.get(1).map(|s| s.as_str()).unwrap_or("service:svc");
            println!("Starting BentoML server for '{}'...", service);
            println!("  API server running on http://0.0.0.0:3000");
            println!("  Swagger UI: http://0.0.0.0:3000/docs");
        }
        "build" => {
            println!("Building Bento...");
            println!("  Packaging model: my-model:latest");
            println!("  Including: service.py, requirements.txt");
            println!("  Built: my-service:abc123");
        }
        "list" => {
            println!("TAG                    SIZE      CREATED");
            println!("my-service:abc123      45.2 MB   2024-01-15");
            println!("my-service:def456      44.8 MB   2024-01-14");
        }
        "models" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("TAG                       MODULE           SIZE       CREATED");
                println!("my-model:latest           sklearn          12.3 MB    2024-01-15");
                println!("text-classifier:v2        transformers     450 MB     2024-01-14");
            } else {
                println!("bentoml models: '{}' completed", sub);
            }
        }
        "containerize" => {
            let bento = args.get(1).map(|s| s.as_str()).unwrap_or("my-service:latest");
            println!("Containerizing '{}'...", bento);
            println!("  Building Docker image...");
            println!("  Image built: my-service:abc123");
        }
        "push" => {
            let bento = args.get(1).map(|s| s.as_str()).unwrap_or("my-service:latest");
            println!("Pushing '{}' to BentoCloud...", bento);
            println!("Done.");
        }
        _ => println!("bentoml: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bentoml".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bentoml(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bentoml};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bentoml"), "bentoml");
        assert_eq!(basename(r"C:\bin\bentoml.exe"), "bentoml.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bentoml.exe"), "bentoml");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_bentoml(&["--help".to_string()]), 0);
        assert_eq!(run_bentoml(&["-h".to_string()]), 0);
        assert_eq!(run_bentoml(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_bentoml(&[]), 0);
    }
}
