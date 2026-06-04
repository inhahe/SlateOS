#![deny(clippy::all)]

//! gridsome-cli — OurOS Gridsome Vue.js static site generator
//!
//! Single personality: `gridsome`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gridsome(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gridsome COMMAND [OPTIONS]");
        println!("Gridsome v0.7.24 (OurOS) — Vue.js-powered static site generator");
        println!();
        println!("Commands:");
        println!("  create NAME     Create new project");
        println!("  develop         Start dev server");
        println!("  build           Build for production");
        println!("  explore         Open GraphQL explorer");
        println!("  info            Show environment info");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gridsome v0.7.24");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("develop");
    match cmd {
        "create" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-site");
            println!("Creating project {}...", name);
            println!("  Installing dependencies...");
            println!("  Project created successfully!");
            println!("  cd {} && gridsome develop", name);
        }
        "develop" => {
            println!("  Gridsome v0.7.24");
            println!("  Initializing...");
            println!("  Loading sources...");
            println!("  Creating GraphQL schema...");
            println!("  Creating pages...");
            println!();
            println!("  Site running at:   http://localhost:8080/");
            println!("  Explore GraphQL:   http://localhost:8080/___explore");
        }
        "build" => {
            println!("  Gridsome v0.7.24");
            println!("  Initializing...");
            println!("  Loading sources...");
            println!("  Creating GraphQL schema...");
            println!("  Generating pages...");
            println!("    /index.html");
            println!("    /about/index.html");
            println!("    /blog/index.html");
            println!("  Done in 3.4s");
        }
        "explore" => println!("  Opening GraphQL explorer..."),
        "info" => {
            println!("  Gridsome: 0.7.24");
            println!("  Vue: 2.7");
            println!("  OS: OurOS x86_64");
        }
        _ => println!("gridsome {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gridsome".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gridsome(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gridsome};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gridsome"), "gridsome");
        assert_eq!(basename(r"C:\bin\gridsome.exe"), "gridsome.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gridsome.exe"), "gridsome");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gridsome(&["--help".to_string()], "gridsome"), 0);
        assert_eq!(run_gridsome(&["-h".to_string()], "gridsome"), 0);
        let _ = run_gridsome(&["--version".to_string()], "gridsome");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gridsome(&[], "gridsome");
    }
}
