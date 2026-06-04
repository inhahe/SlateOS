#![deny(clippy::all)]
//! adyen-cli — OurOS Adyen unified commerce personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Adyen unified commerce platform (personality)");
    println!();
    println!("USAGE:");
    println!("    {prog} <subcommand> [args...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about          Company history (2006 Amsterdam, IPO 2018)");
    println!("    platform       Single platform: gateway + risk + acquiring");
    println!("    payments       Payment methods (3D Secure 2, local methods, APMs)");
    println!("    unified        Unified commerce: online + in-store + mobile");
    println!("    customers      Notable customers (Uber/Spotify/eBay/Etsy/McDonalds)");
    println!("    regions        Global footprint and licensing");
    println!("    pricing        Interchange++ transparent pricing model");
    println!("    revenue        RevenueProtect risk + RevenueAccelerate optimization");
    println!("    help           Show this help");
    println!("    version        Show version");
}

fn print_version() {
    println!("adyen-cli 0.1.0 — OurOS personality binary");
    println!("Adyen N.V. (Euronext: ADYEN), Amsterdam, Netherlands");
}

fn cmd_about() {
    println!("Adyen — One platform. Every payment.");
    println!();
    println!("Founded:  2006 in Amsterdam, Netherlands");
    println!("Founders: Pieter van der Does (CEO) + Arnout Schuijff (CTO)");
    println!("          (Both previously sold Bibit to Royal Bank of Scotland in 2004;");
    println!("          Adyen is Surinamese for 'start over again')");
    println!("HQ:       Amsterdam (Rokin)");
    println!("IPO:      June 13, 2018 on Euronext Amsterdam (ADYEN.AS)");
    println!("          Priced at EUR 240 -> opened at EUR 400 -> closed EUR 455 day one");
    println!("          Market cap exceeded EUR 13B at close (+85% pop)");
    println!();
    println!("Philosophy: 'One platform, one contract, one settlement.'");
    println!("           Built a single full-stack system from scratch — no acquired");
    println!("           pieces stitched together. The competitive moat is the");
    println!("           uniform global codebase, not the feature checklist.");
    println!();
    println!("Culture:   Slow, deliberate hiring. Famously austere office.");
    println!("           Long-term shareholder letters in the Berkshire tradition.");
}

fn cmd_platform() {
    println!("Adyen platform — full-stack vertical integration");
    println!();
    println!("  Layer 1: Gateway");
    println!("    • API + hosted payment pages + drop-in components");
    println!("    • Tokenization vault (PCI DSS Level 1)");
    println!();
    println!("  Layer 2: Risk management");
    println!("    • RevenueProtect — adaptive fraud scoring per merchant");
    println!("    • 3D Secure 2 with frictionless flow optimization");
    println!("    • Custom risk rules + ML model overlays");
    println!();
    println!("  Layer 3: Processing (the unusual part)");
    println!("    • Adyen IS the processor — not a reseller or aggregator");
    println!("    • Direct connections to card schemes (Visa/MC/Amex/CUP/JCB)");
    println!("    • Owns the message format conversions and clearing logic");
    println!();
    println!("  Layer 4: Acquiring");
    println!("    • Adyen N.V. holds acquiring licenses in EU/UK/US/SG/HK/AU/BR/MX/+");
    println!("    • Settlement directly to merchant bank account");
    println!("    • No intermediate acquirer markup");
    println!();
    println!("This vertical stack is the entire Adyen pitch. Every other payments");
    println!("company is buying or wrapping at least one of these layers.");
}

fn cmd_payments() {
    println!("Payment methods Adyen supports natively");
    println!();
    println!("Card schemes:");
    println!("  Visa, Mastercard, American Express, Discover, Diners,");
    println!("  JCB, UnionPay, Maestro, Cartes Bancaires, Bancontact, Dankort");
    println!();
    println!("European local methods:");
    println!("  • iDEAL (Netherlands — direct bank, ~70% of NL ecommerce)");
    println!("  • SEPA Direct Debit, SEPA Credit Transfer");
    println!("  • Sofort / Klarna Pay Now (Germany)");
    println!("  • giropay (Germany, sunset 2024)");
    println!("  • EPS (Austria), Trustly (Sweden/Nordics)");
    println!("  • MB Way (Portugal), Multibanco (Portugal)");
    println!("  • BLIK (Poland), PayU (CEE)");
    println!();
    println!("Wallets:");
    println!("  Apple Pay, Google Pay, Samsung Pay, PayPal, Alipay, WeChat Pay,");
    println!("  Amazon Pay, Cash App Pay, Venmo, GrabPay, GCash, PayMaya");
    println!();
    println!("BNPL:");
    println!("  Klarna, Afterpay/Clearpay, Affirm, Zip, Atome, Kredivo");
    println!();
    println!("Bank transfer / pay-by-bank:");
    println!("  Open Banking (UK/EU), Pix (Brazil), UPI (India), PayNow (Singapore)");
    println!();
    println!("Cash voucher:");
    println!("  OXXO (Mexico), Boleto (Brazil), Konbini (Japan), 7-Eleven (PH)");
    println!();
    println!("Total: 200+ payment methods through ONE integration.");
}

fn cmd_unified() {
    println!("Unified commerce — Adyen's signature offering");
    println!();
    println!("The thesis: a customer who buys online, returns in-store, then");
    println!("redeems a loyalty point on mobile is ONE customer with ONE token,");
    println!("not three different transactions in three different systems.");
    println!();
    println!("Channels unified:");
    println!("  • Ecommerce (web + mobile checkout)");
    println!("  • In-app (native SDKs for iOS/Android)");
    println!("  • In-store POS (Adyen-branded Verifone/PAX terminals)");
    println!("  • MOTO (mail order / telephone order, virtual terminal)");
    println!("  • Subscription / recurring (network tokens + auto-update)");
    println!("  • Pay by link (one-off SMS/email checkout URLs)");
    println!();
    println!("Cross-channel features (the actual moat):");
    println!("  • Shopper Recognition — same token across all channels");
    println!("  • Cross-channel returns — return online order to physical store");
    println!("  • Endless aisle — store associate completes order on Adyen tablet");
    println!("  • Click & collect with deferred capture");
    println!("  • Tap-to-pay on iPhone (Adyen was an early partner)");
    println!();
    println!("This is why Uber, McDonald's, and IKEA picked Adyen — their");
    println!("competitors couldn't unify the data across channels.");
}

fn cmd_customers() {
    println!("Notable Adyen customers (publicly disclosed)");
    println!();
    println!("Tech / ride-hailing / marketplace:");
    println!("  Uber (global), eBay (replacing PayPal as default 2018-2023),");
    println!("  Etsy, Spotify (subscription billing), Booking.com (some flows),");
    println!("  Vinted, GrabPay backend, Cabify, Bolt");
    println!();
    println!("Retail (omnichannel):");
    println!("  McDonald's (global rollout), IKEA, Mango, Tory Burch,");
    println!("  L'Oreal, Burberry, Vans, Crocs, Dr. Martens, Lululemon,");
    println!("  H&M (partial), Tommy Hilfiger / Calvin Klein");
    println!();
    println!("Streaming / digital:");
    println!("  Netflix (some markets), Microsoft (Xbox/M365 in some regions)");
    println!();
    println!("Food delivery / quick commerce:");
    println!("  Just Eat Takeaway, iFood (Brazil), Wolt (pre-DoorDash)");
    println!();
    println!("Travel:");
    println!("  KLM, easyJet, Hertz, Hostelworld, GetYourGuide, Klook");
    println!();
    println!("Pattern: large enterprises with multi-channel + multi-region needs.");
    println!("Adyen rarely competes for SMB — Stripe owns that segment.");
}

fn cmd_regions() {
    println!("Adyen global licensing and processing footprint");
    println!();
    println!("Acquiring licenses (Adyen N.V. or local subsidiary):");
    println!("  • EU / EEA — DNB (Dutch central bank) license, passported");
    println!("  • United Kingdom — FCA authorized");
    println!("  • United States — Adyen LLC, state money transmitter licenses");
    println!("  • Singapore — MAS Major Payment Institution");
    println!("  • Hong Kong — HKMA SVF license");
    println!("  • Australia — ACL, AUSTRAC registered");
    println!("  • Brazil — Adyen Brasil, Bacen authorized");
    println!("  • Mexico — IFPE (Institucion de Fondos de Pago Electronico)");
    println!("  • Canada, Japan, Malaysia, India, UAE, South Africa, NZ");
    println!();
    println!("Processing data centers:");
    println!("  • Primary: Amsterdam, Manchester, Chicago, San Francisco,");
    println!("    Sao Paulo, Singapore, Sydney");
    println!("  • Multi-region failover with sub-second cutover SLA");
    println!();
    println!("Currencies settled: 150+");
    println!("Settlement currencies offered to merchants: 30+");
    println!();
    println!("Notable: Adyen does NOT use third-party acquirers in its");
    println!("primary markets. The license stack IS the business.");
}

fn cmd_pricing() {
    println!("Adyen pricing — Interchange++ (transparent)");
    println!();
    println!("The model:");
    println!("  Total cost = Interchange + Scheme fees + Adyen markup");
    println!();
    println!("  • Interchange: set by card networks, paid to issuing bank");
    println!("    (varies by card type, region, transaction size — public schedules)");
    println!("  • Scheme fees: Visa/Mastercard network fees (also public)");
    println!("  • Adyen markup: the only number Adyen sets");
    println!();
    println!("Typical Adyen markup:");
    println!("  • EUR 0.11 per transaction (processing fee, flat)");
    println!("  • Plus a percentage markup that varies by:");
    println!("      - merchant volume (negotiated, sliding scale)");
    println!("      - card type (premium credit cards cost more)");
    println!("      - region (cross-border is higher)");
    println!();
    println!("Why this matters:");
    println!("  Stripe / Square charge BLENDED rates (2.9% + 30 cents in US).");
    println!("  That includes interchange (~1.5-2%) PLUS a HUGE markup");
    println!("  (~0.5-0.9%) for processing simplicity.");
    println!();
    println!("  Adyen IC++ is unbeatable at >EUR 1M/month volume.");
    println!("  Below that, blended pricing usually wins.");
    println!("  This is by design: Adyen targets large merchants.");
}

fn cmd_revenue() {
    println!("Adyen revenue tools");
    println!();
    println!("RevenueProtect — risk management suite");
    println!("  • Adaptive ML fraud scoring (per-merchant model training)");
    println!("  • 3D Secure 2 with smart exemption routing");
    println!("    (skip auth when issuer is likely to approve frictionless)");
    println!("  • Custom risk rules (block country X, require 3DS over EUR Y)");
    println!("  • Velocity checks (same card, same shopper, same IP)");
    println!("  • Negative lists + trust lists");
    println!();
    println!("RevenueAccelerate — authorization optimization");
    println!("  • Network tokens (auto-update on card reissue)");
    println!("  • Account Updater (Visa AAU + Mastercard ABU)");
    println!("  • Retry logic (intelligent retry on soft declines)");
    println!("  • Real-time Account Updater (RT-AU on auth failure)");
    println!("  • Dual messaging (auth + capture separated for higher approval)");
    println!("  • Local acquiring routing (cross-border becomes domestic)");
    println!();
    println!("Industry-leading auth rates: Adyen consistently publishes");
    println!("approval rates 1-3 percentage points above competitors on");
    println!("identical card portfolios — driven entirely by the unified");
    println!("acquiring stack + ML routing.");
}

fn run_adyen(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "platform" => cmd_platform(),
        "payments" => cmd_payments(),
        "unified" => cmd_unified(),
        "customers" => cmd_customers(),
        "regions" => cmd_regions(),
        "pricing" => cmd_pricing(),
        "revenue" => cmd_revenue(),
        "help" | "--help" | "-h" => print_help(prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'");
            eprintln!("Try '{prog} help' for the list of subcommands.");
            return 2;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "adyen-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_adyen(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/adyen-cli"), "adyen-cli");
        assert_eq!(basename("adyen-cli"), "adyen-cli");
        assert_eq!(basename(r"C:\bin\adyen-cli.exe"), "adyen-cli.exe");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("adyen-cli.exe"), "adyen-cli");
        assert_eq!(strip_ext("adyen-cli"), "adyen-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_adyen(&[], "adyen-cli");
        assert_eq!(run_adyen(&["help".into()], "adyen-cli"), 0);
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_adyen(&["bogus".into()], "adyen-cli"), 2);
    }
}
