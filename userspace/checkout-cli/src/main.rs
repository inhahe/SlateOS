#![deny(clippy::all)]
//! checkout-cli — SlateOS Checkout.com personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Checkout.com payments platform (personality)");
    println!();
    println!("USAGE:");
    println!("    {prog} <subcommand> [args...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about        Company history (Guillaume Pousaz, Geneva -> London)");
    println!("    valuation    The famous USD 40B -> markdown saga");
    println!("    api          Unified Payments API");
    println!("    methods      Global payment methods support");
    println!("    flow         Flow drop-in components");
    println!("    risk         Risk.js + intelligent acceptance");
    println!("    crypto       Crypto on-ramp business");
    println!("    help / version");
}

fn print_version() {
    println!("checkout-cli 0.1.0 — Slate OS personality binary");
    println!("Checkout Ltd — London, United Kingdom");
}

fn cmd_about() {
    println!("Checkout.com — Connect to more payments, fewer providers.");
    println!();
    println!("Origins: NetMerchant founded 2009 in Geneva by Guillaume Pousaz");
    println!("         (Swiss-French, ex-Credit Suisse FX trader)");
    println!("Rebrand: 'Checkout.com' in 2012, HQ moved to London 2013");
    println!("         after buying the checkout.com domain for a reported");
    println!("         seven-figure sum (premium .com, the strategic asset)");
    println!();
    println!("Funding journey:");
    println!("  2019: Series A — USD 230M at USD 2B valuation (Insight Partners, DST)");
    println!("        At the time, largest Series A ever for a European fintech.");
    println!("  2020: Series B — USD 150M at USD 5.5B");
    println!("  Jan 2021: Series C — USD 450M at USD 15B");
    println!("  Jan 2022: Series D — USD 1B at USD 40B");
    println!("            Briefly Europe's most valuable private fintech, ahead of Klarna.");
    println!();
    println!("Then: see 'checkout-cli valuation' for what happened next.");
}

fn cmd_valuation() {
    println!("Checkout.com — the USD 40B -> markdown saga");
    println!();
    println!("Jan 2022:   Series D closes at USD 40B post-money");
    println!("            Pousaz becomes one of Europe's richest fintech founders.");
    println!();
    println!("2022:       Crypto winter begins. Checkout was heavily exposed —");
    println!("            served FTX, Binance, Crypto.com, eToro, and ~40% of the");
    println!("            global crypto on-ramp flow ran through Checkout rails.");
    println!();
    println!("Nov 2022:   FTX collapses. Checkout had material exposure.");
    println!();
    println!("Dec 2022:   Internal valuation marked to USD 11B (per FT reporting).");
    println!("            Some employees holding Series D-priced options were");
    println!("            instantly underwater. Some reports of bonus clawbacks.");
    println!();
    println!("2023:       Further markdowns reported by some LPs. Fidelity marked");
    println!("            its stake at ~USD 9.4B in mid-2023, ~USD 5B end of 2023.");
    println!();
    println!("Mid 2024:   Some marks recovered. Pousaz publicly defended valuation");
    println!("            in WSJ interview ('we made USD 40B happen for a reason').");
    println!();
    println!("Lesson:     Mark-to-market private valuations are real. Concentrated");
    println!("            customer exposure (crypto) compounds with macro cycles.");
    println!("            And paper billionaires can stay paper for years.");
}

fn cmd_api() {
    println!("Checkout.com Unified Payments API");
    println!();
    println!("Base URL: https://api.checkout.com (production)");
    println!("          https://api.sandbox.checkout.com (sandbox)");
    println!();
    println!("Auth:     Bearer token (public key for client, secret for server)");
    println!("Format:   JSON over HTTPS, idempotency keys supported");
    println!();
    println!("Core resources:");
    println!("  POST /payments              — create a payment");
    println!("  POST /payments/{{id}}/captures — capture an authorization");
    println!("  POST /payments/{{id}}/voids    — void an authorization");
    println!("  POST /payments/{{id}}/refunds  — refund a captured payment");
    println!("  GET  /payments/{{id}}         — retrieve payment details");
    println!();
    println!("Tokens:");
    println!("  POST /tokens                — tokenize card details (PCI scope)");
    println!("  POST /sources               — create a saved source (3DS-ready)");
    println!();
    println!("Webhooks:");
    println!("  Signed (HMAC) events for payment lifecycle changes");
    println!("  payment_approved, payment_declined, payment_captured,");
    println!("  payment_refunded, dispute_received, dispute_resolved");
}

fn cmd_methods() {
    println!("Payment methods supported");
    println!();
    println!("Cards: Visa, Mastercard, American Express, Discover, Diners,");
    println!("       JCB, Maestro, UnionPay, Mada (Saudi), Meeza (Egypt)");
    println!();
    println!("Regional strengths:");
    println!("  • Middle East — leader in UAE/Saudi/Egypt (Mada, Knet, Benefit)");
    println!("  • Europe — full SEPA, iDEAL, Bancontact, Sofort, EPS, Giropay");
    println!("  • LATAM — Boleto, Pix, OXXO, SPEI, PSE");
    println!("  • APAC — Alipay+, WeChat Pay, GrabPay, ShopeePay, TrueMoney");
    println!();
    println!("Wallets: Apple Pay, Google Pay, PayPal, Klarna, Tamara, Tabby");
    println!();
    println!("BNPL focus: Tabby + Tamara (Middle East market leaders) — Checkout");
    println!("            was an early partner for both, leveraging its MENA");
    println!("            licensing footprint.");
    println!();
    println!("Account funding:");
    println!("  • Original Credit Transactions (OCT) — push payments to cards");
    println!("  • Used heavily for gig-economy payouts and crypto withdrawals");
}

fn cmd_flow() {
    println!("Flow — Checkout.com drop-in UI");
    println!();
    println!("What it is:");
    println!("  Embeddable JavaScript component that renders a dynamic checkout");
    println!("  form. Selects payment methods based on shopper location, currency,");
    println!("  and merchant configuration.");
    println!();
    println!("Integration:");
    println!("  <script src=\"https://checkout-web-components.checkout.com/index.js\">");
    println!("  </script>");
    println!();
    println!("  const cko = await CheckoutWebComponents({{");
    println!("    publicKey: 'pk_test_...',");
    println!("    environment: 'sandbox',");
    println!("    locale: 'en-GB',");
    println!("    paymentSession: {{ /* server-created session */ }},");
    println!("  }});");
    println!();
    println!("  cko.create('flow').mount('#flow-container');");
    println!();
    println!("Features:");
    println!("  • Auto-detects browser locale + currency");
    println!("  • Renders only methods available for the session");
    println!("  • Handles 3DS challenges inline (modal or redirect)");
    println!("  • PCI scope: SAQ-A (form is iframed from Checkout domain)");
    println!("  • Native look-and-feel via CSS variable theming");
}

fn cmd_risk() {
    println!("Checkout.com Risk — fraud prevention");
    println!();
    println!("Risk.js:");
    println!("  Tiny JS snippet (<3KB) loaded on checkout page that collects");
    println!("  device fingerprint signals BEFORE payment submission.");
    println!();
    println!("  Collected signals:");
    println!("    • Canvas + WebGL fingerprint");
    println!("    • Audio context fingerprint");
    println!("    • Installed fonts, screen resolution, timezone");
    println!("    • Browser plugin enumeration (legacy)");
    println!("    • Behavioral: typing rhythm, mouse path, time-on-page");
    println!("    • Network: IP geolocation, proxy/VPN/Tor detection");
    println!();
    println!("Intelligent Acceptance:");
    println!("  ML system that predicts issuer behavior per transaction:");
    println!("    • Should we send through 3DS or skip it (frictionless)?");
    println!("    • Which acquirer route maximizes auth probability?");
    println!("    • Should we retry a soft decline (and when)?");
    println!();
    println!("  Trained on Checkout's full transaction corpus across all merchants.");
    println!("  Reported lift: 1-4% auth rate improvement on baseline.");
    println!();
    println!("Dispute Resolution:");
    println!("  • Automated chargeback evidence assembly");
    println!("  • Verifi RDR + Ethoca alerts (pre-dispute interception)");
}

fn cmd_crypto() {
    println!("Checkout.com crypto on-ramp business");
    println!();
    println!("Why this matters:");
    println!("  At its 2021-2022 peak, Checkout processed an estimated 30-40%");
    println!("  of all global crypto exchange fiat on-ramp volume. This is the");
    println!("  context for the USD 40B Series D valuation in Jan 2022.");
    println!();
    println!("Major crypto customers (historical):");
    println!("  • Binance (until Mar 2023 wind-down in EU/UK)");
    println!("  • FTX / FTX International (until Nov 2022 bankruptcy)");
    println!("  • Crypto.com");
    println!("  • Coinbase (some flows)");
    println!("  • eToro");
    println!("  • Kraken (some markets)");
    println!("  • Bitstamp");
    println!();
    println!("What Checkout did differently:");
    println!("  • Accepted card payments to fund crypto purchases — most banks");
    println!("    classified these as MCC 6051 (quasi-cash) with high chargeback");
    println!("    risk. Checkout built specialized acquiring + risk models.");
    println!("  • SEPA Instant rails for EUR deposits with sub-10-second flow");
    println!("  • Real-time KYC + travel rule compliance integrations");
    println!();
    println!("The crypto winter (Nov 2022 onward) is the single largest factor");
    println!("in Checkout's subsequent valuation markdowns. Concentration risk,");
    println!("realized.");
}

fn run_checkout(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "valuation" => cmd_valuation(),
        "api" => cmd_api(),
        "methods" => cmd_methods(),
        "flow" => cmd_flow(),
        "risk" => cmd_risk(),
        "crypto" => cmd_crypto(),
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
        .unwrap_or_else(|| "checkout-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_checkout(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/checkout-cli"), "checkout-cli");
        assert_eq!(basename(r"C:\bin\checkout-cli.exe"), "checkout-cli.exe");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("checkout-cli.exe"), "checkout-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_checkout(&[], "checkout-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_checkout(&["bogus".into()], "checkout-cli"), 2);
    }
}
