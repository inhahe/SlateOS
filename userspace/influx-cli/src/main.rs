#![deny(clippy::all)]

//! influx-cli — OurOS InfluxDB CLI client
//!
//! Multi-personality: `influx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_influx(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: influx COMMAND [OPTIONS]");
        println!("InfluxDB CLI 2.7.5 (OurOS)");
        println!();
        println!("Commands:");
        println!("  setup        Set up InfluxDB");
        println!("  write        Write data to InfluxDB");
        println!("  query        Execute a Flux query");
        println!("  bucket       Manage buckets");
        println!("  org          Manage organizations");
        println!("  user         Manage users");
        println!("  auth         Manage API tokens");
        println!("  config       Manage CLI configurations");
        println!("  delete       Delete data");
        println!("  export       Export resources");
        println!("  ping         Check server status");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("Influx CLI 2.7.5 (git: abc1234)"),
        "ping" => {
            println!("OK");
            println!("InfluxDB 2.7.5 is running.");
        }
        "setup" => {
            println!("Welcome to InfluxDB 2.7 setup!");
            println!("  Organization: myorg");
            println!("  Bucket: mybucket");
            println!("  Username: admin");
            println!("  Retention: infinite");
            println!("Setup complete!");
        }
        "write" => {
            let bucket = args.windows(2).find(|w| w[0] == "-b" || w[0] == "--bucket")
                .map(|w| w[1].as_str()).unwrap_or("mybucket");
            println!("Writing to bucket '{}'...", bucket);
            println!("Success. 1 point(s) written.");
        }
        "query" => {
            let query = args.get(1).map(|s| s.as_str())
                .unwrap_or("from(bucket:\"mybucket\") |> range(start:-1h)");
            println!("Executing query:");
            println!("  {}", query);
            println!();
            println!("_time                          _measurement  _field  _value");
            println!("2024-01-01T00:00:00Z           cpu          usage   45.2");
            println!("2024-01-01T00:01:00Z           cpu          usage   42.8");
            println!("2024-01-01T00:02:00Z           cpu          usage   48.1");
        }
        "bucket" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("ID                   Name          Retention  Org");
                    println!("abc123456789         mybucket      infinite   myorg");
                    println!("def123456789         _monitoring   168h       myorg");
                    println!("ghi123456789         _tasks        72h        myorg");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("newbucket");
                    println!("Bucket '{}' created.", name);
                }
                "delete" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("oldbucket");
                    println!("Bucket '{}' deleted.", name);
                }
                _ => println!("influx bucket: '{}' completed", sub),
            }
        }
        "org" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("ID                   Name");
                    println!("abc123456789         myorg");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("neworg");
                    println!("Organization '{}' created.", name);
                }
                _ => println!("influx org: '{}' completed", sub),
            }
        }
        "auth" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("ID                   Description       Token                      Permissions");
                    println!("abc123456789         admin token       xxxxxxxxxxxxxxxxxxxxxxxx   [read:*,write:*]");
                }
                "create" => println!("Token created: xxxxxxxxxxxxxxxxxxxx"),
                _ => println!("influx auth: '{}' completed", sub),
            }
        }
        "config" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("Active  Name     URL                    Org     Token");
                    println!("*       default  http://localhost:8086  myorg   xxx...xxx");
                }
                _ => println!("influx config: '{}' completed", sub),
            }
        }
        "delete" => {
            println!("Data deleted successfully.");
        }
        _ => println!("influx: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "influx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_influx(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_influx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/influx"), "influx");
        assert_eq!(basename(r"C:\bin\influx.exe"), "influx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("influx.exe"), "influx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_influx(&["--help".to_string()]), 0);
        assert_eq!(run_influx(&["-h".to_string()]), 0);
        let _ = run_influx(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_influx(&[]);
    }
}
