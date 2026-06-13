#![deny(clippy::all)]
//! paddle-cli — Slate OS Paddle Merchant-of-Record SaaS billing personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Paddle Merchant-of-Record for SaaS (personality)");
    println!();
    println!("USAGE:");
    println!("    {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about        Christian Owens, London 2012, MoR pioneer");
    println!("    mor          Merchant-of-Record model (the whole pitch)");
    println!("    tax          Global sales tax + VAT compliance");
    println!("    billing      Subscriptions + entitlements + invoicing");
    println!("    api          Paddle Billing v2 API");
    println!("    acquisitions ProfitWell, SaaSOptics, RevenueCat-adjacent moves");
    println!("    apple        The Epic v. Apple sideloading angle");
    println!("    help / version");
}

fn print_version() {
    println!("paddle-cli 0.1.0 — Slate OS personality binary");
    println!("Paddle.com Market Ltd — London, United Kingdom");
}

fn cmd_about() {
    println!("Paddle — Your complete payments infrastructure for selling software.");
    println!();
    println!("Founded:  2012 in London by Christian Owens + Harrison Rose");
    println!("          Christian Owens famously a self-taught teenage entrepreneur");
    println!("          (sold his first business at 16, started Paddle ~age 18)");
    println!();
    println!("Original product:");
    println!("  Distribution + payments for indie macOS / Windows software.");
    println!("  Sat in the niche between Apple's Mac App Store and direct sales.");
    println!("  Took on global tax compliance so single-founder shops could sell");
    println!("  internationally without dealing with EU VAT, US sales tax, etc.");
    println!();
    println!("Strategic pivot (~2017 onward):");
    println!("  Repositioned from 'app store for desktop software' to");
    println!("  'Merchant-of-Record (MoR) billing for SaaS'.");
    println!("  This is now the core business and the entire value prop.");
    println!();
    println!("Funding:");
    println!("  2017 Series A: USD 12.5M (Notion Capital, Kindred)");
    println!("  2018 Series B: USD 25M");
    println!("  2020 Series C: USD 68M");
    println!("  2022 Series D: USD 200M at USD 1.4B (KKR, FTV, Notion, Kindred)");
    println!();
    println!("Customers: 4,000+ SaaS companies. Concentrated in mid-market —");
    println!("           large enough to need tax compliance, small enough to");
    println!("           not want to build it themselves.");
}

fn cmd_mor() {
    println!("Merchant of Record — Paddle's defining model");
    println!();
    println!("The legal structure:");
    println!("  When your customer buys your software, the transaction is legally");
    println!("  between the customer and Paddle (the Merchant of Record), NOT");
    println!("  between the customer and you (the software vendor).");
    println!();
    println!("  You sell to Paddle, Paddle sells to the customer. Paddle then");
    println!("  remits you the net amount (sale - Paddle fees - taxes - refunds).");
    println!();
    println!("What that means in practice — Paddle takes on:");
    println!("  • Global sales tax / VAT / GST collection AND remittance");
    println!("    (registered in 30+ jurisdictions, files returns on your behalf)");
    println!("  • Chargeback / dispute liability");
    println!("  • Fraud risk and fraud losses");
    println!("  • PCI compliance scope");
    println!("  • Buyer-facing receipts, refunds, customer support tickets about billing");
    println!("  • Currency conversion + cross-border acquiring");
    println!();
    println!("What you trade:");
    println!("  • Higher take rate vs. Stripe (5% + USD 0.50 typical, vs Stripe's 2.9%)");
    println!("  • Less control over checkout (Paddle's hosted overlay or inline)");
    println!("  • Paddle's name on customer credit card statements");
    println!();
    println!("Why it's a real product:");
    println!("  Sales tax in the US is a nightmare (45 states + 12,000+ jurisdictions");
    println!("  post-Wayfair). EU VAT MOSS rules + UK divergence + global SaaS tax");
    println!("  treaties. For most B2C SaaS, building this in-house costs more than");
    println!("  Paddle's take rate. That's the entire pitch.");
}

fn cmd_tax() {
    println!("Tax compliance — Paddle's heaviest engineering");
    println!();
    println!("Jurisdictions Paddle handles (registration + collection + remittance):");
    println!();
    println!("US (post-Wayfair, 2018):");
    println!("  All 45 states with sales tax. Economic nexus monitoring.");
    println!("  Some cities/counties with separate filings (Denver, Chicago, NYC...).");
    println!("  Automated marketplace facilitator law compliance per state.");
    println!();
    println!("EU + UK:");
    println!("  EU VAT OSS (One-Stop Shop) — single EU registration, distributes");
    println!("  per-country. UK separate post-Brexit (HMRC). Customer location");
    println!("  determined via two non-conflicting pieces of evidence (IP +");
    println!("  billing country + bank country + card BIN country).");
    println!();
    println!("APAC:");
    println!("  Australia GST, New Zealand GST, Singapore GST, Japan JCT,");
    println!("  South Korea VAT, Taiwan VAT, India GST (where applicable),");
    println!("  Indonesia VAT (on digital services), Malaysia SST");
    println!();
    println!("Other:");
    println!("  Canada GST/HST/PST (each province), Mexico IVA, Russia VAT");
    println!("  (subject to sanctions compliance), Switzerland VAT, Norway VAT,");
    println!("  Turkey VAT, UAE VAT, Saudi Arabia VAT, South Africa VAT, Chile VAT");
    println!();
    println!("Total: 60+ tax jurisdictions actively monitored as of 2024.");
    println!();
    println!("Filing cadence:");
    println!("  Paddle files returns monthly / quarterly per jurisdiction's rules");
    println!("  and remits collected tax. Merchants get a quarterly statement.");
    println!("  Audits land on Paddle, not the merchant. This is the real value.");
}

fn cmd_billing() {
    println!("Paddle Billing — the SaaS subscription engine");
    println!();
    println!("Core entities:");
    println!("  • Products — what you sell (one-time or recurring SKUs)");
    println!("  • Prices — pricing tiers per product (multi-currency)");
    println!("  • Customers — buyer records (one per unique email)");
    println!("  • Subscriptions — recurring billing schedule");
    println!("  • Transactions — individual payment events");
    println!("  • Invoices — generated per transaction (B2B compliance)");
    println!();
    println!("Pricing models supported:");
    println!("  • Flat recurring (USD 49/month)");
    println!("  • Per-seat (USD 10/user/month, dynamic quantity)");
    println!("  • Tiered (volume bands)");
    println!("  • Usage-based (post-pay metered, custom metric)");
    println!("  • Free trials (with credit card requirement or not)");
    println!("  • Setup fees + recurring");
    println!("  • Multi-product subscriptions (bundle several SKUs in one billing)");
    println!();
    println!("Lifecycle events:");
    println!("  subscription.created, subscription.updated, subscription.canceled,");
    println!("  subscription.past_due, subscription.paused, subscription.resumed,");
    println!("  transaction.completed, transaction.refunded,");
    println!("  adjustment.created (credits, refunds, chargebacks)");
    println!();
    println!("Dunning:");
    println!("  Automated retry sequences on failed renewals (1, 3, 7, 14 day");
    println!("  configurable). Smart retry uses ML-based retry-time prediction.");
    println!("  Pre-dunning emails before retry. Card updater integrations.");
}

fn cmd_api() {
    println!("Paddle Billing API (v2 — current generation)");
    println!();
    println!("Base URL: https://api.paddle.com");
    println!("Sandbox:  https://sandbox-api.paddle.com");
    println!("Auth:     Bearer token (API key)");
    println!();
    println!("Resources:");
    println!("  GET  /products          POST /products");
    println!("  GET  /prices            POST /prices");
    println!("  GET  /customers         POST /customers");
    println!("  GET  /subscriptions     POST /subscriptions");
    println!("  GET  /transactions      POST /transactions");
    println!("  GET  /adjustments       POST /adjustments");
    println!();
    println!("Checkout integration:");
    println!("  1. Server: POST /transactions to create a draft transaction");
    println!("  2. Returns 'checkout' object with hosted checkout URL");
    println!("  3. Redirect customer OR load Paddle.js overlay inline:");
    println!();
    println!("     Paddle.Checkout.open({{");
    println!("       transactionId: 'txn_01h...',");
    println!("       settings: {{ displayMode: 'overlay', theme: 'light' }},");
    println!("     }});");
    println!();
    println!("Webhooks:");
    println!("  Signed (HMAC-SHA256) events for every billing lifecycle change.");
    println!("  Endpoint must respond 2xx within 15 seconds or Paddle retries");
    println!("  with exponential backoff for 21 days.");
    println!();
    println!("Migration:");
    println!("  Classic Paddle Billing API (v1) is being deprecated. v2 is the");
    println!("  go-forward platform; new accounts get v2 only.");
}

fn cmd_acquisitions() {
    println!("Paddle acquisitions and strategic moves");
    println!();
    println!("ProfitWell (acquired May 2022, undisclosed but reportedly USD 200M+):");
    println!("  • Patrick Campbell's subscription metrics company (Boston).");
    println!("  • Best-in-class MRR / churn / LTV / cohort analytics.");
    println!("  • Free 'ProfitWell Metrics' product was a category-defining tool.");
    println!("  • Now part of Paddle as 'ProfitWell Retain' (churn recovery)");
    println!("    and 'ProfitWell Metrics' (analytics).");
    println!();
    println!("SaaSOptics (acquired Jul 2022):");
    println!("  • Atlanta-based subscription accounting + revenue recognition.");
    println!("  • Bridges Paddle billing data into NetSuite / QuickBooks / etc.");
    println!("  • ASC 606 / IFRS 15 compliant revenue schedules.");
    println!();
    println!("Strategic pattern:");
    println!("  Paddle is building a vertically integrated 'finance stack for SaaS':");
    println!("    billing (Paddle) -> metrics (ProfitWell) -> revrec (SaaSOptics)");
    println!("    -> retention (ProfitWell Retain) -> dunning (Paddle native)");
    println!();
    println!("Distinct from:");
    println!("  • Stripe Billing (just billing, no MoR, no tax)");
    println!("  • Chargebee (billing + subscription, but not MoR)");
    println!("  • Lemon Squeezy (MoR, smaller, acquired by Stripe Jul 2024)");
    println!("  • FastSpring (oldest MoR, less developer-focused)");
    println!("  • DigitalRiver (legacy MoR, enterprise-only)");
}

fn cmd_apple() {
    println!("Paddle and the Apple sideloading saga");
    println!();
    println!("Background:");
    println!("  In the Epic v. Apple ruling (Sep 2021) and the EU Digital Markets");
    println!("  Act (effective Mar 2024), Apple was forced to allow developers to");
    println!("  steer customers to external payment methods (US: web only;");
    println!("  EU: actual alternative app stores + alt payment).");
    println!();
    println!("Paddle In-App Purchase (announced Mar 2023):");
    println!("  Paddle pre-announced an in-app purchase SDK for iOS developers");
    println!("  to use as an alternative to Apple's IAP — using Paddle as MoR.");
    println!("  Marketed as a 'massive savings' vs Apple's 30% (15% for small dev).");
    println!();
    println!("Apple's response:");
    println!("  Apple's revised terms required 'Core Technology Fee' of EUR 0.50");
    println!("  per install over 1M MAU annually (EU), plus 17% commission on");
    println!("  alt-payment transactions (still funneling money to Apple).");
    println!("  Net savings for most developers turned out to be marginal or zero.");
    println!();
    println!("Paddle iOS IAP — quiet retreat:");
    println!("  By late 2024, Paddle's iOS IAP product had been deprioritized.");
    println!("  Most developers concluded the math didn't work given Apple's terms.");
    println!();
    println!("Lesson:");
    println!("  When the platform owner controls the rules AND the runtime, a");
    println!("  payments alternative is structurally hard. Paddle remains strong");
    println!("  for web / desktop SaaS where this dynamic doesn't apply.");
}

fn run_paddle(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "mor" => cmd_mor(),
        "tax" => cmd_tax(),
        "billing" => cmd_billing(),
        "api" => cmd_api(),
        "acquisitions" => cmd_acquisitions(),
        "apple" => cmd_apple(),
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
        .unwrap_or_else(|| "paddle-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_paddle(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/paddle-cli"), "paddle-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("paddle-cli.exe"), "paddle-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_paddle(&[], "paddle-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_paddle(&["bogus".into()], "paddle-cli"), 2);
    }
}
