#![deny(clippy::all)]
//! affirm-cli — personality CLI for Affirm, the transparent-BNPL pioneer.
//!
//! Founded 2012 in San Francisco by Max Levchin (PayPal co-founder, ex-Slide
//! CEO, ex-Yelp board) with Jeffrey Kaditz and Nathan Gettings. Listed on
//! Nasdaq as AFRM in January 2021. The defining positioning has always been
//! "no late fees, no compounding interest, no fine print" — a direct
//! contrast to credit-card revolving debt and to some peer BNPL late-fee
//! economics. Tight strategic alignment with Shopify: Shop Pay Installments
//! is white-labelled Affirm. Affirm Card (launched 2023) is a debit card
//! with optional pay-over-time on each transaction, blurring the BNPL +
//! debit + revolving-credit line.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Affirm transparent-BNPL personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Max Levchin 2012 SF; Nasdaq:AFRM Jan 2021");
    println!("    products      Pay in 4, monthly financing, Affirm Card");
    println!("    shopify       Shop Pay Installments = white-labelled Affirm");
    println!("    merchants     Major merchant integrations + Amazon deal");
    println!("    transparency  No-late-fee + no-compounding-interest ethos");
    println!("    underwriting  Per-transaction credit decision, not revolving line");
    println!("    pricing       Merchant MDR + consumer APR mechanics");
    println!("    customers     Selected named merchants + partners");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("affirm-cli 0.1.0 (transparent-BNPL personality build)"); }

fn run_about() {
    println!("Affirm Holdings, Inc.");
    println!("  Founded:    2012, San Francisco.");
    println!("  Founders:   Max Levchin (CEO; PayPal co-founder, ex-Slide CEO,");
    println!("              ex-Yelp board chair), Jeffrey Kaditz, Nathan Gettings.");
    println!("  Listing:    Nasdaq:AFRM, IPO Jan 13 2021 at $49/share ($12B day-1 cap).");
    println!("  Volume:     $34B+ annualised GMV processed across the network.");
    println!("  Active:     ~19M consumers, ~290K merchant integrations.");
    println!("  Differentiator: transparent BNPL — fixed total upfront, no late fees,");
    println!("              no compounding interest, no fee for non-payment beyond");
    println!("              the credit-reporting consequences.");
}

fn run_products() {
    println!("Product lineup.");
    println!("  Pay in 4:     four interest-free biweekly instalments at checkout.");
    println!("  Pay in 30:    full balance in 30 days, no interest.");
    println!("  Monthly:      3-60 month fixed-APR instalment loans for larger");
    println!("                tickets (e.g. Peloton, mattresses, electronics).");
    println!("  Adaptive Checkout: shows the consumer the optimal mix of");
    println!("                interest-free and monthly options for their cart.");
    println!("  Affirm Card:  Visa debit card with the ability to convert any");
    println!("                pending or recent transaction into a pay-over-time plan.");
}

fn run_shopify() {
    println!("Shopify partnership.");
    println!("  Shop Pay Installments — Shopify's native BNPL — is white-labelled");
    println!("  Affirm under the hood. Exclusive deal announced Jul 2020,");
    println!("  predating the Nasdaq IPO and a major IPO-narrative input.");
    println!("  Shopify took ~3% Affirm equity warrants as part of the deal.");
    println!("  Every US Shopify merchant can offer Affirm at checkout in one click,");
    println!("  giving Affirm massive merchant-side reach without per-merchant sales.");
    println!("  Levchin sits on the Shopify board.");
}

fn run_merchants() {
    println!("Merchant network.");
    println!("  Walmart (US in-store + online — multi-year exclusive BNPL deal).");
    println!("  Amazon (US checkout integration since Aug 2021).");
    println!("  Target, Best Buy, Peloton (historical heavyweight), Apple (via");
    println!("  Apple Pay Affirm option for monthly instalments on iPhones).");
    println!("  Travel: Expedia, Vrbo, Priceline, Booking.com, American Airlines.");
    println!("  Electronics/home: Samsung, Sony, Casper, Wayfair.");
    println!("  ~290,000 merchants total across the Affirm network.");
}

fn run_transparency() {
    println!("Transparency ethos.");
    println!("  No late fees — ever. Public commitment, marketed heavily.");
    println!("  No compounding interest — interest is calculated on the");
    println!("  original principal, never on accrued interest.");
    println!("  No deferred-interest tricks (the 'pay 0% for 12 months, then we");
    println!("  retroactively charge you' pattern endemic to store-card financing).");
    println!("  Total cost is shown up-front before the consumer confirms.");
    println!("  This positioning is the brand: presented in every ad, every");
    println!("  merchant placement, every investor deck.");
}

fn run_underwriting() {
    println!("Underwriting model.");
    println!("  Per-transaction credit decision — not a revolving credit line.");
    println!("  Each cart is underwritten on its own merits: cart contents,");
    println!("  merchant category, consumer's Affirm history, soft credit pull.");
    println!("  A consumer may be approved at Merchant A and declined at Merchant B");
    println!("  the same day — this is the design, not a bug.");
    println!("  Cross River Bank (US) is the originating partner bank.");
    println!("  Affirm holds + services the loans; some are securitised + sold");
    println!("  through asset-backed securitisation programs to fund growth.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Consumer:");
    println!("    Pay in 4: 0% APR, no fees, no late fees.");
    println!("    Monthly:  0-36% APR depending on credit + merchant subsidy.");
    println!("              Interest is simple, on original principal.");
    println!("              Some merchants subsidise to offer true 0% APR.");
    println!("  Merchant:");
    println!("    MDR (merchant discount rate) ~3-6%% of order value, higher than");
    println!("    cards but justified by lift in conversion + AOV + new-customer mix.");
    println!("    Higher rates for 0%-APR-to-consumer offers (merchant absorbs subsidy).");
}

fn run_customers() {
    println!("Selected merchant + brand partners:");
    println!("  Mega-retail:  Walmart, Amazon, Target, Best Buy, Costco.");
    println!("  Travel:       Expedia, Vrbo, Priceline, American Airlines, Booking.");
    println!("  Lifestyle:    Peloton (legacy heavyweight), Mirror, Tonal, Casper.");
    println!("  Electronics:  Samsung, Sony, Dell, Lenovo, Apple (via Apple Pay).");
    println!("  Home:         Wayfair, West Elm, Pottery Barn, Crate & Barrel.");
    println!("  Shopify long tail: every Shopify merchant via Shop Pay Installments.");
    println!("  Geographic:   US + Canada primary; UK launched 2023; Australia launched 2024.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "affirm-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "products" => run_products(),
        "shopify" => run_shopify(),
        "merchants" => run_merchants(),
        "transparency" => run_transparency(),
        "underwriting" => run_underwriting(),
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
        run_products();
        run_shopify();
        run_merchants();
        run_transparency();
        run_underwriting();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("affirm-cli");
        print_version();
    }
}
