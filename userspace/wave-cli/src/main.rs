#![deny(clippy::all)]

//! wave-cli — OurOS Wave (Wave Apps — free accounting, now owned by H&R Block)
//!
//! Single personality: `wave`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wave(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wave [OPTIONS]");
        println!("Wave (OurOS) — Free accounting + invoicing for small business");
        println!();
        println!("Options:");
        println!("  --invoicing            Free unlimited invoicing");
        println!("  --accounting           Free double-entry accounting");
        println!("  --payments             Payments by Wave (transaction-fee)");
        println!("  --payroll              Wave Payroll (US/Canada, paid)");
        println!("  --advisors             Wave Advisors (paid bookkeeping/tax)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Wave 2024 (OurOS)"); return 0; }
    println!("Wave 2024 (OurOS)");
    println!("  Vendor: Wave Financial Inc. (Toronto, Canada — founded 2010)");
    println!("          acquired by H&R Block June 2019 for $537M USD");
    println!("  Founders: Kirk Simpson + James Lochrie + Glenn Allen");
    println!("  Origin: built as 'Tic' accounting tool, repositioned as ad-supported free for small biz");
    println!("  Strategy: FREE core accounting + invoicing forever — monetize via Payments + Payroll + Advisors");
    println!("           ad-supported for years; now ad-free post-H&R Block acquisition");
    println!("  Scale: ~500K paying users + ~3M free users lifetime");
    println!("        primarily Canada + US (limited international support)");
    println!("  Pricing:");
    println!("    - Accounting + Invoicing: FREE (unlimited transactions, unlimited invoices, double-entry GL)");
    println!("    - Wave Pro: $16/mo (auto-import bank txns, multi-business, unlimited receipts)");
    println!("    - Payments: 2.9% + 60¢ credit, 1% ACH (US), 1% bank payment (CA)");
    println!("    - Payroll: $20/mo + $6/employee (CA, NY, FL, TX, WA self-serve states), $40/mo elsewhere");
    println!("    - Wave Advisors: $149/mo bookkeeping coaching, $379+/mo full bookkeeping");
    println!("    - Wave Tax (US, ex-Hellotax/Wave Tax): tax filing — H&R Block synergy");
    println!("  Features (free):");
    println!("    - Unlimited invoices, recurring invoices, customer/vendor records");
    println!("    - Double-entry accounting with proper chart of accounts");
    println!("    - Bank/credit card transaction import (manual or via OFX/CSV — Wave Pro for auto-feeds)");
    println!("    - Receipt scanning (mobile app, OCR)");
    println!("    - Financial reports (P&L, balance sheet, cash flow, sales tax)");
    println!("    - Multi-business under one login");
    println!("    - Estimate → invoice → payment workflow");
    println!("    - Sales tax tracking (Canada GST/HST/PST, US state sales tax)");
    println!("    - Personal finance integration (track owner draws)");
    println!("  Customers: freelancers, solopreneurs, micro-businesses (1-10 employees)");
    println!("            very strong in Canada (Wave's home)");
    println!("  Critique: limited inventory, no multi-currency, no project tracking");
    println!("           NOT for businesses needing complex accounting");
    println!("           previously had aggressive ads (cleaned up post-acquisition)");
    println!("  Differentiator: legitimately free double-entry accounting — only major free option in the market");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wave".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wave(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wave};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wave"), "wave");
        assert_eq!(basename(r"C:\bin\wave.exe"), "wave.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wave.exe"), "wave");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wave(&["--help".to_string()], "wave"), 0);
        assert_eq!(run_wave(&["-h".to_string()], "wave"), 0);
        assert_eq!(run_wave(&["--version".to_string()], "wave"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wave(&[], "wave"), 0);
    }
}
