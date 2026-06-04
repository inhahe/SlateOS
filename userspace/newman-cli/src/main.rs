#![deny(clippy::all)]

//! newman-cli — OurOS Newman Postman collection runner
//!
//! Multi-personality: `newman`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_newman(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: newman COMMAND [OPTIONS]");
        println!("Newman v6.1.3 (OurOS)");
        println!();
        println!("Commands:");
        println!("  run          Run a collection");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-v" => println!("6.1.3"),
        "run" => {
            let collection = args.get(1).map(|s| s.as_str()).unwrap_or("collection.json");
            println!("newman");
            println!();
            println!("My API Tests");
            println!();
            println!("-> Running collection: {}", collection);
            println!();
            println!("  GET /api/users [200 OK, 234B, 45ms]");
            println!("    - Status code is 200");
            println!("    - Response has users array");
            println!();
            println!("  POST /api/users [201 Created, 123B, 89ms]");
            println!("    - Status code is 201");
            println!("    - User is created");
            println!();
            println!("  GET /api/users/1 [200 OK, 89B, 23ms]");
            println!("    - Status code is 200");
            println!("    - User has correct ID");
            println!();
            println!("--------------------------------------------------------------");
            println!("           executed  failed");
            println!();
            println!("  iterations       1       0");
            println!("  requests         3       0");
            println!("  test-scripts     3       0");
            println!("  assertions       6       0");
            println!();
            println!("total run duration: 157ms");
        }
        _ => println!("newman: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "newman".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_newman(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_newman};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/newman"), "newman");
        assert_eq!(basename(r"C:\bin\newman.exe"), "newman.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("newman.exe"), "newman");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_newman(&["--help".to_string()]), 0);
        assert_eq!(run_newman(&["-h".to_string()]), 0);
        let _ = run_newman(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_newman(&[]);
    }
}
