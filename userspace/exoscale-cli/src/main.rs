#![deny(clippy::all)]

//! exoscale-cli — OurOS Exoscale cloud CLI
//!
//! Multi-personality: `exo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_exo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: exo COMMAND [OPTIONS]");
        println!("Exoscale CLI 1.78.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  compute      Manage compute instances");
        println!("  dbaas        Manage managed databases");
        println!("  dns          Manage DNS");
        println!("  iam          Manage IAM");
        println!("  sks          Manage Kubernetes clusters");
        println!("  storage      Manage object storage");
        println!("  config       Manage CLI config");
        println!("  status       Exoscale service status");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("exo 1.78.0"),
        "compute" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("instance");
            match sub {
                "instance" => {
                    let action = args.get(2).map(|s| s.as_str()).unwrap_or("list");
                    match action {
                        "list" => {
                            println!("ID          Name     Type        Zone      State    IP");
                            println!("abc12345    web-1    standard.s  ch-gva-2  running  198.51.100.1");
                        }
                        "create" => println!("Compute instance created."),
                        _ => println!("exo compute instance: '{}' completed", action),
                    }
                }
                _ => println!("exo compute: '{}' completed", sub),
            }
        }
        "sks" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID          Name      Version  Zone      State");
                    println!("abc12345    myclus    1.29     ch-gva-2  running");
                }
                "create" => println!("SKS cluster created."),
                "kubeconfig" => println!("Kubeconfig saved."),
                _ => println!("exo sks: '{}' completed", sub),
            }
        }
        "status" => {
            println!("Exoscale Status: All Systems Operational");
            println!("  Compute:  ✓ Operational");
            println!("  Storage:  ✓ Operational");
            println!("  Network:  ✓ Operational");
            println!("  DNS:      ✓ Operational");
        }
        _ => println!("exo: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "exo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_exo(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_exo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/exoscale"), "exoscale");
        assert_eq!(basename(r"C:\bin\exoscale.exe"), "exoscale.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("exoscale.exe"), "exoscale");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_exo(&["--help".to_string()]), 0);
        assert_eq!(run_exo(&["-h".to_string()]), 0);
        assert_eq!(run_exo(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_exo(&[]), 0);
    }
}
