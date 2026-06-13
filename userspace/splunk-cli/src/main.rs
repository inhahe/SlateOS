#![deny(clippy::all)]

//! splunk-cli — Slate OS Splunk (the original log/SIEM platform)
//!
//! Single personality: `splunk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_splunk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: splunk [OPTIONS]");
        println!("Splunk Enterprise 9.3 (Slate OS) — Operational intelligence platform");
        println!();
        println!("Options:");
        println!("  search 'spl query'     SPL (Search Processing Language) search");
        println!("  --indexer              Splunk Indexer role");
        println!("  --forwarder            Universal/Heavy Forwarder");
        println!("  --enterprise-security  Splunk ES (SIEM)");
        println!("  --soar                 Splunk SOAR (incident response automation)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Splunk Enterprise 9.3.2 (Slate OS)"); return 0; }
    println!("Splunk Enterprise 9.3.2 (Slate OS)");
    println!("  Vendor: Splunk Inc. (San Francisco, founded 2003)");
    println!("          acquired by Cisco Mar 2024 for $28B (largest cybersecurity acquisition ever)");
    println!("  Founders: Michael Baum, Rob Das, Erik Swan (former systems engineers — 'spelunking' logs)");
    println!("  History: pioneered the 'log everything, search later' model");
    println!("          IPO NYSE:SPLK 2012 → Cisco subsidiary 2024");
    println!("  Pricing: notoriously expensive — workload-based (was data-volume-based, capped pricing reform 2020)");
    println!("          Splunk Cloud Platform: usage-based, starts ~$40K/year for small workloads");
    println!("          → 'Splunk tax' is industry meme for runaway log bills");
    println!("  Editions:");
    println!("    - Splunk Enterprise (self-hosted)");
    println!("    - Splunk Cloud Platform (managed SaaS, SOC 2 / HIPAA / FedRAMP)");
    println!("    - Splunk Enterprise Security (SIEM)");
    println!("    - Splunk SOAR (Phantom acquisition — security automation)");
    println!("    - Splunk Observability Cloud (SignalFx + VictorOps + Omnition acquisitions — APM/RUM/traces/logs)");
    println!("    - Splunk IT Service Intelligence (ITSI — service-level dashboards)");
    println!("  Killer feature — SPL: Search Processing Language");
    println!("    Pipe-style: `index=web sourcetype=access_combined | stats count by host`");
    println!("    Schema-on-read — no schema design upfront, index any text");
    println!("  Components:");
    println!("    - Universal Forwarder (light agent) → Indexer (storage+search) ← Search Head (query UI)");
    println!("    - Distributed: indexer cluster + search head cluster + deployer + license master");
    println!("  Use cases: SIEM, IT operations, fraud detection, business analytics, IoT, compliance");
    println!("  Differentiator: still THE benchmark for ad-hoc log search at petabyte scale");
    println!("                  enterprises pay millions because nothing else handles that volume of unstructured data");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "splunk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_splunk(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_splunk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/splunk"), "splunk");
        assert_eq!(basename(r"C:\bin\splunk.exe"), "splunk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("splunk.exe"), "splunk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_splunk(&["--help".to_string()], "splunk"), 0);
        assert_eq!(run_splunk(&["-h".to_string()], "splunk"), 0);
        let _ = run_splunk(&["--version".to_string()], "splunk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_splunk(&[], "splunk");
    }
}
