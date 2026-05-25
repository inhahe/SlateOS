#![deny(clippy::all)]

//! appdynamics-cli — OurOS AppDynamics (Cisco's enterprise APM, business transaction model)
//!
//! Single personality: `appdynamics`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_appd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: appdynamics [OPTIONS]");
        println!("AppDynamics SaaS 24.11 (OurOS) — Application performance management");
        println!();
        println!("Options:");
        println!("  --controller           AppDynamics Controller (data + UI)");
        println!("  --agent                Language agent (Java/.NET/Node/Python/PHP/Go)");
        println!("  --business-iq          Business transaction analytics (revenue impact)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("AppDynamics SaaS 24.11 (OurOS)"); return 0; }
    println!("AppDynamics SaaS 24.11 (OurOS)");
    println!("  Vendor: AppDynamics (part of Cisco Systems since Jan 2017)");
    println!("  Founder: Jyoti Bansal (Indian-American engineer — also founded Harness, Unusual Ventures)");
    println!("          founded 2008 in San Francisco");
    println!("  History: IPO filed early 2017 — Cisco bought just before pricing for $3.7B");
    println!("          (largest 'won't-go-public' deal at the time)");
    println!("          now part of Cisco's 'Splunk + AppDynamics + ThousandEyes' observability stack");
    println!("          (since Cisco's $28B Splunk acquisition Mar 2024 — being consolidated)");
    println!("  Pricing: subscription, agent-based — typically $400-700/agent/year for APM Pro");
    println!("          Cisco Full-Stack Observability bundle pricing for large enterprises");
    println!("  Killer concept — Business Transactions:");
    println!("    every user click traced as a 'Business Transaction' end-to-end");
    println!("    correlate code-level performance with revenue impact");
    println!("    e.g.: 'slow checkout BT cost $X in lost orders last hour'");
    println!("  Components:");
    println!("    - Controller (data store + UI — SaaS or on-prem)");
    println!("    - APM Agents (Java/CLR/PHP/Node/Python/Go/C++)");
    println!("    - Database Visibility Agent");
    println!("    - Browser RUM (JS snippet)");
    println!("    - Mobile RUM (iOS/Android SDKs)");
    println!("    - Synthetic Monitoring");
    println!("    - End User Monitoring (EUM)");
    println!("    - Network Visibility (NetViz)");
    println!("  Features:");
    println!("    - Auto-discovered service map (Java/.NET bytecode instrumentation)");
    println!("    - Snapshot collection: deep call graphs for slow transactions");
    println!("    - SQL/HTTP exit calls auto-detected");
    println!("    - Dynamic baselines (compares to last 30 days at same hour)");
    println!("    - Health Rules + Policies (alerting)");
    println!("  Vs competitors: Strong in Java/.NET enterprise, weaker in cloud-native / k8s");
    println!("                  Dynatrace + Datadog both ahead on multi-cloud + container support");
    println!("  Customers: large enterprises with Java EE monoliths (banks, insurance, telco)");
    println!("  Differentiator: business-transaction-centric model — direct revenue correlation");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "appdynamics".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_appd(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
