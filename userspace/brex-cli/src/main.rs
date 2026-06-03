#![deny(clippy::all)]

//! brex-cli — OurOS Brex (corporate card for startups, the original challenger)
//!
//! Single personality: `brex`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_brex(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: brex [OPTIONS]");
        println!("Brex (OurOS) — Corporate card + spend + AP + business account");
        println!();
        println!("Options:");
        println!("  --card                 Issue virtual or physical Brex card");
        println!("  --cash                 Brex Business Account (FDIC + treasury MMF)");
        println!("  --travel               Brex Travel");
        println!("  --essentials           Essentials ($0/user/mo)");
        println!("  --premium              Premium ($12/user/mo)");
        println!("  --enterprise           Enterprise — custom");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Brex 2024 (OurOS)"); return 0; }
    println!("Brex 2024 (OurOS)");
    println!("  Vendor: Brex Inc. (San Francisco — founded 2017)");
    println!("  Founders: Henrique Dubugras + Pedro Franceschi (Brazil — sold Pagar.me to Stone for $150M before age 22)");
    println!("           moved to Stanford, founded Brex");
    println!("  Funding: Founders Fund, DST, Y Combinator (S17), Greenoaks, Lone Pine + others");
    println!("          $1.5B+ raised");
    println!("          $12.3B valuation Jan 2022 (down from peak — adjusted with market)");
    println!("  History: 2017 launched first corporate card for startups (no personal guarantee, daily payoff)");
    println!("          2020: pivoted to broader 'financial OS for startups'");
    println!("          2022: famously pulled out of SMB market ('focus on growth-stage + enterprise')");
    println!("          2023: laid off ~20% of staff in restructuring");
    println!("          2024: AI agent product launch ('Brex AI')");
    println!("  Scale: ~30K customer companies");
    println!("        $40B+ annualized card spend");
    println!("        Cash + investment products growing");
    println!("  Pricing:");
    println!("    Essentials FREE ($0/user/mo) — card + travel + Cash account");
    println!("    Premium $12/user/mo — bill pay + advanced expense + multi-entity");
    println!("    Enterprise — custom (procurement, multi-currency, etc.)");
    println!("  Killer features:");
    println!("    - Daily payoff card (not 30-day cycle) — preserves credit limit for fast-burning startups");
    println!("    - Underwriting based on cash balance (not personal credit) — perfect for funded startups");
    println!("    - Brex Cash (formerly Brex Empower) — business account with FDIC sweep + MMF yield (~4.7% Mar 2024)");
    println!("    - Travel booking with policy + duty-of-care + corp rates");
    println!("    - Expense capture: receipts via SMS, photo, email — auto-matched to card txns");
    println!("    - Customizable spend policies (per-user limit, per-merchant restrictions)");
    println!("    - Multi-currency: USD, EUR, GBP, CAD card spend with no FX fees on Premium");
    println!("    - Bill Pay (Premium) — vendor invoice OCR → approval → ACH/wire/check");
    println!("    - Embeddings: integrates with QuickBooks, NetSuite, Sage, Xero, Workday");
    println!("    - Brex Empower (AI assistant): natural-language spend queries + agent actions");
    println!("  Rewards: Brex card cashback varies (1-7x on certain categories like SaaS, Apple, dining)");
    println!("          Brex Points redeemable for travel, statements, gift cards");
    println!("  Customers: 30K startups + scale-ups — Anthropic, Coinbase, Robinhood, Plaid, Notion");
    println!("            historic 'YC startup card' — every YC batch uses Brex by default");
    println!("  Critique: SMB exit angered many small customers (now back to SMB via Essentials);");
    println!("           Ramp's spend-savings AI eclipsed Brex's earlier mindshare;");
    println!("           2023 layoffs raised questions about path to profitability");
    println!("  Differentiator: deep startup-CFO product (Cash account + Premium) + premium category rewards");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "brex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_brex(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_brex};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/brex"), "brex");
        assert_eq!(basename(r"C:\bin\brex.exe"), "brex.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("brex.exe"), "brex");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_brex(&["--help".to_string()], "brex"), 0);
        assert_eq!(run_brex(&["-h".to_string()], "brex"), 0);
        assert_eq!(run_brex(&["--version".to_string()], "brex"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_brex(&[], "brex"), 0);
    }
}
