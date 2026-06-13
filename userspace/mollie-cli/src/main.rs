#![deny(clippy::all)]
//! mollie-cli — Slate OS Mollie European payments personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Mollie European payments (personality)");
    println!();
    println!("USAGE:");
    println!("    {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about        Adriaan Mol, Amsterdam 2004, founder-led to USD 6.5B");
    println!("    methods      iDEAL, Bancontact, SEPA, Klarna and friends");
    println!("    api          Mollie REST API");
    println!("    onboarding   The famously fast self-serve activation");
    println!("    pricing      Per-transaction transparent pricing");
    println!("    customers    SMBs across the Benelux + Europe");
    println!("    ideal        iDEAL deep dive (NL's killer payment method)");
    println!("    help / version");
}

fn print_version() {
    println!("mollie-cli 0.1.0 — Slate OS personality binary");
    println!("Mollie B.V. — Amsterdam, Netherlands");
}

fn cmd_about() {
    println!("Mollie — Effortless payments for European businesses.");
    println!();
    println!("Founded:  2004 in Amsterdam by Adriaan Mol (then ~19 years old)");
    println!("          Originally an SMS gateway / micropayments shop");
    println!("          Pivoted to online payments around 2008-2010");
    println!();
    println!("Founder:  Adriaan Mol — quintessential Dutch tech founder.");
    println!("          Bootstrapped Mollie for over a decade. Also founded");
    println!("          MessageBird (later Bird) in 2011 — different company,");
    println!("          same founder, eventually had separate trajectories.");
    println!();
    println!("Major rounds:");
    println!("  Sep 2020:  USD 106M at USD 1B+ valuation (TCV — first unicorn round)");
    println!("  Jun 2021:  USD 800M Series C at USD 6.5B (Blackstone Growth, EQT,");
    println!("             General Atlantic). Largest European fintech round of 2021.");
    println!();
    println!("Positioning:");
    println!("  • SMB-first (not enterprise like Adyen)");
    println!("  • Europe-focused (NL/BE/DE/FR/UK/AT/CH primary)");
    println!("  • Simple onboarding (often <30 min from signup to first payment)");
    println!("  • Beautiful dashboard — long famous for design polish");
    println!();
    println!("Mollie is the answer to 'what if Stripe was Dutch and only sold");
    println!("in Europe?' — and that's a real, sustainable USD 6.5B business.");
}

fn cmd_methods() {
    println!("Payment methods Mollie supports");
    println!();
    println!("Cards: Visa, Mastercard, American Express, Maestro, V Pay,");
    println!("       Cartes Bancaires");
    println!();
    println!("European local methods (Mollie's stronghold):");
    println!("  • iDEAL (Netherlands) — single most-used method on the platform");
    println!("  • Bancontact (Belgium) — ~80% of BE online payments");
    println!("  • SEPA Direct Debit (recurring, mandate-based)");
    println!("  • SEPA Bank Transfer (manual reference-matching)");
    println!("  • Sofort (Germany, sunset path)");
    println!("  • EPS (Austria)");
    println!("  • Przelewy24 (Poland)");
    println!("  • giropay (Germany, retired by Deutsche Kreditwirtschaft Jun 2024)");
    println!();
    println!("Wallets:");
    println!("  Apple Pay, Google Pay, PayPal, Trustly, MobilePay (Nordics)");
    println!();
    println!("BNPL:");
    println!("  Klarna (Pay later / Pay in 3 / Slice it),");
    println!("  Riverty (formerly AfterPay BV — NL/BE/DE)");
    println!();
    println!("Gift / voucher:");
    println!("  Cadeaubon (NL gift cards), VVV Cadeaukaart, Podium Cadeaukaart,");
    println!("  Webshop Giftcard, Boekenbon");
    println!();
    println!("Crypto: BitcoinPay (custodial), partially deprecated");
    println!();
    println!("Bank-grade:");
    println!("  Mollie is a licensed PSP under the Dutch DNB (De Nederlandsche Bank)");
}

fn cmd_api() {
    println!("Mollie REST API");
    println!();
    println!("Base URL: https://api.mollie.com/v2");
    println!("Auth:     Bearer token (test_XXX or live_XXX prefixed key)");
    println!();
    println!("Core endpoints:");
    println!("  POST /payments               — create a payment");
    println!("  GET  /payments/{{id}}          — retrieve a payment");
    println!("  POST /payments/{{id}}/refunds  — issue a refund");
    println!("  POST /customers              — create a customer (for recurring)");
    println!("  POST /subscriptions          — create a subscription (mandate-based)");
    println!("  POST /orders                 — create an order (Klarna-style line items)");
    println!("  POST /shipments              — capture a partial shipment");
    println!();
    println!("Recurring model:");
    println!("  1. Customer makes initial payment with sequenceType='first'");
    println!("  2. Mollie creates a 'mandate' from the payment method");
    println!("  3. Subsequent charges reference the customer + mandate");
    println!();
    println!("Webhooks:");
    println!("  Single 'webhookUrl' per payment — Mollie POSTs payment ID");
    println!("  Your server then GETs /payments/{{id}} for full state");
    println!("  (Webhook is just a wake-up signal — by design)");
    println!();
    println!("Idempotency: Idempotency-Key header on POST");
    println!("Languages: officially-supported clients in PHP, Ruby, Python,");
    println!("           Node.js, .NET, Java, Go");
}

fn cmd_onboarding() {
    println!("Mollie onboarding — the activation experience");
    println!();
    println!("The famous Mollie sign-up flow:");
    println!();
    println!("  1. Email + password — instant test mode access");
    println!("     You can call the API and accept TEST payments within seconds.");
    println!();
    println!("  2. Submit business details:");
    println!("     • KvK / Chamber of Commerce number (NL/BE/DE etc.)");
    println!("     • Beneficial owner identification (UBO)");
    println!("     • Bank account for settlement (IBAN)");
    println!();
    println!("  3. Identity verification:");
    println!("     • Bank verification — small refundable payment from your IBAN");
    println!("     • For NL: iDIN (digital ID) for instant verification");
    println!     ("     • Document upload as fallback");
    println!();
    println!("  4. Activation review:");
    println!("     • Typically same-day for NL/BE businesses with clean KvK records");
    println!("     • 1-3 business days for cross-border or higher-risk MCCs");
    println!();
    println!("Average time from signup to live transactions:");
    println!("  • NL SMB with iDIN: ~30 minutes");
    println!("  • EU SMB with documents: ~1-2 days");
    println!("  • UK / non-EU: ~3-5 days");
    println!();
    println!("This is dramatically faster than legacy acquirers (Worldpay, Adyen");
    println!("enterprise, etc.) who often take 2-6 weeks. Speed of activation is");
    println!("Mollie's primary distribution advantage in the SMB segment.");
}

fn cmd_pricing() {
    println!("Mollie pricing — transparent per-transaction");
    println!();
    println!("No monthly fees. No setup fees. No PCI compliance fees.");
    println!("Per-transaction only.");
    println!();
    println!("Indicative rates (NL, EUR, EEA cards — check official rate card):");
    println!("  • iDEAL:           EUR 0.29 per transaction (flat)");
    println!("  • Bancontact:      EUR 0.39");
    println!("  • SEPA Direct Debit: EUR 0.25");
    println!("  • Credit card (EEA consumer): 1.8% + EUR 0.25");
    println!("  • Credit card (non-EEA / commercial): 2.8% + EUR 0.25");
    println!("  • American Express: 2.8% + EUR 0.25");
    println!("  • PayPal (passthrough): PayPal's standard rates");
    println!("  • Klarna: 2.99% + EUR 0.35 (varies by country)");
    println!();
    println!("Volume tiers available above ~EUR 100k/month with manual sales contact.");
    println!();
    println!("What's NOT charged:");
    println!("  • Refunds (free, except non-refundable interchange in some cases)");
    println!("  • Chargebacks — small fee EUR 25, no monthly minimum");
    println!("  • Currency conversion — yes, ~0.2% on settlement FX");
    println!();
    println!("Compare to Adyen IC++ — Mollie's blended pricing is simpler but");
    println!("more expensive at high volume. Crossover is around EUR 500k/month.");
}

fn cmd_customers() {
    println!("Mollie customer base");
    println!();
    println!("Notable European SMB / mid-market customers (publicly disclosed):");
    println!("  • Mediq, Etam Group, Buienradar, Albert Heijn (some flows)");
    println!("  • Picnic (NL online grocer)");
    println!("  • Coolblue (NL ecommerce giant, partial)");
    println!("  • Greenpeace, UNICEF NL (donations + recurring)");
    println!("  • Many Shopify Plus merchants in NL/BE/DE");
    println!("  • Most NL WooCommerce + Magento sites");
    println!();
    println!("Total active merchants: ~200,000+ (as of 2023 disclosures)");
    println!();
    println!("Segment distribution:");
    println!("  • Long tail of SMBs — most merchants under EUR 1M/year volume");
    println!("  • Heavy Shopify, WooCommerce, Magento, PrestaShop, Lightspeed");
    println!("  • Strong subscription business (Mollie subscriptions API");
    println!("    powers many EU SaaS, charities, gym memberships)");
    println!();
    println!("Geographic distribution:");
    println!("  ~50% Netherlands, ~25% Belgium, ~15% Germany, rest EU + UK");
    println!();
    println!("Compare: Adyen serves ~7,000 enterprises. Mollie serves 200k SMBs.");
    println!("Same continent, different segment — they barely compete in practice.");
}

fn cmd_ideal() {
    println!("iDEAL — the Dutch payment method, deep dive");
    println!();
    println!("What it is:");
    println!("  iDEAL is a real-time bank transfer scheme operated by Currence iDEAL");
    println!("  (owned by the major Dutch banks). The customer selects their bank");
    println!("  at checkout, gets redirected to the bank's app or web banking, and");
    println!("  authorizes the payment with their normal 2FA/biometric login.");
    println!();
    println!("Why it dominates NL:");
    println!("  • ~70% of all NL ecommerce payments");
    println!("  • Customer trusts their own bank (not a card network)");
    println!("  • No card needed (huge for the demographic without credit cards)");
    println!("  • Real-time settlement — merchant sees confirmation in <10 seconds");
    println!("  • Almost zero chargeback risk (the bank authorized the payment)");
    println!();
    println!("Participating banks:");
    println!("  ABN AMRO, ING, Rabobank, SNS, ASN, RegioBank, Triodos, Knab,");
    println!("  Revolut, bunq, ING Belgium (NL flow), Van Lanschot");
    println!();
    println!("iDEAL 2.0 (Nexus / European Payments Initiative):");
    println!("  • Launched 2023-2024");
    println!("  • Customer identity via iDIN (built-in)");
    println!("  • One-click recurring (no per-payment redirect)");
    println!("  • Cross-border ambitions — designed to scale beyond NL");
    println!("  • EPI 'Wero' wallet rebrand for the broader European launch");
    println!();
    println!("Mollie was an early integrator of iDEAL 2.0.");
    println!("If you sell to NL consumers and don't accept iDEAL, you will lose");
    println!("roughly two-thirds of potential transactions. It is THAT dominant.");
}

fn run_mollie(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "methods" => cmd_methods(),
        "api" => cmd_api(),
        "onboarding" => cmd_onboarding(),
        "pricing" => cmd_pricing(),
        "customers" => cmd_customers(),
        "ideal" => cmd_ideal(),
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
        .unwrap_or_else(|| "mollie-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_mollie(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mollie-cli"), "mollie-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mollie-cli.exe"), "mollie-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_mollie(&[], "mollie-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_mollie(&["bogus".into()], "mollie-cli"), 2);
    }
}
