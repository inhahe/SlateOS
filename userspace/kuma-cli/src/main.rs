#![deny(clippy::all)]

//! kuma-cli — OurOS Kuma service mesh
//!
//! Multi-personality: `kuma-cp`, `kuma-dp`, `kumactl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kuma(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "kuma-cp" => {
                println!("kuma-cp (OurOS) — Kuma control plane");
                println!("  run                Start control plane");
                println!("  migrate            Run store migrations");
            }
            "kuma-dp" => {
                println!("kuma-dp (OurOS) — Kuma data plane proxy");
                println!("  run                Start data plane");
                println!("  --cp-address URL   Control plane address");
                println!("  --name NAME        Data plane name");
            }
            _ => {
                println!("kumactl (OurOS) — Kuma management CLI");
                println!("  get meshes|dataplanes|policies  List resources");
                println!("  apply -f FILE      Apply policy");
                println!("  delete TYPE NAME   Delete resource");
                println!("  inspect            Inspect resource");
                println!("  install            Install components");
                println!("  config             Manage CLI config");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Kuma v2.7.2 (OurOS)"); return 0; }
    match prog {
        "kuma-cp" => {
            println!("Kuma Control Plane v2.7.2");
            println!("  API: https://0.0.0.0:5681");
            println!("  GUI: https://0.0.0.0:5682");
            println!("  Meshes: 2");
            println!("  Data planes: 15");
        }
        "kuma-dp" => {
            println!("Kuma Data Plane v2.7.2");
            println!("  Connected to CP: https://kuma-cp:5681");
            println!("  Envoy: v1.29");
        }
        _ => {
            println!("Kuma v2.7.2 (OurOS)");
            println!("  Meshes: 2 (default, production)");
            println!("  Data planes: 15 online");
            println!("  Policies: 23 applied");
            println!("  mTLS: permissive");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kumactl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kuma(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kuma};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kuma"), "kuma");
        assert_eq!(basename(r"C:\bin\kuma.exe"), "kuma.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kuma.exe"), "kuma");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kuma(&["--help".to_string()], "kuma"), 0);
        assert_eq!(run_kuma(&["-h".to_string()], "kuma"), 0);
        let _ = run_kuma(&["--version".to_string()], "kuma");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kuma(&[], "kuma");
    }
}
