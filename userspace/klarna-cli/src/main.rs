#![deny(clippy::all)]
//! klarna-cli — personality CLI for Klarna, the Swedish BNPL pioneer.
//!
//! Founded 2005 in Stockholm by Sebastian Siemiatkowski, Niklas Adalberth
//! and Victor Jacobsson while at the Stockholm School of Economics. Created
//! the modern Buy-Now-Pay-Later category long before the 2020 pandemic boom
//! made it a household phrase. Majority-backed by Sequoia (Michael Moritz
//! chaired the board for years). Peak private valuation $45.6B mid-2021;
//! re-priced down to $6.7B in the Jul 2022 down-round (one of the most
//! discussed venture re-rates of the cycle); since re-rated upward as
//! profitability returned. NYSE IPO paperwork filed Nov 2024.
//! Famous for an aggressive AI-first messaging pivot in 2023-2024 and for
//! Siemiatkowski personally claiming the equivalent of 700 FTE of work
//! was being absorbed by an internal LLM platform.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Klarna BNPL + AI-first consumer-finance personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Siemiatkowski/Adalberth/Jacobsson 2005 Stockholm");
    println!("    bnpl          Pay in 4, Pay in 30 days, monthly financing");
    println!("    merchants     Checkout + Klarna-branded merchant network");
    println!("    app           Shopping app: discovery, price-drop, wishlists");
    println!("    ai            AI-first 2023+ pivot; OpenAI-powered assistant");
    println!("    pricing       Merchant take-rate; consumer no-fee positioning");
    println!("    regulatory    UK FCA BNPL regulation; Swedish bank licence");
    println!("    customers     Selected named merchants + brand partners");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("klarna-cli 0.1.0 (BNPL-pioneer personality build)"); }

fn run_about() {
    println!("Klarna Bank AB (publ).");
    println!("  Founded:    2005, Stockholm.");
    println!("  Founders:   Sebastian Siemiatkowski (CEO), Niklas Adalberth,");
    println!("              Victor Jacobsson — Stockholm School of Economics.");
    println!("  Backers:    Sequoia (majority; Michael Moritz long-time chair),");
    println!("              Silver Lake, Permira, Commonwealth Bank of Australia,");
    println!("              SoftBank Vision Fund (entered at peak 2021).");
    println!("  Licence:    Full Swedish bank licence since 2017 (SFSA).");
    println!("  Valuation:  $45.6B Jun 2021 → $6.7B Jul 2022 down-round → re-rated");
    println!("              upward 2023-2024 on return to profitability.");
    println!("  IPO:        S-1 / F-1 filed with SEC Nov 2024 for NYSE listing.");
}

fn run_bnpl() {
    println!("Buy-Now-Pay-Later product lineup.");
    println!("  Pay in 4:     four equal interest-free instalments, biweekly.");
    println!("  Pay in 30:    full balance due in 30 days, no interest.");
    println!("  Financing:    6-36 month instalment loans, interest applies.");
    println!("  One-time card: virtual card spend at any merchant in-app.");
    println!("  Klarna Card:  physical Visa with pay-later toggle at checkout.");
    println!("  Pioneered the category in 2005 — pre-dates Afterpay (2014) and");
    println!("  Affirm (2012) by most of a decade.");
}

fn run_merchants() {
    println!("Merchant network.");
    println!("  Klarna Checkout: full hosted checkout UI (huge in DACH + Nordics).");
    println!("  Klarna Payments: drop-in payment-method widget for existing carts.");
    println!("  500,000+ merchant integrations globally.");
    println!("  Deep coverage: H&M, IKEA, Sephora, Etsy, Macy's, Nike, Adidas,");
    println!("  Wayfair, Expedia, Airbnb, Footlocker, Saks, Bloomingdale's.");
    println!("  Shopify integration via Klarna Payments app + Klarna On-site Messaging.");
}

fn run_app() {
    println!("Klarna shopping app.");
    println!("  150M+ active consumers globally.");
    println!("  Product discovery + price-drop alerts + wishlists + cashback.");
    println!("  In-app browser: tap Klarna button on any site to BNPL even where");
    println!("  the merchant has not integrated.");
    println!("  Klarna Plus subscription: $7.99/mo, no service fees + double rewards.");
    println!("  Recently a heavy product-marketing push around AI-driven shopping.");
}

fn run_ai() {
    println!("AI-first pivot.");
    println!("  Public partnership announcement with OpenAI Mar 2024.");
    println!("  In-app AI assistant: handles 2/3 of customer-service chats globally,");
    println!("  equivalent (per Klarna) to ~700 FTE of work.");
    println!("  Internal Kair platform: image gen, marketing-copy gen, internal Q&A");
    println!("  on Klarna's own knowledge base, code-completion across engineering.");
    println!("  Public messaging from Siemiatkowski has been the most explicit");
    println!("  'AI is replacing headcount' CEO narrative of any large fintech.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Consumer:   $0 for Pay in 4 / Pay in 30 if paid on time.");
    println!("              Interest on multi-month financing (region-dependent APR).");
    println!("              Late fees in some markets, capped or banned in others.");
    println!("  Merchant:   transaction fee ~3-6%% of order value (vs. 2-3%% card),");
    println!("              justified by Klarna assuming credit risk + lifting AOV.");
    println!("              Variable + fixed-fee components by region + product.");
    println!("  Float:      Klarna funds the consumer side, repays merchant upfront,");
    println!("              collects from consumer over time — net interest income");
    println!("              + transaction take is the core economic model.");
}

fn run_regulatory() {
    println!("Regulatory + credit posture.");
    println!("  Swedish bank licence (Finansinspektionen) since 2017 — formally a");
    println!("  bank, not just a fintech. Full deposit-taking permissions.");
    println!("  UK: HM Treasury announced BNPL will be brought under FCA");
    println!("  regulation; Klarna publicly supportive of the rules.");
    println!("  US: state-level licensing; no federal BNPL framework yet but");
    println!("  CFPB issued interpretive rule treating Pay-in-4 as credit cards");
    println!("  for dispute-rights purposes (May 2024).");
    println!("  EU Consumer Credit Directive II affects all EU BNPL from 2026.");
}

fn run_customers() {
    println!("Selected merchant + brand partners:");
    println!("  Apparel/beauty: H&M, Nike, Adidas, Sephora, Etsy, Foot Locker,");
    println!("    ASOS, Boohoo, Macy's, Saks, Bloomingdale's, Lululemon.");
    println!("  Home + travel:  IKEA, Wayfair, Expedia, Airbnb, Booking.com.");
    println!("  Marketplaces:   eBay (selected markets), Shopify merchants via app.");
    println!("  Heavy concentration in Sweden, Germany, UK, US, Australia.");
    println!("  Most-recent strategic push: deeper US merchant penetration ahead");
    println!("  of the NYSE listing.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "klarna-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "bnpl" => run_bnpl(),
        "merchants" => run_merchants(),
        "app" => run_app(),
        "ai" => run_ai(),
        "pricing" => run_pricing(),
        "regulatory" => run_regulatory(),
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
        run_bnpl();
        run_merchants();
        run_app();
        run_ai();
        run_pricing();
        run_regulatory();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("klarna-cli");
        print_version();
    }
}
