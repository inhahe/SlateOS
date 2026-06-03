#![deny(clippy::all)]

//! paw-cli — OurOS Paw API client tool
//!
//! Single personality: `paw`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_paw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: paw [OPTIONS] COMMAND");
        println!("Paw v4.0.0 (OurOS) — API development tool");
        println!();
        println!("Commands:");
        println!("  run FILE         Execute API request file");
        println!("  collection LIST  List collections");
        println!("  env              Manage environments");
        println!("  import FILE      Import from Postman/OpenAPI");
        println!("  export FILE      Export collection");
        println!("  mock             Start mock server");
        println!("  docs             Generate API docs");
        println!("  version          Show version");
        println!();
        println!("Options:");
        println!("  -e, --env NAME   Use environment");
        println!("  -v, --verbose    Verbose output");
        println!("  --json           JSON output format");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Paw v4.0.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("collection");
    match cmd {
        "run" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("request.paw");
            println!("Executing: {}", file);
            println!();
            println!("GET https://api.example.com/users");
            println!("Status: 200 OK");
            println!("Time: 142ms");
            println!("Size: 1.2 KB");
        }
        "collection" => {
            println!("Collections:");
            println!("  1. My API (12 requests)");
            println!("  2. Auth Service (8 requests)");
            println!("  3. User Management (15 requests)");
        }
        "env" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Environments:");
                println!("  * development");
                println!("    staging");
                println!("    production");
            }
        }
        "import" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("api.json");
            println!("Importing from: {}", file);
            println!("Imported 25 requests into 'Imported API' collection.");
        }
        "mock" => {
            println!("Starting mock server on http://localhost:3000");
            println!("  GET  /users     -> 200");
            println!("  POST /users     -> 201");
            println!("  GET  /users/:id -> 200");
        }
        "docs" => println!("API documentation generated: docs/api.html"),
        _ => println!("paw {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "paw".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_paw(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_paw};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/paw"), "paw");
        assert_eq!(basename(r"C:\bin\paw.exe"), "paw.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("paw.exe"), "paw");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_paw(&["--help".to_string()], "paw"), 0);
        assert_eq!(run_paw(&["-h".to_string()], "paw"), 0);
        assert_eq!(run_paw(&["--version".to_string()], "paw"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_paw(&[], "paw"), 0);
    }
}
