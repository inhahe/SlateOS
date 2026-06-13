#![deny(clippy::all)]

//! expensify-cli — SlateOS Expensify (SmartScan receipts, NewDot New Expensify, politically loud)
//!
//! Single personality: `expensify`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_exp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: expensify [OPTIONS]");
        println!("Expensify (SlateOS) — Expense management with SmartScan");
        println!();
        println!("Options:");
        println!("  --smartscan            SmartScan receipt OCR (10 free/mo)");
        println!("  --collect              Collect plan ($10/user/mo)");
        println!("  --control              Control plan ($18/user/mo with workflows)");
        println!("  --card                 Expensify Card (cashback corporate card)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Expensify (SlateOS)"); return 0; }
    println!("Expensify (SlateOS)");
    println!("  Vendor: Expensify, Inc. (San Francisco — NASDAQ:EXFY)");
    println!("  Founder: David Barrett (BitTorrent veteran, CTO Red Swoosh acquired by Akamai)");
    println!("  Founded: 2008 — kicked off at TechCrunch50");
    println!("  IPO: November 2021 ($EXFY, NASDAQ) — IPO'd at $27, since dropped under $4");
    println!("  Scale: ~12M users lifetime, ~700K active users");
    println!("        ~140 employees (small for a public co.)");
    println!("        revenue ~$150M with declining trend (more competition)");
    println!("  Strategy: 'expense reports that don't suck' — consumer-grade UX for hated finance task");
    println!("           pivoted to 'super app' New Expensify (NewDot) 2022 — chat + expenses + payments + invoicing");
    println!("  Politics: Barrett famously emailed all customers urging votes against Trump (2020)");
    println!("           outspoken political stances drive both fans and detractors");
    println!("           public-letter culture (the 'Open Letter' format is Expensify's brand quirk)");
    println!("  Pricing:");
    println!("    Free with Expensify Card (no per-user fee if 50% spend on card)");
    println!("    Collect $10/user/mo (without Card)");
    println!("    Control $18/user/mo (workflows, multi-level approval, accountant features)");
    println!("    Pay-per-use $20-50/mo for individuals");
    println!("  Killer feature — SmartScan:");
    println!("    photo receipt → OCR'd into category + amount + merchant + date");
    println!("    human verification on edge cases (the 'Expensify Pause' — receipt sent to humans worldwide)");
    println!("    10 SmartScans/mo free on personal account");
    println!("  Features:");
    println!("    - SmartScan receipt OCR (mobile + email)");
    println!("    - Expense report creation + multi-level approval");
    println!("    - Corporate card auto-import (most major issuers)");
    println!("    - Mileage tracking (manual or GPS)");
    println!("    - Expensify Card (Visa) — instant policy enforcement, cashback 1-2%");
    println!("    - Bill Pay (vendor invoices)");
    println!("    - Invoicing (since New Expensify)");
    println!("    - Chat (New Expensify built around chat-with-anyone — payment/expense/invoice context)");
    println!("    - QuickBooks/Xero/NetSuite/Sage Intacct integrations");
    println!("  Customers: SMBs through mid-market — Snap Inc., Plaid, Atlassian, Pinterest (historical)");
    println!("  Critique: New Expensify (NewDot) rollout messy — many users prefer Old Expensify");
    println!("           Concur+Brex+Ramp+Airbase all squeezing mindshare");
    println!("  Differentiator: SmartScan + Expensify Card combo for SMBs that hate Concur");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "expensify".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_exp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_exp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/expensify"), "expensify");
        assert_eq!(basename(r"C:\bin\expensify.exe"), "expensify.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("expensify.exe"), "expensify");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_exp(&["--help".to_string()], "expensify"), 0);
        assert_eq!(run_exp(&["-h".to_string()], "expensify"), 0);
        let _ = run_exp(&["--version".to_string()], "expensify");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_exp(&[], "expensify");
    }
}
