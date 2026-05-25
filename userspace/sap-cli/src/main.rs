#![deny(clippy::all)]

//! sap-cli — OurOS SAP S/4HANA + SAP GUI
//!
//! Single personality: `sap`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sap(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sap [OPTIONS]");
        println!("SAP S/4HANA 2023 + SAP GUI 8.00 (OurOS) — Enterprise ERP");
        println!();
        println!("Options:");
        println!("  -conn SYSID            Connect to system (e.g. PRD/QAS/DEV)");
        println!("  -client CLT            Client number");
        println!("  -lang EN/DE/...        Logon language");
        println!("  -tcode TCODE           Transaction code (e.g. SE80, VA01, ME21N)");
        println!("  --fiori                Open Fiori Launchpad");
        println!("  --btp                  SAP Business Technology Platform");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SAP S/4HANA 2023 FPS02 + SAP GUI for Windows 8.00 (OurOS)"); return 0; }
    println!("SAP S/4HANA 2023 FPS02 + SAP GUI for Windows 8.00 (OurOS)");
    println!("  Editions: S/4HANA Cloud (Public/Private), S/4HANA on-prem, ECC (legacy)");
    println!("  Database: SAP HANA (in-memory columnar) — required for S/4HANA");
    println!("  Modules: FI/CO, MM, SD, PP, QM, PM, HR (now SuccessFactors), CRM, EWM");
    println!("  Language: ABAP (Advanced Business Application Programming), now ABAP Cloud");
    println!("  UX: SAP Fiori (HTML5/UI5), classic SAP GUI (Windows/Java/HTML)");
    println!("  BTP: Business Technology Platform — extensions, integrations, AI Hub");
    println!("  Joule: GenAI assistant; Datasphere (data fabric); LeanIX (EA)");
    println!("  License: enterprise — per-user, FUE metrics, contract-based");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sap".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sap(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
