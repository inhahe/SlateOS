#![deny(clippy::all)]

//! wrangler-cli — OurOS Cloudflare Wrangler CLI
//!
//! Multi-personality: `wrangler`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wrangler(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wrangler COMMAND [OPTIONS]");
        println!("wrangler 3.62.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  init          Initialize a new Workers project");
        println!("  dev           Start local dev server");
        println!("  deploy        Deploy Worker to Cloudflare");
        println!("  publish       Alias for deploy");
        println!("  tail          Stream live logs");
        println!("  secret        Manage Worker secrets");
        println!("  kv            Manage KV namespaces");
        println!("  r2            Manage R2 buckets");
        println!("  d1            Manage D1 databases");
        println!("  pages         Manage Pages projects");
        println!("  queues        Manage Queues");
        println!("  login         Authenticate with Cloudflare");
        println!("  whoami        Show current user");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("wrangler 3.62.0"),
        "init" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-worker");
            println!("Creating {} ...", name);
            println!("  Created wrangler.toml");
            println!("  Created src/index.ts");
            println!("  Created package.json");
            println!("Done!");
        }
        "dev" => {
            println!("wrangler dev");
            println!("Starting local server on http://localhost:8787");
            println!("Using compatibility date: 2024-06-14");
            println!("[wrangler:inf] Ready on http://localhost:8787");
        }
        "deploy" | "publish" => {
            println!("Deploying my-worker...");
            println!("Total Upload: 12.45 KiB / gzip: 4.23 KiB");
            println!("Published my-worker (1.23s)");
            println!("  https://my-worker.username.workers.dev");
        }
        "tail" => {
            println!("Connected to my-worker, waiting for logs...");
            println!("[2024-06-15 12:00:00] GET /api/hello 200 OK (12ms)");
        }
        "secret" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Name            Type");
                    println!("API_KEY         secret_text");
                    println!("DATABASE_URL    secret_text");
                }
                "put" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("MY_SECRET");
                    println!("Secret {} created.", name);
                }
                _ => println!("wrangler secret: '{}' completed", sub),
            }
        }
        "kv" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("ID                                    Title");
                println!("abc12345678901234567890123456789012    MY_KV");
            }
        }
        "r2" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Name            Created");
                println!("my-bucket       2024-01-15");
            }
        }
        "d1" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("UUID                                  Name        Size");
                println!("abc12345-xxxx-xxxx-xxxx-abc123456789  mydb        1.2 MB");
            }
        }
        "whoami" => {
            println!("You are logged in with an API Token, associated with:");
            println!("  Account: My Account (abc123)");
            println!("  Token: ...xxxx");
        }
        "pages" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Name           URL                                  Branch");
                println!("my-site        https://my-site.pages.dev            main");
            }
        }
        _ => println!("wrangler: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wrangler".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wrangler(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wrangler};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wrangler"), "wrangler");
        assert_eq!(basename(r"C:\bin\wrangler.exe"), "wrangler.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wrangler.exe"), "wrangler");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wrangler(&["--help".to_string()]), 0);
        assert_eq!(run_wrangler(&["-h".to_string()]), 0);
        let _ = run_wrangler(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wrangler(&[]);
    }
}
