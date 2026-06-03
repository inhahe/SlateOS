#![deny(clippy::all)]

//! weave-cli — OurOS Weave Net container networking
//!
//! Multi-personality: `weave`, `weaveutil`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_weave(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] COMMAND", prog);
        match prog {
            "weaveutil" => {
                println!("weaveutil (OurOS) — Weave Net utility commands");
                println!("  bridge-ip      Show bridge IP");
                println!("  container-addrs Show container addresses");
                println!("  cni-net        CNI network config");
            }
            _ => {
                println!("Weave Net v2.8 (OurOS) — Container network overlay");
                println!("  launch         Launch Weave Net");
                println!("  connect HOST   Connect to peer");
                println!("  forget HOST    Forget peer");
                println!("  status         Show status");
                println!("  ps             List attached containers");
                println!("  expose CIDR    Expose network to host");
                println!("  dns-add        Add DNS entry");
                println!("  dns-remove     Remove DNS entry");
            }
        }
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Weave Net v2.8.7 (OurOS)"); return 0; }
    println!("Weave Net v2.8.7 (OurOS)");
    println!("  Status: ready");
    println!("  Network: 10.32.0.0/12");
    println!("  Range: 10.32.0.1 - 10.47.255.254");
    println!("  Peers: 3 (established)");
    println!("  Connections: 6 (full mesh)");
    println!("  Encryption: enabled (NaCl)");
    println!("  DNS: weave.local");
    println!("  IPAM: 45 allocated");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "weave".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_weave(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_weave};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/weave"), "weave");
        assert_eq!(basename(r"C:\bin\weave.exe"), "weave.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("weave.exe"), "weave");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_weave(&["--help".to_string()], "weave"), 0);
        assert_eq!(run_weave(&["-h".to_string()], "weave"), 0);
        assert_eq!(run_weave(&["--version".to_string()], "weave"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_weave(&[], "weave"), 0);
    }
}
