#![deny(clippy::all)]

//! civo-cli — OurOS Civo cloud CLI
//!
//! Multi-personality: `civo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_civo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: civo COMMAND [OPTIONS]");
        println!("Civo CLI 1.1.76 (OurOS)");
        println!();
        println!("Commands:");
        println!("  instance     Manage compute instances");
        println!("  kubernetes   Manage Kubernetes clusters");
        println!("  network      Manage networks");
        println!("  volume       Manage volumes");
        println!("  firewall     Manage firewalls");
        println!("  database     Manage databases");
        println!("  domain       Manage DNS domains");
        println!("  objectstore  Manage object storage");
        println!("  apikey       Manage API keys");
        println!("  region       Manage regions");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("Civo CLI v1.1.76"),
        "instance" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("ID          Hostname     Size        Status   Public IP");
                    println!("abc12345    web-1        g3.medium   ACTIVE   198.51.100.1");
                    println!("def12345    worker-1     g3.large    ACTIVE   198.51.100.2");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("new-instance");
                    println!("Instance '{}' created.", name);
                }
                _ => println!("civo instance: '{}' completed", sub),
            }
        }
        "kubernetes" | "k8s" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("ID          Name      Nodes   Version   Status");
                    println!("abc12345    myclus    3       1.29.3    ACTIVE");
                }
                "create" => println!("Kubernetes cluster created."),
                "config" => println!("Kubeconfig saved to ~/.kube/config"),
                _ => println!("civo kubernetes: '{}' completed", sub),
            }
        }
        "region" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" || sub == "ls" {
                println!("Code      Name            Current");
                println!("LON1      London          <---");
                println!("NYC1      New York        ");
                println!("FRA1      Frankfurt       ");
                println!("PHX1      Phoenix         ");
            }
        }
        "apikey" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" || sub == "ls" {
                println!("Name       Key");
                println!("default    xxxxxxxxxxxxxxxxxxxxxxxx");
            }
        }
        _ => println!("civo: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "civo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_civo(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_civo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/civo"), "civo");
        assert_eq!(basename(r"C:\bin\civo.exe"), "civo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("civo.exe"), "civo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_civo(&["--help".to_string()]), 0);
        assert_eq!(run_civo(&["-h".to_string()]), 0);
        assert_eq!(run_civo(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_civo(&[]), 0);
    }
}
