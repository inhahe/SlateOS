#![deny(clippy::all)]

//! netsuite-cli — OurOS Oracle NetSuite cloud ERP
//!
//! Single personality: `netsuite`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ns(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: netsuite [OPTIONS] [SUBCMD]");
        println!("Oracle NetSuite 2024.2 (OurOS) — Cloud ERP / accounting / commerce");
        println!();
        println!("Options:");
        println!("  --account ACCT         Account ID");
        println!("  sdfcli                 SuiteCloud Development Framework CLI");
        println!("  --suiteanalytics       SuiteAnalytics Workbook/Connect");
        println!("  --suiteapps            SuiteApp Marketplace");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Oracle NetSuite 2024.2 (OurOS)"); return 0; }
    println!("Oracle NetSuite 2024.2 (OurOS)");
    println!("  Modules: Financial Management, ERP, CRM, PSA, HCM (SuitePeople),");
    println!("           E-commerce (SuiteCommerce), Omnichannel, Inventory, Manufacturing");
    println!("  Architecture: cloud-native multi-tenant, 2 releases/year");
    println!("  Customization: SuiteCloud Platform — SuiteScript 2.x (JavaScript)");
    println!("  Workflows: SuiteFlow (no-code), SuiteAnalytics (reporting)");
    println!("  SuiteBuilder: custom records, fields, forms, sublists");
    println!("  SuiteBundler: package customizations for distribution");
    println!("  SDF: SuiteCloud Development Framework — file-based git-friendly dev");
    println!("  License: per-user subscription, modules add-on (mid-market ERP leader)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "netsuite".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ns(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
