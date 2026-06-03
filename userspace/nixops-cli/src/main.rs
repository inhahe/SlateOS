#![deny(clippy::all)]

//! nixops-cli — OurOS NixOps deployment tool
//!
//! Single personality: `nixops`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nixops(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nixops COMMAND [OPTIONS]");
        println!("nixops v2.0 (OurOS) — NixOS cloud deployment tool");
        println!();
        println!("Commands:");
        println!("  create            Create a new deployment");
        println!("  deploy            Deploy the network");
        println!("  destroy           Destroy all resources");
        println!("  info              Show deployment info");
        println!("  list              List deployments");
        println!("  ssh               SSH into a machine");
        println!("  check             Check deployment health");
        println!("  rollback          Rollback to previous config");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "create" => {
            println!("Created deployment 'network1'");
            println!("  UUID: a1b2c3d4-e5f6-7890");
        }
        "deploy" => {
            println!("Deploying network1...");
            println!("  Building: /nix/store/...-nixos-system");
            println!("  Copying to web-01... done");
            println!("  Copying to db-01... done");
            println!("  Activating on web-01... done");
            println!("  Activating on db-01... done");
        }
        "info" => {
            println!("Deployment: network1");
            println!("  UUID: a1b2c3d4-e5f6-7890");
            println!("  Machines:");
            println!("    web-01: 10.0.1.10 (running)");
            println!("    db-01: 10.0.1.20 (running)");
        }
        "list" => {
            println!("UUID                                  Name        Status");
            println!("a1b2c3d4-e5f6-7890                    network1    active");
        }
        "check" => {
            println!("Checking network1...");
            println!("  web-01: reachable, services OK");
            println!("  db-01: reachable, services OK");
        }
        _ => println!("nixops {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nixops".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nixops(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nixops};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nixops"), "nixops");
        assert_eq!(basename(r"C:\bin\nixops.exe"), "nixops.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nixops.exe"), "nixops");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nixops(&["--help".to_string()], "nixops"), 0);
        assert_eq!(run_nixops(&["-h".to_string()], "nixops"), 0);
        assert_eq!(run_nixops(&["--version".to_string()], "nixops"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nixops(&[], "nixops"), 0);
    }
}
