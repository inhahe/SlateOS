#![deny(clippy::all)]

//! adp-cli — Slate OS ADP (Automatic Data Processing — the largest US payroll provider)
//!
//! Single personality: `adp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_adp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: adp [OPTIONS]");
        println!("ADP Workforce Now (Slate OS) — Payroll + HR + benefits + time + talent");
        println!();
        println!("Options:");
        println!("  --run                  ADP RUN (small business payroll)");
        println!("  --workforce-now        Workforce Now (50-1000 employees)");
        println!("  --vantage              Vantage HCM (1000+ employees)");
        println!("  --globalview           GlobalView (multi-country payroll)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ADP Workforce Now 2024 (Slate OS)"); return 0; }
    println!("ADP Workforce Now 2024 (Slate OS)");
    println!("  Vendor: Automatic Data Processing, Inc. (Roseland, NJ — NASDAQ:ADP)");
    println!("  Founded: 1949 by Henry Taub as 'Automatic Payrolls, Inc.'");
    println!("          one of the OLDEST surviving SaaS-adjacent businesses (pre-computer payroll service bureau)");
    println!("  Scale: pays 1 in 6 US workers (~26 million Americans)");
    println!("         ~60,000 employees, $18B annual revenue (FY2024)");
    println!("         processes ~$2 trillion in client payrolls annually");
    println!("  History: started as a manual payroll service bureau (Taub at age 21)");
    println!("          1961: IPO");
    println!("          spun off Broadridge (proxy services) 2007 — focus on payroll");
    println!("          spun off CDK Global (auto-dealer software) 2014");
    println!("  Products:");
    println!("    - ADP RUN (1-49 employees) — entry tier, ~$59/mo + $4/employee");
    println!("    - Workforce Now (50-1000) — HCM suite");
    println!("    - Vantage HCM (1000+) — enterprise HCM");
    println!("    - GlobalView Payroll (multi-country, 140+ countries)");
    println!("    - Celergo (international payroll aggregation)");
    println!("    - ADP TotalSource (PEO — co-employment service for SMBs)");
    println!("    - DataCloud (anonymized labor market analytics)");
    println!("  Features:");
    println!("    - Multi-jurisdiction payroll (federal/state/local US, Canada provinces, EU, APAC)");
    println!("    - Direct deposit + paycards (ADP Aline Card)");
    println!("    - Tax filing (federal/state/local, ~80 million W-2/1099 per year)");
    println!("    - Benefits administration (health, 401k, FSA, HSA, COBRA)");
    println!("    - Time + attendance (kiosks, mobile, geofence)");
    println!("    - Talent management (recruiting, onboarding, performance, learning)");
    println!("  ADP Marketplace: 600+ integrated apps (Slack, Microsoft, Concur, etc.)");
    println!("  Critique: legacy UI, complex pricing, sales-led friction");
    println!("           still THE incumbent — switching cost is high");
    println!("  Differentiator: scale + tax filing + 75-year track record — 'no one got fired for choosing ADP'");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "adp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_adp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_adp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/adp"), "adp");
        assert_eq!(basename(r"C:\bin\adp.exe"), "adp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("adp.exe"), "adp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_adp(&["--help".to_string()], "adp"), 0);
        assert_eq!(run_adp(&["-h".to_string()], "adp"), 0);
        let _ = run_adp(&["--version".to_string()], "adp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_adp(&[], "adp");
    }
}
