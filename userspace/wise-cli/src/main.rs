#![deny(clippy::all)]
//! wise-cli — personality CLI for Wise (formerly TransferWise), the
//! mid-market exchange-rate cross-border payments company.
//!
//! Founded 2011 in London by Kristo Käärmann and Taavet Hinrikus (an
//! early Skype employee) after they realised they were both shuffling
//! money between Estonia and the UK and paying enormous high-street-bank
//! FX margins to do it. The product idea: route currency between
//! peer-matched accounts at the real mid-market interbank rate, charge
//! a small explicit fee, and never bake the margin into the FX rate.
//! Listed direct on the London Stock Exchange July 2021 — Europe's
//! biggest tech direct listing — and renamed from TransferWise to Wise.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Wise mid-market cross-border payments personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Käärmann+Hinrikus 2011 London; LSE direct 2021");
    println!("    pricing       Real mid-market rate + explicit small fee");
    println!("    multicurrency Wise Account: hold 40+ currencies + local details");
    println!("    business      Wise Business + Wise Platform API");
    println!("    rails         Direct connections to local payment rails");
    println!("    debitcard     Wise debit card with FX at point of sale");
    println!("    regulation    EMI/MTL licences, EU + UK + US oversight");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("wise-cli 0.1.0 (mid-market-rate personality build)"); }

fn run_about() {
    println!("Wise plc (formerly TransferWise Ltd).");
    println!("  Founded:    2011, London, UK.");
    println!("  Founders:   Kristo Käärmann (CEO), Taavet Hinrikus (early Skype).");
    println!("  Listing:    Direct listing on LSE Jul 2021 (ticker WISE),");
    println!("              biggest UK tech direct listing.");
    println!("  Rebrand:    TransferWise -> Wise around the listing.");
    println!("  Mission:    'Money without borders, instant, convenient,");
    println!("              transparent, and eventually free.'");
    println!("  Volume:     Hundreds of billions in annualised cross-border flow.");
}

fn run_pricing() {
    println!("Pricing — explicit fee, no FX margin.");
    println!("  Always uses the real interbank mid-market rate (XE-style).");
    println!("  Fee shown up front, before the transfer, in the source currency.");
    println!("  Typical retail fee: 0.4-0.6% for major corridors; less for big.");
    println!("  Comparison: high-street banks bake 2-4% margins into the FX rate.");
    println!("  Founding pitch was a literal 'price comparison' page vs banks.");
}

fn run_multicurrency() {
    println!("Wise Account — multi-currency wallet.");
    println!("  Hold balances in 40+ currencies in a single account.");
    println!("  Get local account details (account+sort code GBP, IBAN EUR,");
    println!("  routing+account USD, BSB AUD) — receive like a local.");
    println!("  Hold and convert at the mid-market rate when convenient.");
    println!("  Earn interest on USD/EUR/GBP balances on supported tiers.");
}

fn run_business() {
    println!("Wise Business + Wise Platform.");
    println!("  Wise Business: multi-currency biz account, batch payouts,");
    println!("  bank-feed accounting integrations (Xero, QuickBooks, Sage).");
    println!("  Wise Platform: white-label API for banks + fintechs;");
    println!("  letters them embed Wise's FX + cross-border rails behind their own brand.");
    println!("  Customers: Monzo, N26, Bank Mandiri, Standard Chartered, Google Pay.");
}

fn run_rails() {
    println!("Local payment rails — the operational moat.");
    println!("  Direct connections to ~6 local payment systems where regulators allow:");
    println!("  Faster Payments (UK), SEPA Instant (EU), FedWire/ACH (US),");
    println!("  PayNet (MY), UPI (IN), NPP (AU).");
    println!("  Bypasses correspondent banking SWIFT chain for those corridors.");
    println!("  Result: 60%+ of transfers arrive in under 20 seconds.");
}

fn run_debitcard() {
    println!("Wise debit card.");
    println!("  Mastercard or Visa depending on issuing region.");
    println!("  Spend in any currency — converts at the mid-market rate at PoS.");
    println!("  Free ATM withdrawals up to a monthly limit per region.");
    println!("  Tap-to-pay + Apple Pay + Google Pay.");
    println!("  Popular with digital nomads + frequent travellers + expats.");
}

fn run_regulation() {
    println!("Regulatory footprint.");
    println!("  UK: EMI authorised by the FCA.");
    println!("  EU: Belgian National Bank licensed e-money institution.");
    println!("  US: state-by-state Money Transmitter Licences (MTLs) covering");
    println!("       every state where Wise operates (largest set in the industry).");
    println!("  Singapore: MAS Major Payment Institution licence.");
    println!("  Audit: PwC primary, public quarterly results post-listing.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  16M+ customers globally (consumer + business).");
    println!("  Embedded users: Monzo (FX), N26 (cross-border), Google Pay,");
    println!("  Standard Chartered, Mandiri Bank.");
    println!("  Heavy use among expats, digital nomads, ecom sellers paying");
    println!("  overseas suppliers, freelancers receiving international fees.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "wise-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "pricing" => run_pricing(),
        "multicurrency" => run_multicurrency(),
        "business" => run_business(),
        "rails" => run_rails(),
        "debitcard" => run_debitcard(),
        "regulation" => run_regulation(),
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
        run_pricing();
        run_multicurrency();
        run_business();
        run_rails();
        run_debitcard();
        run_regulation();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("wise-cli");
        print_version();
    }
}
