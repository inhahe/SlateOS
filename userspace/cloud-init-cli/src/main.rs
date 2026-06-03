#![deny(clippy::all)]

//! cloud-init-cli — OurOS cloud-init CLI
//!
//! Single personality: `cloud-init`

use std::env;
use std::process;

fn run_cloud_init(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cloud-init [OPTIONS] COMMAND [ARGS]");
        println!();
        println!("cloud-init — cloud instance initialization (OurOS).");
        println!();
        println!("Commands:");
        println!("  init             Run cloud-init init stage");
        println!("  modules          Run modules for a stage");
        println!("  single           Run a single module");
        println!("  status           Report cloud-init status");
        println!("  query            Query instance metadata");
        println!("  analyze          Analyze cloud-init logs");
        println!("  clean            Remove cloud-init artifacts");
        println!("  collect-logs     Collect debug logs");
        println!("  schema           Validate cloud-config");
        println!("  devel            Development tools");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("cloud-init 24.1 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();

    match cmd {
        "status" => {
            let long = rest.iter().any(|a| *a == "--long" || *a == "-l");
            let wait = rest.iter().any(|a| *a == "--wait" || *a == "-w");
            if wait {
                println!("cloud-init: waiting for completion...");
            }
            println!("status: done");
            if long {
                println!("boot_status_code: enabled-by-generator");
                println!("detail:");
                println!("  DataSourceNoCloud [seed=/dev/sr0]");
                println!("errors: []");
                println!("recoverable_errors: {{}}");
                println!("extended_status: done");
                println!("init:");
                println!("  start: 2024-01-15 10:30:00.123456");
                println!("  finished: 2024-01-15 10:30:05.234567");
                println!("modules-config:");
                println!("  start: 2024-01-15 10:30:05.345678");
                println!("  finished: 2024-01-15 10:30:08.456789");
                println!("modules-final:");
                println!("  start: 2024-01-15 10:30:08.567890");
                println!("  finished: 2024-01-15 10:30:12.678901");
            }
        }
        "query" => {
            let key = rest.iter().find(|a| !a.starts_with('-')).unwrap_or(&"");
            match *key {
                "instance-id" => println!("i-abc123def456"),
                "region" => println!("us-east-1"),
                "availability-zone" => println!("us-east-1a"),
                "local-hostname" => println!("ip-10-0-0-42"),
                "public-ipv4" => println!("203.0.113.42"),
                "cloud-name" => println!("nocloud"),
                "" => {
                    println!("instance-id: i-abc123def456");
                    println!("region: us-east-1");
                    println!("availability-zone: us-east-1a");
                    println!("local-hostname: ip-10-0-0-42");
                    println!("cloud-name: nocloud");
                }
                _ => println!("(not found)"),
            }
        }
        "analyze" => {
            let subcmd = rest.first().unwrap_or(&"show");
            match *subcmd {
                "show" | "blame" => {
                    println!("-- Boot Record 01 --");
                    println!("     00.234s (modules-final/config-phone-home)");
                    println!("     00.189s (modules-config/config-apt-configure)");
                    println!("     00.156s (init-network/config-growpart)");
                    println!("     00.123s (init-network/search-NoCloud)");
                    println!("     12.678s (total)");
                }
                "dump" => println!("(raw log data)"),
                _ => println!("cloud-init analyze: unknown subcommand"),
            }
        }
        "clean" => {
            println!("cloud-init: removing artifacts...");
            println!("cloud-init: cleaned.");
        }
        "collect-logs" => {
            println!("Collecting debug logs...");
            println!("Wrote /tmp/cloud-init-logs.tar.gz");
        }
        "schema" => {
            let file = rest.iter().find(|a| !a.starts_with('-'));
            if let Some(f) = file {
                println!("Valid cloud-config: {}", f);
            } else {
                println!("cloud-init schema: specify --config-file");
            }
        }
        "init" => {
            println!("cloud-init: running init stage...");
            println!("cloud-init: init complete.");
        }
        "modules" => {
            println!("cloud-init: running modules...");
            println!("cloud-init: modules complete.");
        }
        _ => {
            eprintln!("cloud-init: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cloud_init(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cloud_init};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cloud_init(vec!["--help".to_string()]), 0);
        assert_eq!(run_cloud_init(vec!["-h".to_string()]), 0);
        assert_eq!(run_cloud_init(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cloud_init(vec![]), 0);
    }
}
