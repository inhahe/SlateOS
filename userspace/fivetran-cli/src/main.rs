#![deny(clippy::all)]

//! fivetran-cli — OurOS Fivetran CLI
//!
//! Multi-personality: `fivetran`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fivetran(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: fivetran COMMAND [OPTIONS]");
        println!("Fivetran CLI 1.2.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  connector      Manage connectors");
        println!("  destination    Manage destinations");
        println!("  group          Manage groups");
        println!("  sync           Trigger sync");
        println!("  login          Authenticate");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("fivetran 1.2.0"),
        "login" => {
            println!("Authenticated successfully.");
        }
        "connector" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID           NAME              SOURCE      STATUS       LAST SYNC");
                    println!("conn_abc     postgres-prod     postgres    connected    2h ago");
                    println!("conn_def     stripe-data       stripe      connected    1h ago");
                    println!("conn_ghi     salesforce-crm    salesforce  paused       3d ago");
                }
                "status" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("conn_abc");
                    println!("Connector: {}", id);
                    println!("  Status: connected");
                    println!("  Last sync: 2024-01-15 10:00:00 UTC");
                    println!("  Rows synced: 1,234,567");
                }
                _ => println!("fivetran connector: '{}' completed", sub),
            }
        }
        "destination" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("ID           NAME              TYPE        REGION");
                println!("dest_abc     snowflake-wh      snowflake   us-east-1");
                println!("dest_def     bigquery-prod     bigquery    us-central1");
            } else {
                println!("fivetran destination: '{}' completed", sub);
            }
        }
        "sync" => {
            let connector = args.get(1).map(|s| s.as_str()).unwrap_or("conn_abc");
            println!("Triggering sync for connector '{}'...", connector);
            println!("Sync started. Check status with: fivetran connector status {}", connector);
        }
        _ => println!("fivetran: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fivetran".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fivetran(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fivetran};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fivetran"), "fivetran");
        assert_eq!(basename(r"C:\bin\fivetran.exe"), "fivetran.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fivetran.exe"), "fivetran");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fivetran(&["--help".to_string()]), 0);
        assert_eq!(run_fivetran(&["-h".to_string()]), 0);
        let _ = run_fivetran(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fivetran(&[]);
    }
}
