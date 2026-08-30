#![deny(clippy::all)]

//! sophos-cli — Slate OS Sophos Home / Intercept X / Central
//!
//! Single personality: `sophos`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sophos(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sophos [OPTIONS]");
        println!("Sophos Intercept X Advanced / Central / Home (Slate OS)");
        println!();
        println!("Options:");
        println!("  --scan TYPE            full/quick/custom");
        println!("  --xdr                  Sophos XDR (extended detection and response)");
        println!("  --mdr                  Sophos MDR (managed detection and response)");
        println!("  --firewall             Sophos Firewall (XGS, formerly XG)");
        println!("  --home                 Sophos Home (consumer, free for 3 PCs)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Sophos Intercept X Advanced 2024.4 / Endpoint Agent 2024.4.5.5 (Slate OS)"); return 0; }
    println!("Sophos Intercept X Advanced 2024.4 (Slate OS)");
    println!("  Origin: UK, founded 1985 (Abingdon, Oxfordshire); private equity owned 2020");
    println!("  Endpoint: Intercept X (NGAV + EDR), Server (workload), Mobile (UEM)");
    println!("  Network: Sophos Firewall (XGS series), wireless, switches, SD-WAN");
    println!("  Email: Sophos Email (gateway + post-delivery), Encryption (PhishThreat)");
    println!("  Cloud: Sophos Cloud Workload Protection, Cloud Optix (CSPM)");
    println!("  Detection: Deep Learning (neural network), CryptoGuard (anti-ransomware),");
    println!("             ExploitGuard (memory hardening), Active Adversary Mitigations");
    println!("  Services: Sophos MDR (24/7 threat hunting), Rapid Response (incident)");
    println!("  Acquisitions: Capsule8 (Linux), Refactr (DevSecOps), SOC.OS (alert triage)");
    println!("  License: per-user/per-device subscription, partner-led sales channel");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sophos".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sophos(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, run_sophos, strip_ext};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sophos"), "sophos");
        assert_eq!(basename(r"C:\bin\sophos.exe"), "sophos.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sophos.exe"), "sophos");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sophos(&["--help".to_string()], "sophos"), 0);
        assert_eq!(run_sophos(&["-h".to_string()], "sophos"), 0);
        let _ = run_sophos(&["--version".to_string()], "sophos");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sophos(&[], "sophos");
    }
}
