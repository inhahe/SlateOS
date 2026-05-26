#![deny(clippy::all)]
//! rapyd-cli — personality CLI for Rapyd, the global "fintech-as-a-service"
//! payments network.
//!
//! Founded 2016 by Arik Shtilman (CEO, ex-CTC/Telmap GPS founder),
//! Arik Shtilman + Omer Priel + Joel Yarbrough. Headquartered in London with
//! large engineering presence in Tel Aviv. The differentiator vs Stripe and
//! Adyen: Rapyd offers a single API that aggregates 900+ local payment
//! methods across 100+ countries — bank transfers in Brazil, OXXO cash
//! in Mexico, GrabPay in SE Asia, M-Pesa in Kenya — alongside cards.
//! Bought PayU's GPO business from Naspers for $610M (Aug 2023), and
//! acquired Valitor from Arion Bank in 2022 for $100M. Reached unicorn
//! status 2019; ~$15B valuation per 2021 Series E round.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Rapyd global-payments-as-a-service personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Shtilman 2016 London/Tel Aviv; PayU GPO Aug 2023");
    println!("    collect       900+ local payment methods across 100+ markets");
    println!("    disburse      Global payouts to bank, card, cash, wallet");
    println!("    wallet        Branded e-wallet under client's brand");
    println!("    cardissuing   Issue physical + virtual cards in 40+ countries");
    println!("    fx            Multi-currency settlement + treasury");
    println!("    pricing       Per-transaction + per-method, custom enterprise");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("rapyd-cli 0.1.0 (global-payment-aggregator personality build)"); }

fn run_about() {
    println!("Rapyd Financial Network.");
    println!("  Founded:    2016, London (HQ) + Tel Aviv (engineering).");
    println!("  Founders:   Arik Shtilman (CEO, ex-CTC/Telmap), Omer Priel,");
    println!("              Joel Yarbrough.");
    println!("  Pitch:      'The world's largest local payments network.'");
    println!("  Funding:    $300M Series E Aug 2021 at ~$15B; ~$1B raised total.");
    println!("  M&A:        Valitor (Icelandic acquirer) Jul 2022 $100M;");
    println!("              PayU GPO (Naspers/Prosus global payments) Aug 2023 $610M.");
    println!("  Coverage:   900+ payment methods, 100+ countries.");
}

fn run_collect() {
    println!("Rapyd Collect — accept payments globally.");
    println!("  Cards: Visa, Mastercard, Amex, Discover, JCB, UnionPay, local cards.");
    println!("  Bank transfer: PIX (BR), SEPA (EU), FPS (UK), iDEAL (NL), BLIK (PL).");
    println!("  Wallets: GrabPay, AliPay, WeChat Pay, M-Pesa, OVO, GCash, MercadoPago.");
    println!("  Cash: OXXO (MX), Boleto (BR), 7-Eleven cash vouchers (JP).");
    println!("  Single API across all of the above; one consolidated reconciliation.");
}

fn run_disburse() {
    println!("Rapyd Disburse — global payouts.");
    println!("  Pay out to bank account in 195+ countries.");
    println!("  Push-to-card payouts where supported.");
    println!("  Cash pickup at OXXO + Walmart + Western Union endpoints.");
    println!("  E-wallet payouts to GrabPay, M-Pesa, etc.");
    println!("  Use cases: marketplace seller payouts, gig worker payments,");
    println!("  insurance claim disbursements, content creator royalties.");
}

fn run_wallet() {
    println!("Rapyd Wallet — branded e-wallet under client's brand.");
    println!("  Embed a wallet into your app; user balance held by Rapyd.");
    println!("  Top up via card / bank / cash / wallet, spend back through Rapyd.");
    println!("  KYC + compliance handled by Rapyd in regulated jurisdictions.");
    println!("  Use cases: super-apps, gig platforms, creator economies needing");
    println!("  internal balance flows without becoming a regulated EMI themselves.");
}

fn run_cardissuing() {
    println!("Rapyd Card Issuing.");
    println!("  Issue branded physical or virtual Visa + Mastercard.");
    println!("  Programs in 40+ countries.");
    println!("  Real-time spend controls + per-transaction authorisation hooks.");
    println!("  Use cases: gig-economy expense cards, marketplace payout cards,");
    println!("  fintech-on-top-of-Rapyd consumer products.");
}

fn run_fx() {
    println!("FX + treasury.");
    println!("  Settlement in 50+ currencies; choose payout currency per market.");
    println!("  Hold balances in multi-currency Rapyd treasury accounts.");
    println!("  Auto-FX or manual FX at competitive interbank+ rates.");
    println!("  Reduces correspondent banking + nostro-account overhead for clients.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Card processing: per-transaction percentage + fixed fee per region.");
    println!("  Alternative payment methods: per-method rate card.");
    println!("  Payouts: per-corridor fixed + percentage.");
    println!("  Card issuing: monthly + per-card + interchange share.");
    println!("  Enterprise: bespoke contracts, volume-tiered, dedicated success.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Uber (regional payments), Crypto.com (fiat on-ramps), Booking.com,");
    println!("  Wayfair, FedEx, IKEA, Maersk, Wix, Cuentas, Ebanx-style competitors.");
    println!("  Heavy use among global marketplaces needing one integration for");
    println!("  many local payment methods in emerging markets.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "rapyd-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "collect" => run_collect(),
        "disburse" => run_disburse(),
        "wallet" => run_wallet(),
        "cardissuing" => run_cardissuing(),
        "fx" => run_fx(),
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
        run_collect();
        run_disburse();
        run_wallet();
        run_cardissuing();
        run_fx();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("rapyd-cli");
        print_version();
    }
}
