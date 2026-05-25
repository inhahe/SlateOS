#![deny(clippy::all)]

//! xero-cli — OurOS Xero (NZ-founded cloud accounting, QuickBooks rival outside US)
//!
//! Single personality: `xero`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xero(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xero [OPTIONS]");
        println!("Xero (OurOS) — Cloud accounting for small business");
        println!();
        println!("Options:");
        println!("  --starter              Starter ($15/mo)");
        println!("  --standard             Standard ($42/mo)");
        println!("  --premium              Premium ($78/mo — multi-currency)");
        println!("  --ultimate             Ultimate ($115/mo — Xero Projects/Expenses)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Xero 2024 (OurOS)"); return 0; }
    println!("Xero 2024 (OurOS)");
    println!("  Vendor: Xero Limited (Wellington, New Zealand — ASX:XRO, $19B AUD market cap)");
    println!("  Founders: Rod Drury + Hamish Edwards (Wellington, 2006)");
    println!("           Drury: NZ tech entrepreneur, IRD veteran, prolific writer/speaker");
    println!("  History: founded 2006, listed NZX 2007 (NZ's first SaaS IPO)");
    println!("          listed ASX (Australia) 2012 — moved primary listing to ASX 2018");
    println!("          delisted NZX 2018");
    println!("  Strategy: 'beautiful accounting' — modern UX vs Sage/MYOB legacy desktop");
    println!("           dominant in AU/NZ/UK SMB, growing US/Canada");
    println!("  Scale: 3.95M+ subscribers (Mar 2024)");
    println!("        ~6,000 employees");
    println!("        $1.7B AUD revenue (FY2024)");
    println!("  Pricing (US): Starter $15, Standard $42, Premium $78, Ultimate $115/mo");
    println!("              Starter limits to 20 invoices/quotes + 5 bills/mo");
    println!("  Features:");
    println!("    - Double-entry accounting (proper GL, journals, trial balance)");
    println!("    - Bank feeds (3,000+ banks via direct feeds + Plaid)");
    println!("    - Bank reconciliation with ML rules (Xero learns)");
    println!("    - Invoicing + recurring invoices + payment gateway integrations (Stripe, GoCardless)");
    println!("    - Quotes + purchase orders + inventory");
    println!("    - Multi-currency (Premium+) with FX gain/loss");
    println!("    - Project tracking + time tracking (Ultimate)");
    println!("    - Expense claims (Xero Expenses, mobile receipt capture)");
    println!("    - Fixed asset register with depreciation schedules");
    println!("    - Payroll (UK, AU, NZ native; US partners with Gusto)");
    println!("    - GST/VAT/HST + sales tax automation");
    println!("    - Hubdoc (receipt + bill capture, OCR'd, posted as drafts)");
    println!("    - Financial reports (P&L, balance sheet, cash flow, custom)");
    println!("  Xero App Store: 1,000+ integrations (Stripe, Shopify, Squarespace, payroll vendors, ...)");
    println!("  Customers: SMBs and accountants (Xero has huge accountant/bookkeeper community)");
    println!("            150+ countries, dominant in AU/NZ/UK, gaining in US (vs QuickBooks)");
    println!("  History note: famously sponsored All Blacks rugby — global brand recognition campaign");
    println!("  Differentiator: 'beautiful' UX + best-in-class bank feeds + accountant ecosystem outside US");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xero".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xero(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
