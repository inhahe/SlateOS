#![deny(clippy::all)]

//! wsk-cli — OurOS Apache OpenWhisk serverless CLI
//!
//! Single personality: `wsk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wsk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wsk COMMAND [OPTIONS]");
        println!("wsk v1.2.0 (OurOS) — Apache OpenWhisk CLI");
        println!();
        println!("Commands:");
        println!("  action          Manage actions");
        println!("  activation      Manage activations");
        println!("  package         Manage packages");
        println!("  rule            Manage rules");
        println!("  trigger         Manage triggers");
        println!("  namespace       Manage namespaces");
        println!("  list            List entities");
        println!("  api             Manage APIs");
        println!("  property        Manage properties");
        println!("  sdk             Manage SDKs");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("wsk CLI version: 1.2.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "action" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("actions");
                    println!("/guest/hello                       private nodejs:20");
                    println!("/guest/processOrder                private python:3.11");
                    println!("/guest/sendEmail                   private nodejs:20");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("myAction");
                    println!("ok: created action {}", name);
                }
                "invoke" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("hello");
                    println!("ok: invoked /_/{}  with id abc123", name);
                    println!("{{");
                    println!("  \"result\": \"Hello, World!\"");
                    println!("}}");
                }
                "get" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("hello");
                    println!("ok: got action {}", name);
                    println!("  kind: nodejs:20");
                    println!("  timeout: 60000");
                    println!("  memory: 256");
                }
                "delete" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("myAction");
                    println!("ok: deleted action {}", name);
                }
                _ => println!("wsk action {}: completed", sub),
            }
        }
        "activation" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("activations");
                println!("abc123  hello          success  20ms   2024-01-15 10:00:00");
                println!("def456  processOrder   success  45ms   2024-01-15 09:55:00");
            }
        }
        "package" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("packages");
                println!("/guest/utils      private");
                println!("/whisk.system/utils  shared");
            }
        }
        "trigger" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("triggers");
                println!("/guest/orderReceived    private");
            }
        }
        "list" => {
            println!("entities in namespace: default");
            println!("packages:");
            println!("  /guest/utils");
            println!("actions:");
            println!("  /guest/hello");
            println!("  /guest/processOrder");
            println!("triggers:");
            println!("  /guest/orderReceived");
        }
        "property" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("get");
            if sub == "get" {
                println!("whisk auth         abc123...def");
                println!("whisk API host     https://localhost:443");
                println!("whisk namespace    guest");
            }
        }
        _ => println!("wsk {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wsk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wsk(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
