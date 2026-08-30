#![deny(clippy::all)]

//! cilium-cli — Slate OS Cilium eBPF networking
//!
//! Multi-personality: `cilium`, `hubble`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cilium(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] COMMAND", prog);
        match prog {
            "hubble" => {
                println!("Hubble v0.13 (Slate OS) — Network observability (Cilium)");
                println!("  observe        Observe network flows");
                println!("  status         Show Hubble status");
                println!("  list           List nodes");
                println!("  -f             Follow mode");
                println!("  --type TYPE    Flow type filter");
                println!("  --verdict V    Verdict filter (FORWARDED, DROPPED)");
            }
            _ => {
                println!("Cilium v1.15 (Slate OS) — eBPF-based networking");
                println!("  install        Install Cilium");
                println!("  status         Show status");
                println!("  connectivity   Connectivity tests");
                println!("  endpoint       Endpoint management");
                println!("  policy         Policy management");
                println!("  service        Service management");
                println!("  bpf            BPF map management");
            }
        }
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cilium v1.15.6 (Slate OS)"); return 0; }
    match prog {
        "hubble" => {
            println!("Hubble v0.13 (Slate OS)");
            println!("  Nodes: 5");
            println!("  Flows observed: 1,234,567");
            println!("  Flow rate: 5,678/s");
            println!("  Verdicts: FORWARDED=98.5%, DROPPED=1.5%");
        }
        _ => {
            println!("Cilium v1.15.6 (Slate OS)");
            println!("  /host           Ready");
            println!("  Nodes: 5/5 ready");
            println!("  Endpoints: 45 ready, 0 not-ready");
            println!("  Identities: 23");
            println!("  Cluster mesh: disabled");
            println!("  Encryption: WireGuard");
            println!("  BPF maps: 67 loaded");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cilium".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cilium(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cilium};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cilium"), "cilium");
        assert_eq!(basename(r"C:\bin\cilium.exe"), "cilium.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cilium.exe"), "cilium");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cilium(&["--help".to_string()], "cilium"), 0);
        assert_eq!(run_cilium(&["-h".to_string()], "cilium"), 0);
        let _ = run_cilium(&["--version".to_string()], "cilium");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cilium(&[], "cilium");
    }
}
