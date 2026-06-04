#![deny(clippy::all)]

//! griddb-cli — OurOS GridDB IoT database
//!
//! Multi-personality: `gridstore`, `gs_admin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_griddb(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "gs_admin" => {
                println!("gs_admin (OurOS) — GridDB admin tool");
                println!("  -u USER            Admin user");
                println!("  -p PASSWORD        Admin password");
                println!("  --cluster CLUSTER  Cluster name");
                println!("  --show-cluster     Show cluster info");
                println!("  --show-node        Show node info");
                println!("  --show-container   Show containers");
            }
            _ => {
                println!("gridstore (OurOS) — GridDB server node");
                println!("  --config DIR       Config directory");
                println!("  --cluster NAME     Cluster name");
                println!("  --mode MODE        Start mode (normal/maintenance)");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("GridDB v5.5.0 (OurOS)"); return 0; }
    match prog {
        "gs_admin" => {
            println!("GridDB Admin v5.5.0");
            println!("  Cluster: myCluster (MASTER)");
            println!("  Nodes: 3 (all active)");
            println!("  Containers: 1,234");
            println!("  Partitions: 128");
            println!("  Memory: 8 GB / 16 GB");
        }
        _ => {
            println!("GridDB v5.5.0 (OurOS)");
            println!("  Cluster: myCluster");
            println!("  Node: node001 (MASTER)");
            println!("  Listening: 0.0.0.0:10001");
            println!("  SQL: 0.0.0.0:20001");
            println!("  Containers: 1,234");
            println!("  Rows: 567 million");
            println!("  Storage: 23.4 GB");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gridstore".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_griddb(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_griddb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/griddb"), "griddb");
        assert_eq!(basename(r"C:\bin\griddb.exe"), "griddb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("griddb.exe"), "griddb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_griddb(&["--help".to_string()], "griddb"), 0);
        assert_eq!(run_griddb(&["-h".to_string()], "griddb"), 0);
        let _ = run_griddb(&["--version".to_string()], "griddb");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_griddb(&[], "griddb");
    }
}
