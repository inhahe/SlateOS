#![deny(clippy::all)]

//! kops-cli — Slate OS kops Kubernetes operations tool
//!
//! Single personality: `kops`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kops(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: kops COMMAND [OPTIONS]");
        println!("kops v1.29.0 (Slate OS) — Kubernetes Operations");
        println!();
        println!("Commands:");
        println!("  create          Create resources");
        println!("  delete          Delete resources");
        println!("  edit            Edit resources");
        println!("  get             List resources");
        println!("  rolling-update  Rolling update cluster");
        println!("  update          Update cluster config");
        println!("  upgrade         Upgrade cluster");
        println!("  validate        Validate cluster");
        println!("  export          Export config");
        println!("  toolbox         Toolbox utilities");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("kops v1.29.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("get");
    match cmd {
        "create" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("cluster");
            match sub {
                "cluster" => {
                    println!("Created cluster configuration.");
                    println!("  * Run 'kops update cluster' to apply.");
                }
                "secret" => println!("Created secret."),
                "ig" | "instancegroup" => println!("Created instance group."),
                _ => println!("kops create {}: completed", sub),
            }
        }
        "get" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("clusters");
            match sub {
                "clusters" => {
                    println!("NAME              CLOUD   ZONES");
                    println!("mycluster.k8s     aws     us-east-1a,us-east-1b");
                }
                "ig" | "instancegroups" => {
                    println!("NAME          ROLE    MACHINETYPE  MIN  MAX  ZONES");
                    println!("master-1a     Master  t3.medium    1    1    us-east-1a");
                    println!("nodes-1a      Node    t3.large     2    5    us-east-1a");
                }
                _ => println!("kops get {}: completed", sub),
            }
        }
        "update" => {
            println!("Cluster changes have been applied to the cloud.");
            println!("  kops has set your kubectl context to mycluster.k8s");
        }
        "validate" => {
            println!("Validating cluster mycluster.k8s");
            println!();
            println!("NODE STATUS");
            println!("NAME              ROLE    READY");
            println!("ip-10-0-1-100     master  True");
            println!("ip-10-0-1-101     node    True");
            println!("ip-10-0-1-102     node    True");
            println!();
            println!("Your cluster mycluster.k8s is ready.");
        }
        "rolling-update" => {
            println!("NAME              STATUS     NEEDUPDATE  READY  MIN  MAX");
            println!("master-1a         NeedsUpdate 1          0      1    1");
            println!("nodes-1a          Ready       0          2      2    5");
        }
        "delete" => println!("kops: Deleting cluster..."),
        "export" => println!("kops: Exported kubeconfig."),
        _ => println!("kops {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kops".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kops(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kops};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kops"), "kops");
        assert_eq!(basename(r"C:\bin\kops.exe"), "kops.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kops.exe"), "kops");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kops(&["--help".to_string()], "kops"), 0);
        assert_eq!(run_kops(&["-h".to_string()], "kops"), 0);
        let _ = run_kops(&["--version".to_string()], "kops");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kops(&[], "kops");
    }
}
