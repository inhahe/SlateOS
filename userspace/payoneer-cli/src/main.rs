#![deny(clippy::all)]
//! payoneer-cli — personality CLI for Payoneer, the marketplace + freelancer
//! cross-border payments veteran.
//!
//! Founded 2005 in New York by Yuval Tal (now retired chair). For years
//! Payoneer was the de-facto answer to "how do non-US Amazon/Upwork/eBay
//! sellers receive USD payouts" — they'd open a virtual US receiving
//! account in Payoneer and convert + withdraw locally. Went public via
//! SPAC June 2021 (Nasdaq:PAYO) at a $3.3B valuation. Headquartered in
//! New York with major engineering offices in Tel Aviv. Counts a
//! particularly large Israeli + Filipino + Pakistani + Latam freelancer base.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Payoneer marketplace + freelancer payments personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Yuval Tal 2005 NYC; SPAC IPO Nasdaq:PAYO Jun 2021");
    println!("    receiving     Virtual US/EU/UK/JP/CN receiving accounts");
    println!("    marketplaces  Amazon, Upwork, Fiverr, Airbnb host payouts");
    println!("    card          Prepaid Mastercard for instant access to balance");
    println!("    capital       Working-capital advances against marketplace receivables");
    println!("    business      AP/AR + invoice + supplier payments");
    println!("    pricing       Lower fees than SWIFT for emerging-market freelancers");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("payoneer-cli 0.1.0 (marketplace-receivable personality build)"); }

fn run_about() {
    println!("Payoneer Global Inc.");
    println!("  Founded:    2005, New York.");
    println!("  Founder:    Yuval Tal (now retired chair); current CEO John Caplan.");
    println!("  HQ:         New York (commercial), Tel Aviv (engineering).");
    println!("  Listing:    Nasdaq:PAYO via SPAC merger Jun 2021 at $3.3B.");
    println!("  Original wedge: cross-border payouts for marketplace sellers");
    println!("              + freelancers who had no PayPal access in their country");
    println!("              (Israel, India, Pakistan, Bangladesh, Philippines, etc).");
    println!("  Volume:     $60B+ annualised processed.");
}

fn run_receiving() {
    println!("Receiving accounts.");
    println!("  Virtual receiving account details in USD, EUR, GBP, JPY, AUD, CAD,");
    println!("  CNY, HKD, MXN, SGD, AED, ZAR.");
    println!("  Marketplaces pay these accounts as if they were a domestic supplier.");
    println!("  Payoneer holds the balance; user converts to local currency + withdraws.");
    println!("  Major selling point: no need to open a US LLC + Wells Fargo account");
    println!("  just to receive Amazon US payouts.");
}

fn run_marketplaces() {
    println!("Marketplace integrations.");
    println!("  Amazon: seller payouts in 20+ countries, including FBA + Vine.");
    println!("  Upwork: freelancer withdrawal default for many countries.");
    println!("  Fiverr: same — seller withdraws to Payoneer balance.");
    println!("  Airbnb host payouts in regions where direct deposit is awkward.");
    println!("  Walmart Marketplace, Etsy, eBay, Wayfair, Wish.");
    println!("  Stock images: Shutterstock, Adobe Stock, Pond5 contributors.");
}

fn run_card() {
    println!("Payoneer Mastercard.");
    println!("  Prepaid card linked to the Payoneer balance.");
    println!("  Spend in local currency anywhere Mastercard is accepted.");
    println!("  ATM withdrawal in local currency.");
    println!("  Spend in any of the held balance currencies — auto-FX at withdrawal.");
    println!("  Multiple cards per account for businesses with team members.");
}

fn run_capital() {
    println!("Working capital advances.");
    println!("  Payoneer Capital Advance: receive a percentage of expected future");
    println!("  marketplace payouts up front; repay automatically from incoming flow.");
    println!("  Underwritten on Payoneer's view of the seller's marketplace history.");
    println!("  Use case: e-commerce sellers needing to fund next inventory order.");
    println!("  Available in select countries; flat-fee pricing, not interest.");
}

fn run_business() {
    println!("Business AP/AR + invoicing.");
    println!("  Invoice clients globally; pay link goes via Payoneer rails.");
    println!("  AP: pay suppliers, contractors, partners in 150+ countries.");
    println!("  Mass-payouts API for marketplaces + creator platforms.");
    println!("  Tax form collection: W-8BEN, W-9, automated reporting.");
    println!("  Accounting: QuickBooks Online + Xero + NetSuite integrations.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Marketplace deposits: free or low fixed fee (depends on source).");
    println!("  Withdraw to local bank: ~1.5-2% currency conversion margin.");
    println!("  Card spend: small FX margin on cross-currency PoS.");
    println!("  Business invoice payment: per-invoice + percentage.");
    println!("  Working capital: flat fee on advanced amount, varying by region.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Amazon, Upwork, Fiverr, Airbnb, Walmart Marketplace, Pinterest creators,");
    println!("  Vimeo, AdMob, AdSense (historical), Etsy sellers, eBay, Wayfair.");
    println!("  Large freelancer + seller base in Israel, Philippines, Pakistan,");
    println!("  India, Bangladesh, Vietnam, Egypt, Argentina.");
    println!("  Common payout method for content creators in countries");
    println!("  where Stripe / PayPal coverage is partial or absent.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "payoneer-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "receiving" => run_receiving(),
        "marketplaces" => run_marketplaces(),
        "card" => run_card(),
        "capital" => run_capital(),
        "business" => run_business(),
        "pricing" => run_pricing(),
        "customers" => run_customers(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs() {
        run_about();
        run_receiving();
        run_marketplaces();
        run_card();
        run_capital();
        run_business();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("payoneer-cli");
        print_version();
    }
}
