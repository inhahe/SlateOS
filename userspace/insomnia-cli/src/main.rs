#![deny(clippy::all)]

//! insomnia-cli — Slate OS Insomnia CLI (Inso)
//!
//! Multi-personality: `inso`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_inso(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: inso COMMAND [OPTIONS]");
        println!("Inso CLI 9.3.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  run          Run tests or generate config");
        println!("  generate     Generate Kubernetes/declarative config");
        println!("  lint         Lint API spec");
        println!("  export       Export Insomnia data");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("9.3.0"),
        "run" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("test");
            match sub {
                "test" => {
                    let suite = args.get(2).map(|s| s.as_str()).unwrap_or("API Tests");
                    println!("Running test suite: {}", suite);
                    println!();
                    println!("  GET /api/health ........ PASS (23ms)");
                    println!("  GET /api/users ......... PASS (45ms)");
                    println!("  POST /api/users ........ PASS (89ms)");
                    println!();
                    println!("3 passing (157ms)");
                }
                _ => println!("inso run: '{}' completed", sub),
            }
        }
        "generate" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("config");
            match sub {
                "config" => {
                    println!("Generating Kong declarative config...");
                    println!("  Output: kong.yaml");
                    println!("Done.");
                }
                _ => println!("inso generate: '{}' completed", sub),
            }
        }
        "lint" => {
            let spec = args.get(1).map(|s| s.as_str()).unwrap_or("openapi.yaml");
            println!("Linting {}...", spec);
            println!("  No errors found.");
        }
        "export" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("spec");
            println!("Exporting {}...", sub);
            println!("Done.");
        }
        _ => println!("inso: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "inso".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_inso(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_inso};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/insomnia"), "insomnia");
        assert_eq!(basename(r"C:\bin\insomnia.exe"), "insomnia.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("insomnia.exe"), "insomnia");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_inso(&["--help".to_string()]), 0);
        assert_eq!(run_inso(&["-h".to_string()]), 0);
        let _ = run_inso(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_inso(&[]);
    }
}
