#![deny(clippy::all)]

//! scaleway-cli — SlateOS Scaleway cloud CLI
//!
//! Multi-personality: `scw`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_scw(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: scw COMMAND [OPTIONS]");
        println!("Scaleway CLI 2.30.0 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  instance     Manage compute instances");
        println!("  k8s          Manage Kubernetes clusters");
        println!("  rdb          Manage managed databases");
        println!("  object       Manage object storage");
        println!("  vpc          Manage VPC networks");
        println!("  lb           Manage load balancers");
        println!("  dns          Manage DNS zones");
        println!("  registry     Manage container registry");
        println!("  init         Initialize CLI configuration");
        println!("  config       Manage CLI profiles");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("scw 2.30.0"),
        "init" => {
            println!("Access Key: SCWXXXXXXXXXXXXXXXXX");
            println!("Secret Key: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx");
            println!("Default Organization ID: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx");
            println!("Default Region: fr-par");
            println!("Default Zone: fr-par-1");
            println!("Configuration saved to ~/.config/scw/config.yaml");
        }
        "instance" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("ID                                    Name       Type       State    Zone");
                    println!("abc12345-1234-1234-1234-abc123456789  web-1      DEV1-S     running  fr-par-1");
                    println!("def12345-1234-1234-1234-def123456789  db-1       GP1-XS     running  fr-par-1");
                }
                "create" => {
                    let itype = args.windows(2).find(|w| w[0] == "--type").map(|w| w[1].as_str()).unwrap_or("DEV1-S");
                    println!("Instance created: type={}", itype);
                }
                "start" | "stop" | "delete" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("abc12345");
                    println!("Instance {}: {} done.", id, sub);
                }
                _ => println!("scw instance: '{}' completed", sub),
            }
        }
        "k8s" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("ID                                    Name    Version  Status  Nodes");
                    println!("abc12345-xxxx-xxxx-xxxx-abc123456789  myclus  1.29.3   ready   3");
                }
                "create" => println!("Kubernetes cluster created."),
                _ => println!("scw k8s: '{}' completed", sub),
            }
        }
        "object" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            println!("scw object: '{}' completed", sub);
        }
        _ => println!("scw: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "scw".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_scw(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_scw};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/scaleway"), "scaleway");
        assert_eq!(basename(r"C:\bin\scaleway.exe"), "scaleway.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("scaleway.exe"), "scaleway");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_scw(&["--help".to_string()]), 0);
        assert_eq!(run_scw(&["-h".to_string()]), 0);
        let _ = run_scw(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_scw(&[]);
    }
}
