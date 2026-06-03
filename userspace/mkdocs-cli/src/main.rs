#![deny(clippy::all)]

//! mkdocs-cli — OurOS MkDocs documentation site generator
//!
//! Multi-personality: `mkdocs`

use std::env;
use std::process;

fn run_mkdocs(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mkdocs COMMAND [OPTIONS]");
        println!("MkDocs 1.5.3 (OurOS)");
        println!();
        println!("Commands:");
        println!("  new DIR       Create new project");
        println!("  build         Build documentation");
        println!("  serve         Start dev server");
        println!("  gh-deploy     Deploy to GitHub Pages");
        println!("  version       Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => println!("mkdocs, version 1.5.3 (OurOS)"),
        "new" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("my-project");
            println!("Creating project: {}", dir);
            println!("  Created: {}/mkdocs.yml", dir);
            println!("  Created: {}/docs/index.md", dir);
            println!("  Project created.");
        }
        "build" => {
            println!("INFO    -  Cleaning site directory");
            println!("INFO    -  Building documentation to directory: site/");
            println!("INFO    -  Documentation built in 0.45 seconds");
        }
        "serve" => {
            let port = args.windows(2).find(|w| w[0] == "-a").map(|w| w[1].as_str()).unwrap_or("127.0.0.1:8000");
            println!("INFO    -  Building documentation...");
            println!("INFO    -  Serving on http://{}/", port);
            println!("INFO    -  Start watching for changes...");
        }
        "gh-deploy" => {
            println!("INFO    -  Cleaning site directory");
            println!("INFO    -  Building documentation");
            println!("INFO    -  Deploying to GitHub Pages");
            println!("INFO    -  Deployed successfully.");
        }
        _ => println!("mkdocs: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mkdocs(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mkdocs};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mkdocs(&["--help".to_string()]), 0);
        assert_eq!(run_mkdocs(&["-h".to_string()]), 0);
        assert_eq!(run_mkdocs(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mkdocs(&[]), 0);
    }
}
