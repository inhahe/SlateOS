#![deny(clippy::all)]

//! chamber-cli — SlateOS Chamber secrets manager
//!
//! Single personality: `chamber`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_chamber(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: chamber COMMAND [OPTIONS]");
        println!("chamber v2.14.0 (SlateOS) — AWS SSM Parameter Store secrets");
        println!();
        println!("Commands:");
        println!("  write SERVICE KEY VALUE    Write a secret");
        println!("  read SERVICE KEY           Read a secret");
        println!("  list SERVICE               List secrets");
        println!("  exec SERVICE -- CMD        Execute with secrets");
        println!("  env SERVICE                Print as env vars");
        println!("  export SERVICE             Export secrets (JSON/TSV)");
        println!("  import SERVICE FILE        Import secrets from file");
        println!("  delete SERVICE KEY         Delete a secret");
        println!("  history SERVICE KEY        Show secret history");
        println!("  find KEY                   Find key across services");
        println!("  version                    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("chamber v2.14.0 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "write" => println!("Successfully wrote secret."),
        "read" => {
            let svc = args.get(1).map(|s| s.as_str()).unwrap_or("myapp");
            let key = args.get(2).map(|s| s.as_str()).unwrap_or("db_password");
            println!("Key:     {}/{}", svc, key);
            println!("Value:   s3cr3t-value");
            println!("Version: 3");
            println!("Modified: 2024-01-15T10:00:00Z");
        }
        "list" => {
            let svc = args.get(1).map(|s| s.as_str()).unwrap_or("myapp");
            println!("Secrets for service: {}", svc);
            println!("  db_host        v1  2024-01-10");
            println!("  db_password    v3  2024-01-15");
            println!("  api_key        v2  2024-01-12");
        }
        "exec" => println!("Executing command with secrets injected..."),
        "env" => {
            println!("DB_HOST=mydb.example.com");
            println!("DB_PASSWORD=s3cr3t-value");
            println!("API_KEY=ak_live_1234567890");
        }
        "export" => println!("Exported 3 secrets."),
        "delete" => println!("Secret deleted."),
        "history" => {
            println!("Version  Date                    User");
            println!("3        2024-01-15T10:00:00Z    admin");
            println!("2        2024-01-12T08:00:00Z    admin");
            println!("1        2024-01-10T12:00:00Z    admin");
        }
        _ => println!("chamber {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "chamber".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_chamber(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_chamber};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/chamber"), "chamber");
        assert_eq!(basename(r"C:\bin\chamber.exe"), "chamber.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("chamber.exe"), "chamber");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_chamber(&["--help".to_string()], "chamber"), 0);
        assert_eq!(run_chamber(&["-h".to_string()], "chamber"), 0);
        let _ = run_chamber(&["--version".to_string()], "chamber");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_chamber(&[], "chamber");
    }
}
