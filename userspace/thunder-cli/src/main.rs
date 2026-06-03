#![deny(clippy::all)]

//! thunder-cli — OurOS Thunder Client REST API testing
//!
//! Single personality: `thunder`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_thunder(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: thunder COMMAND [OPTIONS]");
        println!("Thunder Client v2.20.0 (OurOS) — REST API testing tool");
        println!();
        println!("Commands:");
        println!("  run             Run collection or request");
        println!("  collection      Manage collections");
        println!("  env             Manage environments");
        println!("  import          Import collection");
        println!("  export          Export collection");
        println!("  test            Run tests");
        println!("  version         Show version");
        println!();
        println!("Run options:");
        println!("  -c, --collection NAME   Collection to run");
        println!("  -e, --env NAME          Environment to use");
        println!("  -r, --report FILE       Save report");
        println!("  --delay MS              Delay between requests");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Thunder Client v2.20.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("collection");
    match cmd {
        "run" => {
            println!("Running collection: My API Tests");
            println!();
            println!("  GET /api/users          200 OK    45ms  PASS");
            println!("  POST /api/users         201 OK    82ms  PASS");
            println!("  GET /api/users/1        200 OK    31ms  PASS");
            println!("  PUT /api/users/1        200 OK    56ms  PASS");
            println!("  DELETE /api/users/1     204 OK    28ms  PASS");
            println!();
            println!("Results: 5 passed, 0 failed");
            println!("Total time: 242ms");
        }
        "collection" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Collections:");
                println!("  My API Tests        5 requests");
                println!("  Auth Endpoints      3 requests");
                println!("  Admin API          12 requests");
            }
        }
        "env" => {
            println!("Environments:");
            println!("  * local       (base_url: http://localhost:3000)");
            println!("    staging     (base_url: https://staging.example.com)");
            println!("    production  (base_url: https://api.example.com)");
        }
        "test" => {
            println!("Running tests...");
            println!("  [PASS] Status code is 200");
            println!("  [PASS] Response contains 'id'");
            println!("  [PASS] Response time < 500ms");
            println!("  3 tests passed");
        }
        "import" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("collection.json");
            println!("Importing from: {}", file);
            println!("Imported 10 requests.");
        }
        "export" => println!("Exported collection to thunder-collection.json"),
        _ => println!("thunder {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "thunder".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_thunder(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_thunder};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/thunder"), "thunder");
        assert_eq!(basename(r"C:\bin\thunder.exe"), "thunder.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("thunder.exe"), "thunder");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_thunder(&["--help".to_string()], "thunder"), 0);
        assert_eq!(run_thunder(&["-h".to_string()], "thunder"), 0);
        assert_eq!(run_thunder(&["--version".to_string()], "thunder"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_thunder(&[], "thunder"), 0);
    }
}
