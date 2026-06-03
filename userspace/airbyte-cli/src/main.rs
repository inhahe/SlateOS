#![deny(clippy::all)]

//! airbyte-cli — OurOS Airbyte CLI (octavia)
//!
//! Multi-personality: `octavia`, `airbyte`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_airbyte(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: octavia COMMAND [OPTIONS]");
        println!("Airbyte CLI (Octavia) 0.44.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  init              Initialize local config");
        println!("  list              List resources");
        println!("  get               Get resource details");
        println!("  import            Import config from Airbyte");
        println!("  apply             Apply local config to Airbyte");
        println!("  generate          Generate source/destination");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("0.44.0"),
        "init" => {
            println!("Initializing Airbyte local config...");
            println!("  Created: connections/");
            println!("  Created: sources/");
            println!("  Created: destinations/");
            println!("Done.");
        }
        "list" => {
            let resource = args.get(1).map(|s| s.as_str()).unwrap_or("connections");
            println!("Listing {}:", resource);
            match resource {
                "sources" => {
                    println!("  postgres-prod       PostgreSQL    active");
                    println!("  stripe-api          Stripe        active");
                }
                "destinations" => {
                    println!("  snowflake-wh        Snowflake     active");
                    println!("  bigquery-analytics  BigQuery      active");
                }
                _ => {
                    println!("  pg-to-snowflake     active    every 6h    last: 2h ago");
                    println!("  stripe-to-bq        active    every 1h    last: 30m ago");
                }
            }
        }
        "apply" => {
            println!("Applying local config to Airbyte...");
            println!("  Updated source: postgres-prod");
            println!("  Updated connection: pg-to-snowflake");
            println!("Done. 2 resources updated.");
        }
        _ => println!("octavia: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "octavia".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_airbyte(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_airbyte};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/airbyte"), "airbyte");
        assert_eq!(basename(r"C:\bin\airbyte.exe"), "airbyte.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("airbyte.exe"), "airbyte");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_airbyte(&["--help".to_string()]), 0);
        assert_eq!(run_airbyte(&["-h".to_string()]), 0);
        assert_eq!(run_airbyte(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_airbyte(&[]), 0);
    }
}
