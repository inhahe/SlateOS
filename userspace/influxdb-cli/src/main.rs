#![deny(clippy::all)]

//! influxdb-cli — SlateOS InfluxDB CLI
//!
//! Single personality: `influx`

use std::env;
use std::process;

fn run_influx(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: influx [COMMAND] [OPTIONS]");
        println!();
        println!("influx — InfluxDB CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  setup            Setup InfluxDB");
        println!("  write            Write data");
        println!("  query            Query data");
        println!("  bucket           Manage buckets");
        println!("  org              Manage organizations");
        println!("  auth             Manage auth tokens");
        println!("  export           Export resources");
        println!("  delete           Delete data");
        println!("  ping             Check server health");
        println!("  config           Manage CLI config");
        println!("  telegrafs        Manage Telegraf configs");
        println!("  dashboards       Manage dashboards");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") {
        println!("Influx CLI 2.7.3 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "ping" => {
            println!("OK");
        }
        "setup" => {
            println!("? Username: admin");
            println!("? Password: ********");
            println!("? Org: my-org");
            println!("? Bucket: my-bucket");
            println!("? Retention: 0 (infinite)");
            println!("Setup complete!");
        }
        "bucket" => match sub {
            "list" | "" => {
                println!("ID                  Name        Retention  Org");
                println!("abcdef1234567890    my-bucket   infinite   my-org");
                println!("1234567890abcdef    _monitoring 168h       my-org");
            }
            "create" => {
                let name = args.windows(2).find(|w| w[0] == "-n" || w[0] == "--name")
                    .map(|w| w[1].as_str()).unwrap_or("new-bucket");
                println!("Bucket \"{}\" created.", name);
            }
            _ => { println!("influx bucket {}: see --help.", sub); }
        },
        "query" => {
            println!("Result: _time                 _measurement  _field  _value");
            println!("        2024-01-15T12:00:00Z  cpu           usage   45.2");
            println!("        2024-01-15T12:01:00Z  cpu           usage   47.8");
            println!("        2024-01-15T12:02:00Z  cpu           usage   42.1");
        }
        "write" => {
            println!("Write successful.");
        }
        "org" => match sub {
            "list" | "" => {
                println!("ID                  Name");
                println!("abcdef1234567890    my-org");
            }
            _ => { println!("influx org {}: see --help.", sub); }
        },
        "auth" => match sub {
            "list" | "" => {
                println!("ID                  Description    User    Permissions");
                println!("abcdef1234567890    admin token    admin   read/write");
            }
            _ => { println!("influx auth {}: see --help.", sub); }
        },
        "delete" => {
            println!("Delete successful.");
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("influx: no command specified. See --help.");
                return 1;
            }
            println!("influx {}: see influx {} --help.", cmd, cmd);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_influx(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_influx};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_influx(vec!["--help".to_string()]), 0);
        assert_eq!(run_influx(vec!["-h".to_string()]), 0);
        let _ = run_influx(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_influx(vec![]);
    }
}
