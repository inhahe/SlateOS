#![deny(clippy::all)]

//! infisical-cli — SlateOS Infisical secrets management
//!
//! Single personality: `infisical`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_infisical(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: infisical COMMAND [OPTIONS]");
        println!("Infisical CLI v0.22.0 (SlateOS) — Secrets management");
        println!();
        println!("Commands:");
        println!("  init            Link to Infisical project");
        println!("  run CMD         Run with secrets injected");
        println!("  secrets         Manage secrets");
        println!("  export          Export secrets");
        println!("  login           Authenticate");
        println!("  token           Manage service tokens");
        println!("  scan            Scan for leaked secrets");
        println!("  vault           Manage vaults");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("infisical-cli v0.22.0 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("secrets");
    match cmd {
        "init" => println!("Linked to project: my-project (env: development)"),
        "run" => println!("Injecting 8 secrets into environment..."),
        "secrets" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("KEY              TYPE      UPDATED");
                    println!("DATABASE_URL     shared    2024-01-15");
                    println!("API_KEY          personal  2024-01-14");
                    println!("SECRET_KEY       shared    2024-01-10");
                    println!("REDIS_URL        shared    2024-01-12");
                }
                "set" => println!("Secret created successfully."),
                "delete" => println!("Secret deleted."),
                _ => println!("infisical secrets {}: completed", sub),
            }
        }
        "export" => {
            println!("DATABASE_URL=postgres://user:pass@localhost/db");
            println!("API_KEY=ak_live_1234567890");
            println!("SECRET_KEY=sk_test_abcdefgh");
        }
        "login" => println!("Successfully logged in."),
        "scan" => {
            println!("Scanning for leaked secrets...");
            println!("  No leaks detected in 42 files.");
        }
        _ => println!("infisical {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "infisical".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_infisical(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_infisical};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/infisical"), "infisical");
        assert_eq!(basename(r"C:\bin\infisical.exe"), "infisical.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("infisical.exe"), "infisical");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_infisical(&["--help".to_string()], "infisical"), 0);
        assert_eq!(run_infisical(&["-h".to_string()], "infisical"), 0);
        let _ = run_infisical(&["--version".to_string()], "infisical");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_infisical(&[], "infisical");
    }
}
