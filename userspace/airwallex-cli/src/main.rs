#![deny(clippy::all)]
//! airwallex-cli — personality CLI for Airwallex, the Australia-founded
//! global B2B payments + business banking platform.
//!
//! Founded 2015 in Melbourne by Jack Zhang (CEO), Lucy Liu, Max Li, Xijing
//! Dai, and Jacob Dai — reportedly inspired by the FX costs of trying to
//! import speciality coffee for a café Liu and Zhang co-owned in Melbourne.
//! Built its own multi-country payments infrastructure with direct domestic
//! clearing connections in many markets (rather than reselling SWIFT). Now
//! HQ'd in Singapore + London + San Francisco. Series F Apr 2022 valued
//! it at $5.5B; named one of the largest Australian-born tech companies.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Airwallex global B2B payments personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Zhang+Liu+Li+Dais 2015 Melbourne; ~$5.5B 2022");
    println!("    accounts      Global business accounts in 60+ currencies");
    println!("    payments      Domestic clearing in 110+ countries");
    println!("    cards         Multi-currency Visa cards, employee + virtual");
    println!("    fx            Wholesale FX with same-day conversions");
    println!("    embedded      Embedded finance for SaaS marketplaces");
    println!("    licences      EMI/MTL across UK/EU/AU/US/SG/HK/MY");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("airwallex-cli 0.1.0 (global-B2B-rails personality build)"); }

fn run_about() {
    println!("Airwallex.");
    println!("  Founded:    2015, Melbourne, Australia.");
    println!("  Founders:   Jack Zhang (CEO), Lucy Liu (President + co-founder),");
    println!("              Max Li, Xijing Dai, Jacob Dai.");
    println!("  Origin myth: Café-import-FX frustration spawned the company.");
    println!("  Funding:    Series F Apr 2022 at $5.5B post; ~$1B raised total.");
    println!("              Investors: Tencent, Sequoia, DST, Visa, Salesforce Ventures.");
    println!("  Global HQ:  Singapore + London + Hong Kong + Melbourne + SF.");
    println!("  Volume:     $100B+ annualised processed.");
}

fn run_accounts() {
    println!("Global Business Accounts.");
    println!("  Hold 60+ currencies in one account.");
    println!("  Receive in 23+ currencies with local account details.");
    println!("  Local clearing networks: SWIFT + 110+ countries via Airwallex's");
    println!("  own connections to local domestic rails.");
    println!("  Multi-entity: many sub-accounts per legal entity, group view.");
    println!("  Replaces the 'open a Wells Fargo USD account + DBS SGD account' problem.");
}

fn run_payments() {
    println!("Payments — collect + send globally.");
    println!("  Collect: cards + APMs in 180+ markets, single integration.");
    println!("  Pay out: 110+ countries via direct local rails where possible,");
    println!("  SWIFT where not.");
    println!("  Local rail examples: ACH (US), Faster Payments (UK), SEPA Instant");
    println!("  (EU), DBT (HK), BECS (AU), PromptPay (TH), UPI (IN).");
    println!("  Same-day settlement on many major corridors.");
}

fn run_cards() {
    println!("Cards — multi-currency Visa.");
    println!("  Physical + virtual cards for employees + departments.");
    println!("  Spend in any currency; charged to the matching balance.");
    println!("  Real-time spend controls + categorisation.");
    println!("  Cashback or rebates depending on region + tier.");
    println!("  Receipt + memo capture for accounting export (Xero/QB/Sage/NetSuite).");
}

fn run_fx() {
    println!("FX — wholesale rates, transparent margins.");
    println!("  100+ currency pairs, near-interbank rates for business customers.");
    println!("  Same-day or next-day settlement on major pairs.");
    println!("  Forwards (locked-rate FX for future delivery) on some pairs.");
    println!("  Real-time API for FX quote + book; programmatic conversion.");
    println!("  Treasury teams can hedge via API rather than via dealer phone.");
}

fn run_embedded() {
    println!("Embedded finance for SaaS + marketplaces.");
    println!("  White-label issued accounts + cards + transfers under client brand.");
    println!("  Marketplace payouts: split a single collected payment to N sellers.");
    println!("  Connected accounts model similar to Stripe Connect, more international.");
    println!("  KYC + compliance handled by Airwallex; client gets the user experience.");
}

fn run_licences() {
    println!("Regulatory footprint.");
    println!("  UK: EMI authorised by FCA.");
    println!("  EU: Lithuanian EMI licence (via subsidiary).");
    println!("  AU: AUSTRAC-registered, ASIC AFSL.");
    println!("  US: state-by-state Money Transmitter Licences.");
    println!("  SG: MAS Major Payment Institution licence.");
    println!("  HK: Money Service Operator licence.");
    println!("  MY: Bank Negara Malaysia approval.");
    println!("  CN: through China-specific licensed partners.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  SHEIN, Brex, McLaren Racing, Papaya Global, Plum, Cuvva,");
    println!("  Navan, Qantas Loyalty, Xero (partner integration), many Asian");
    println!("  e-commerce + creator-economy marketplaces.");
    println!("  Strong base of Australian + Chinese-diaspora-founded global SaaS.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "airwallex-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "accounts" => run_accounts(),
        "payments" => run_payments(),
        "cards" => run_cards(),
        "fx" => run_fx(),
        "embedded" => run_embedded(),
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
        run_accounts();
        run_payments();
        run_cards();
        run_fx();
        run_embedded();
        run_licences();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("airwallex-cli");
        print_version();
    }
}
