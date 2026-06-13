#![deny(clippy::all)]

//! bill-cli — SlateOS BILL (formerly Bill.com — accounts payable/receivable automation)
//!
//! Single personality: `bill`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bill(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bill [OPTIONS]");
        println!("BILL (formerly Bill.com) (SlateOS) — AP + AR automation");
        println!();
        println!("Options:");
        println!("  --ap                   Accounts Payable (Bill Pay automation)");
        println!("  --ar                   Accounts Receivable (Get Paid)");
        println!("  --spend                BILL Spend & Expense (formerly Divvy)");
        println!("  --accountants          BILL Accountant Console");
        println!("  --essentials           Essentials $45/user/mo");
        println!("  --team                 Team $55/user/mo");
        println!("  --corporate            Corporate $79/user/mo");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("BILL 2024 (SlateOS)"); return 0; }
    println!("BILL 2024 (SlateOS)");
    println!("  Vendor: BILL Holdings, Inc. (San Jose, CA — NYSE:BILL)");
    println!("  Founder: René Lacerte (also founded PayCycle, sold to Intuit 2009)");
    println!("          Lacerte family name = Lacerte Software, which became Intuit's tax product");
    println!("  Founded: 2006 as Bill.com");
    println!("          IPO 2019 ($BILL, NYSE) — popped from $22 IPO price to $96 on first day");
    println!("  Rebrand: 'Bill.com' → 'BILL' May 2022 (signals broader spend management beyond AP)");
    println!("  Acquisitions: Divvy (corporate cards) May 2021 for $2.5B → 'BILL Spend & Expense'");
    println!("               Invoice2go (invoice for solopreneurs) Sep 2021 for $625M");
    println!("               Finmark (financial planning) Feb 2023 — divested 2024");
    println!("  Scale: 480,000+ businesses across BILL network");
    println!("        $300B+ annual payment volume flowing through platform");
    println!("        ~3,000 employees");
    println!("        FY2024 revenue ~$1.3B");
    println!("  Pricing: Essentials $45/user/mo (basic AP or AR)");
    println!("          Team $55/user/mo (AP + AR + multi-user)");
    println!("          Corporate $79/user/mo (workflows, multi-entity, advanced approvals)");
    println!("          Enterprise — custom");
    println!("  Features (AP):");
    println!("    - OCR vendor invoices (email forward → text extraction → ready to approve)");
    println!("    - 2-way + 3-way match (PO + invoice + receipt)");
    println!("    - Multi-level approval workflows by amount/department/vendor");
    println!("    - Pay vendors via ACH (free), check (BILL prints + mails), card, international wire");
    println!("    - Vendor network — pay 5M+ vendors that already have a BILL profile (faster onboarding)");
    println!("    - 1099 tax form generation + e-filing");
    println!("  Features (AR):");
    println!("    - Customer-facing 'Get Paid' invoice portal");
    println!("    - ACH + credit card + same-day ACH");
    println!("    - Recurring invoices + payment reminders + auto-charge on file");
    println!("  Features (Spend & Expense, ex-Divvy):");
    println!("    - Virtual + physical corporate cards with budget enforcement at swipe");
    println!("    - Cashback rewards on card spend");
    println!("    - Receipt capture mobile app + AI categorization");
    println!("    - Approval-before-spend workflows (vs Ramp/Brex's after-spend approval)");
    println!("  Integrations: QuickBooks (deepest), Xero, NetSuite, Sage Intacct, Microsoft Dynamics");
    println!("              accountant + bookkeeper marketplace");
    println!("  Customers: SMB + mid-market — primarily 10-1000 employees");
    println!("            accountant-led market (many CPAs onboard their clients onto BILL)");
    println!("  Critique: per-user pricing adds up vs Ramp/Brex's free models");
    println!("           Divvy integration into BILL UI rocky for first ~2 years (separated UX)");
    println!("           checks-by-mail still account for huge volume — modernizing slowly");
    println!("  Differentiator: deep AP automation (paper checks → ACH/wire/intl) + vendor network reach");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bill".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bill(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bill};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bill"), "bill");
        assert_eq!(basename(r"C:\bin\bill.exe"), "bill.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bill.exe"), "bill");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bill(&["--help".to_string()], "bill"), 0);
        assert_eq!(run_bill(&["-h".to_string()], "bill"), 0);
        let _ = run_bill(&["--version".to_string()], "bill");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bill(&[], "bill");
    }
}
