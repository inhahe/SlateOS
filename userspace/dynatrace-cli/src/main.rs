#![deny(clippy::all)]

//! dynatrace-cli — OurOS Dynatrace (AI-driven full-stack observability, Davis AI)
//!
//! Single personality: `dynatrace`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dynatrace [OPTIONS]");
        println!("Dynatrace SaaS (OurOS) — Full-stack observability + Davis AI");
        println!();
        println!("Options:");
        println!("  --oneagent             Deploy OneAgent (single binary for all monitoring)");
        println!("  --davis                Davis AI (deterministic root-cause analysis)");
        println!("  --grail                Grail (causal log/event lakehouse)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Dynatrace SaaS 2024.11 (OurOS)"); return 0; }
    println!("Dynatrace SaaS 2024.11 (OurOS)");
    println!("  Vendor: Dynatrace Inc. (Waltham, MA + Linz, Austria — NYSE:DT)");
    println!("  Founders: Bernd Greifeneder + team (Linz, Austria, 2005)");
    println!("           originally 'dynaTrace Software' — Java APM");
    println!("  History: acquired by Compuware 2011");
    println!("          spun out via Thoma Bravo PE 2014");
    println!("          rebooted SaaS platform 'Dynatrace' (capital D) 2016 — full re-architecture");
    println!("          IPO NYSE 2019");
    println!("  Pricing: usage-based (Davis Data Units + Host Units + others)");
    println!("          enterprise-only — typically $50K-$1M+/yr deployments");
    println!("  Killer feature — OneAgent:");
    println!("    single binary auto-discovers everything on the host");
    println!("    JVM/CLR/PHP/Node/Python/Go bytecode instrumentation, no code changes");
    println!("    network/kernel/process visibility via eBPF");
    println!("    auto-detects services, dependencies, code paths");
    println!("  Killer feature — Davis AI:");
    println!("    deterministic causation engine (NOT statistical correlation)");
    println!("    uses Smartscape topology graph to walk dependencies");
    println!("    points at single root cause for an outage, not 50 alerts");
    println!("  Components:");
    println!("    - OneAgent (auto-instrumentation agent)");
    println!("    - Smartscape (real-time topology map)");
    println!("    - Davis AI (RCA engine)");
    println!("    - Grail (lakehouse-style observability data store, schema-on-read)");
    println!("    - DQL (Dynatrace Query Language — SPL-style pipe queries on Grail)");
    println!("    - Synthetic Monitoring, Real User Monitoring (RUM), Session Replay");
    println!("    - Application Security (RASP — runtime detect+block CVE exploitation)");
    println!("  Vs competitors: Davis AI > Datadog Watchdog (more deterministic), OneAgent > APM agents per language");
    println!("                  but pricier per host than Datadog");
    println!("  Customers: SAP, Lufthansa, BMW, Mercedes — large European enterprises (Austrian roots)");
    println!("  Differentiator: AI-driven RCA + zero-config full-stack OneAgent — minimal manual setup");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dynatrace".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dt(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dynatrace"), "dynatrace");
        assert_eq!(basename(r"C:\bin\dynatrace.exe"), "dynatrace.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dynatrace.exe"), "dynatrace");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dt(&["--help".to_string()], "dynatrace"), 0);
        assert_eq!(run_dt(&["-h".to_string()], "dynatrace"), 0);
        let _ = run_dt(&["--version".to_string()], "dynatrace");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dt(&[], "dynatrace");
    }
}
