#![deny(clippy::all)]
//! revolut-cli — personality CLI for Revolut, the all-in-one European
//! neobank turned global super-app.
//!
//! Founded July 2015 in London by Nikolay Storonsky (a former Lehman + Credit
//! Suisse derivatives trader) and Vlad Yatsenko (engineer, ex-Deutsche
//! Bank / Credit Suisse). Started as a no-FX-fee multi-currency prepaid
//! card aimed at frequent travellers; aggressively expanded into stocks,
//! crypto, savings, lending, business banking, eSIM, and joint accounts.
//! Granted a full UK banking licence in July 2024 with restrictions
//! (mobilisation phase). Reported 50M+ retail customers and ~$45B
//! secondary-market valuation as of August 2024.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Revolut all-in-one neobank super-app personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Storonsky+Yatsenko 2015 London; UK bank licence 2024");
    println!("    multicurrency 36+ currencies + FX with weekday allowance");
    println!("    cards         Physical + virtual + disposable cards");
    println!("    invest        Stocks, ETFs, commodities, crypto in-app");
    println!("    business      Revolut Business + Revolut Pro for SMBs");
    println!("    superapp      eSIM, lounges, joint accounts, savings, lending");
    println!("    licences      UK bank + Lithuanian EU bank + US partner");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("revolut-cli 0.1.0 (super-app personality build)"); }

fn run_about() {
    println!("Revolut Ltd / Revolut Bank UAB.");
    println!("  Founded:    July 2015, London, UK.");
    println!("  Founders:   Nikolay Storonsky (CEO, ex-Lehman/CS derivatives),");
    println!("              Vladyslav Yatsenko (CTO, ex-DB/CS engineering).");
    println!("  Funding:    $800M Series E 2021 at $33B; secondary Aug 2024 ~$45B.");
    println!("  Licences:   Lithuanian EU bank licence (active);");
    println!("              UK PRA/FCA bank licence Jul 2024 (mobilisation phase).");
    println!("  Customers:  50M+ retail accounts globally, ~10M business accounts.");
    println!("  Posture:    Aggressive feature-shipping cadence, sometimes");
    println!("              criticised in regulatory press for the pace.");
}

fn run_multicurrency() {
    println!("Multi-currency wallet.");
    println!("  Hold 36+ currencies in one account.");
    println!("  In-app FX at the interbank rate on weekdays up to a monthly");
    println!("  allowance (then a small markup, larger on weekends).");
    println!("  Local account details in GBP, EUR, USD, RON, PLN, etc.");
    println!("  Designed for travellers, expats, freelancers paid internationally.");
}

fn run_cards() {
    println!("Card products.");
    println!("  Physical card: standard, premium metal, ultra, business card.");
    println!("  Virtual cards: spawn per-subscription or per-merchant.");
    println!("  Disposable virtual cards: card number rotates after each use.");
    println!("  Apple Pay + Google Pay + Garmin Pay supported.");
    println!("  Card-control: freeze, geo-lock, online-only toggle, magstripe-off.");
}

fn run_invest() {
    println!("Invest + crypto in-app.");
    println!("  Fractional US + EU stocks + ETFs, commission-free up to a monthly cap.");
    println!("  Commodities: gold + silver fractional holdings.");
    println!("  Crypto: 200+ tokens, in-app buy/sell/hold/transfer.");
    println!("  Robo advisor: index-tracking portfolio with risk profile.");
    println!("  Savings: instant-access EUR/GBP/USD interest-bearing vaults.");
}

fn run_business() {
    println!("Revolut Business + Revolut Pro.");
    println!("  Business: multi-currency biz account, expense cards, batch payouts,");
    println!("  bank-feed accounting integrations (Xero, QuickBooks, Sage, Zoho).");
    println!("  Pro: account for freelancers, distinct from personal account,");
    println!("  invoicing + payment link tools.");
    println!("  Both: API access for programmatic payments + accounting.");
}

fn run_superapp() {
    println!("Super-app sprawl.");
    println!("  eSIM data plans for travel, no roaming charges, pay-as-you-go.");
    println!("  Airport lounge access (LoungeKey-style) on premium tiers.");
    println!("  Joint accounts: shared pots between users.");
    println!("  Children's accounts + parental controls.");
    println!("  Buy-now-pay-later instalments in some markets.");
    println!("  Stays: book hotels through the app with cashback.");
    println!("  Insurance: device, travel, medical on supported tiers.");
}

fn run_licences() {
    println!("Regulatory footprint.");
    println!("  UK     PRA + FCA licensed bank since Jul 2024 (mobilisation).");
    println!("  EU     Bank of Lithuania licensed bank — passport across EEA.");
    println!("  US     Partners with Lead Bank (FDIC-insured); no own US charter.");
    println!("  AU     Australian Financial Services Licence.");
    println!("  Crypto Various per-market crypto licences.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  50M+ retail customers across UK, EU, US, AU, JP, SG, Brazil.");
    println!("  Heavy in Eastern Europe, UK, Ireland, Iberia, France.");
    println!("  ~10M Revolut Business customers — SMBs + freelancers.");
    println!("  Popular among digital nomads + frequent travellers + remote workers.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "revolut-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "multicurrency" => run_multicurrency(),
        "cards" => run_cards(),
        "invest" => run_invest(),
        "business" => run_business(),
        "superapp" => run_superapp(),
        "licences" => run_licences(),
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
        run_multicurrency();
        run_cards();
        run_invest();
        run_business();
        run_superapp();
        run_licences();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("revolut-cli");
        print_version();
    }
}
