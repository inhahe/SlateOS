#![deny(clippy::all)]

//! ramp-cli — Slate OS Ramp (corporate cards + spend mgmt, fastest-growing fintech)
//!
//! Single personality: `ramp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ramp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ramp [OPTIONS]");
        println!("Ramp (Slate OS) — Corporate cards + expense + bill pay + accounting automation");
        println!();
        println!("Options:");
        println!("  --card                 Issue virtual or physical Ramp card");
        println!("  --bill-pay             Bill Pay (AP automation)");
        println!("  --travel               Ramp Travel");
        println!("  --plus                 Ramp Plus ($15/user/mo — adds workflows)");
        println!("  --enterprise           Ramp Enterprise");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ramp 2024 (Slate OS)"); return 0; }
    println!("Ramp 2024 (Slate OS)");
    println!("  Vendor: Ramp Business Corporation (New York, NY — founded 2019)");
    println!("  Founders: Eric Glyman + Karim Atiyeh + Gene Lee (previously Paribus, sold to Capital One 2016)");
    println!("  Funding: Founders Fund, Sequoia, Stripe, Khosla, Greylock");
    println!("          $1.4B+ raised");
    println!("          $13B valuation Apr 2024 (down from $8.1B in 2022, up from $11.8B)");
    println!("  Strategy: 'first card built for saving money, not earning rewards'");
    println!("           kill Brex/AmEx by undercutting on rewards but adding spend-cutting AI");
    println!("           Brex+Ramp+Mercury define the modern startup fintech stack");
    println!("  Scale: 25,000+ customer companies");
    println!("        $10B+ annualized transaction volume");
    println!("        ~$1B revenue est. 2024");
    println!("  Pricing:");
    println!("    Free (no per-user fee for card + bill pay + expense reports)");
    println!("    Plus $15/user/mo (advanced workflows + procurement + multi-entity)");
    println!("    Enterprise — custom");
    println!("  Killer features:");
    println!("    - Cashback 1.5% on all Ramp card spend (some categories higher)");
    println!("    - Free unlimited virtual + physical cards");
    println!("    - 'Savings Insights' — AI flags duplicate subscriptions, price hikes, unused seats");
    println!("    - Bill Pay with OCR + 2-way match (PO + invoice + receipt)");
    println!("    - Vendor management (consolidate all SaaS/vendor spend in one place)");
    println!("    - Procurement (intake forms + approval workflows)");
    println!("    - Travel (book + manage + auto-policy-enforce)");
    println!("    - Ramp Treasury (4%+ APY business savings on idle cash)");
    println!("  Underwriting: 'fund-based' — credit limits based on cash in bank, not personal guarantee");
    println!("  Integrations: QuickBooks, NetSuite, Sage Intacct, Xero, Workday, Microsoft Dynamics");
    println!("  AI Push: 'Ramp Intelligence' — agentic finance ops (auto-categorize, auto-approve, auto-pay)");
    println!("  Customers: 25K+ — Anduril, Webflow, Vimeo, Shopify (parts), Notion, ClickUp");
    println!("            sweet spot: 50-1000 employees");
    println!("  Critique: cashback lower than premium Brex tiers");
    println!("           growing fast → occasional support friction");
    println!("           less mature international (US-focused)");
    println!("  Differentiator: spend-savings AI angle + 1.5% cashback floor + free spend platform — Concur killer");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ramp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ramp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ramp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ramp"), "ramp");
        assert_eq!(basename(r"C:\bin\ramp.exe"), "ramp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ramp.exe"), "ramp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ramp(&["--help".to_string()], "ramp"), 0);
        assert_eq!(run_ramp(&["-h".to_string()], "ramp"), 0);
        let _ = run_ramp(&["--version".to_string()], "ramp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ramp(&[], "ramp");
    }
}
