#![deny(clippy::all)]

//! rippling-cli — SlateOS Rippling (Parker Conrad's HR+IT+Finance super-platform)
//!
//! Single personality: `rippling`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rip(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rippling [OPTIONS]");
        println!("Rippling (SlateOS) — HR + IT + Finance employee-graph platform");
        println!();
        println!("Options:");
        println!("  --hr-cloud             HR Cloud (payroll, benefits, talent, time)");
        println!("  --it-cloud             IT Cloud (SSO, device mgmt, app provisioning)");
        println!("  --finance-cloud        Finance Cloud (corp cards, expense, bill pay)");
        println!("  --eor                  EOR (Employer of Record, 50+ countries)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Rippling 2024 (SlateOS)"); return 0; }
    println!("Rippling 2024 (SlateOS)");
    println!("  Vendor: People Center, Inc. dba Rippling (San Francisco, founded 2016)");
    println!("  Founder: Parker Conrad (previously Zenefits CEO — ousted 2016 over licensing scandal)");
    println!("  Funding: Founders Fund, Kleiner Perkins, Sequoia, Coatue + others");
    println!("          $1.4B+ raised, $13.5B valuation Aug 2024");
    println!("  Strategy: 'Employee Graph' — one identity → all employee systems (HR, IT, finance)");
    println!("           pitch: 'onboard a new hire in 90 seconds: payroll + email + Slack + 1Password + laptop'");
    println!("  Sue against Deel: 2024 — Rippling sued Deel for corporate espionage");
    println!("                   (Deel allegedly paid Rippling employee for trade secrets)");
    println!("                   high-profile HR-tech rivalry");
    println!("  Pricing: starts ~$8/employee/mo + per-module fees");
    println!("          custom pricing typical (sales-led)");
    println!("  Three clouds:");
    println!("    HR Cloud: payroll, benefits, time, recruiting, learning, performance, ATS, surveys");
    println!("    IT Cloud: SSO, identity (SCIM provisioning), MDM (Mac/Win), device buy/ship/manage,");
    println!("              password manager, automated app provisioning (Slack/G Suite/Salesforce/etc.)");
    println!("    Finance Cloud: corporate cards, expense management, bill pay, travel booking, spend mgmt");
    println!("  EOR (Employer of Record): hire in 50+ countries without setting up entities");
    println!("                            Rippling becomes the legal employer abroad");
    println!("  Killer features:");
    println!("    - Workflow Automation Studio (no-code, employee-data-driven triggers)");
    println!("    - Custom fields → custom reports → custom workflows on any employee attribute");
    println!("    - Recipes (pre-built workflow templates)");
    println!("    - Headcount Planning (FP&A integration)");
    println!("    - Spend management on top of corporate cards (Brex/Ramp-style)");
    println!("  Critique: complex pricing, every module is an add-on (à la carte → bill creep)");
    println!("           Parker Conrad's Zenefits reputational baggage");
    println!("           still less polished than Gusto/BambooHR in narrow use cases");
    println!("  Differentiator: only platform with HR + IT + Finance unified on one employee graph");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rippling".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rip(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rip};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rippling"), "rippling");
        assert_eq!(basename(r"C:\bin\rippling.exe"), "rippling.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rippling.exe"), "rippling");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rip(&["--help".to_string()], "rippling"), 0);
        assert_eq!(run_rip(&["-h".to_string()], "rippling"), 0);
        let _ = run_rip(&["--version".to_string()], "rippling");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rip(&[], "rippling");
    }
}
