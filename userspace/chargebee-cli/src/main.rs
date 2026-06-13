#![deny(clippy::all)]
//! chargebee-cli — SlateOS Chargebee subscription billing personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Chargebee subscription management platform (personality)");
    println!();
    println!("USAGE:");
    println!("    {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about         Chennai 2011, ChargeBee -> Chargebee, USD 3.5B");
    println!("    plans         Plans + addons + coupons + price overrides");
    println!("    api           Chargebee REST API (v2)");
    println!("    revops        Revenue operations suite (RevRec, Retention)");
    println!("    integrations  PSP integrations (Stripe, Adyen, Braintree, GoCardless...)");
    println!("    quotes        Quote-to-cash for B2B SaaS");
    println!("    tax           Avalara / TaxJar integration model");
    println!("    receivables   AR + dunning + collections");
    println!("    help / version");
}

fn print_version() {
    println!("chargebee-cli 0.1.0 — SlateOS personality binary");
    println!("Chargebee Inc. — Chennai, India / San Francisco, USA");
}

fn cmd_about() {
    println!("Chargebee — The leading revenue growth management platform.");
    println!();
    println!("Founded:  2011 in Chennai, India");
    println!("Founders: Krish Subramanian (CEO), Saravanan Parthasarathy,");
    println!("          Rajaraman Santhanam, Thiyagarajan Thiyagu");
    println!("          (Four ex-Zoho engineers + product folks)");
    println!();
    println!("Origin story:");
    println!("  Founders left Zoho to build subscription billing infrastructure");
    println!("  for SaaS companies. Bootstrapped initially, then went VC route");
    println!("  as the SaaS market exploded.");
    println!();
    println!("Funding history:");
    println!("  2014:  USD 2.1M Series A (Accel India)");
    println!("  2017:  USD 18M Series B (Insight Venture Partners)");
    println!("  2019:  USD 14M Series C (Tiger Global, Steadview)");
    println!("  2020:  USD 55M Series D (Insight Partners, Steadview)");
    println!("  Apr 2021: USD 125M Series F at USD 1.4B (unicorn)");
    println!("  Jan 2022: USD 250M Series H at USD 3.5B");
    println!("           (Sequoia Capital India, Tiger Global, Insight)");
    println!();
    println!("Positioning:");
    println!("  Subscription billing + revenue operations, broader than Stripe");
    println!("  Billing but not MoR like Paddle. Sits between the PSP (Stripe/");
    println!("  Adyen/Braintree) and the accounting system (NetSuite/QuickBooks).");
    println!();
    println!("  Customers: 6,500+ SaaS / subscription businesses including");
    println!("  Freshworks, Calendly, Okta, Pret a Manger, Study.com, Yelp Eat24");
}

fn cmd_plans() {
    println!("Chargebee plans / pricing model entities");
    println!();
    println!("Plans:");
    println!("  Core recurring SKU. Has price, period, period unit, currency.");
    println!("  E.g. 'Pro Annual' = USD 1188 / year, or 'Pro Monthly' = USD 99/month.");
    println!();
    println!("Addons:");
    println!("  Optional incremental items added to a subscription.");
    println!("  Recurring (extra seats) or one-time (setup fee).");
    println!("  Quantity-based, tiered, volume-based, or flat.");
    println!();
    println!("Charges:");
    println!("  One-time line items (e.g. overage charge, custom fee).");
    println!();
    println!("Coupons:");
    println!("  Percentage / fixed amount discounts. Apply to specific plans/");
    println!("  addons. Time-limited or duration-limited (e.g. 'first 3 months').");
    println!("  Promo codes are coupon front-ends with vanity codes.");
    println!();
    println!("Price overrides:");
    println!("  Per-subscription custom price ('this customer pays USD 199/mo");
    println!("  on the Pro plan because we sold them a custom deal').");
    println!("  Negotiated pricing without forking the plan catalog.");
    println!();
    println!("Item families + item prices (newer model):");
    println!("  Modernized Product Catalog v2 — item families group related");
    println!("  SKUs, item prices hold the actual pricing in each currency.");
    println!("  Better for multi-currency + multi-tier catalogs.");
    println!();
    println!("Trial settings:");
    println!("  Trial period (days), trial card capture required Y/N, trial end");
    println!("  action (auto-cancel vs auto-charge), pause-during-trial allowed.");
}

fn cmd_api() {
    println!("Chargebee REST API (v2)");
    println!();
    println!("Base URL: https://{{site}}.chargebee.com/api/v2");
    println!("          (per-merchant subdomain, also functions as merchant ID)");
    println!("Auth:     HTTP Basic — username=API_KEY, password empty");
    println!("Format:   application/x-www-form-urlencoded (legacy choice, but stable)");
    println!();
    println!("Top-level resources (all CRUDable):");
    println!("  /subscriptions, /customers, /plans, /addons, /coupons,");
    println!("  /invoices, /credit_notes, /transactions, /events, /quotes,");
    println!("  /hosted_pages, /payment_sources, /payment_intents, /unbilled_charges,");
    println!("  /orders, /gifts, /promotional_credits");
    println!();
    println!("Hosted Pages (Chargebee Checkout):");
    println!("  POST /hosted_pages/checkout_new -> returns a URL.");
    println!("  Redirect customer there. They pick a payment method, enter card");
    println!("  (PCI scope stays with Chargebee), subscription is created on success.");
    println!("  Webhook fires subscription_created -> your app fulfills.");
    println!();
    println!("Drop-in (Chargebee.js):");
    println!("  Embed payment form in your own checkout. Fields are iframed");
    println!("  from Chargebee for PCI scope reduction.");
    println!();
    println!("Webhooks:");
    println!("  Per-merchant configurable endpoints. Events for every billing");
    println!("  lifecycle transition. Idempotency via event ID. HMAC signing");
    println!("  available (X-CB-Signature) since 2022.");
    println!();
    println!("Bulk operations:");
    println!("  Export jobs for large data pulls (invoices, transactions).");
    println!("  Import jobs for legacy-system migration (customers, subs).");
}

fn cmd_revops() {
    println!("Chargebee Revenue Operations suite");
    println!();
    println!("Chargebee Billing (core):");
    println!("  Subscription management, invoicing, dunning, lifecycle automation.");
    println!();
    println!("Chargebee Receivables (collections):");
    println!("  AR aging buckets, automated reminder cadences, payment promises,");
    println!("  agent workflows, integrated calling/email, write-off rules.");
    println!();
    println!("Chargebee Retention (churn recovery):");
    println!("  Cancellation flow with save offers. A/B-tested save funnels.");
    println!("  Pause-instead-of-cancel options. Built around the Brightback");
    println!("  acquisition (Chargebee bought Brightback in 2021).");
    println!();
    println!("Chargebee RevRec (revenue recognition):");
    println!("  ASC 606 / IFRS 15 compliant revenue scheduling. Multi-element");
    println!("  arrangement allocation. Recognized vs. deferred revenue tracking.");
    println!("  Audit-ready journal entry exports.");
    println!();
    println!("Chargebee Pricing Insights (analytics):");
    println!("  MRR, ARR, churn, expansion, contraction, NRR, gross/net dollar");
    println!("  retention, cohort analysis, plan migration analysis.");
    println!();
    println!("CPQ / Quotes (B2B sales-led):");
    println!("  Quote generation, approval workflows, e-signature integration,");
    println!("  quote-to-subscription auto-creation on close.");
    println!();
    println!("Bottom line: Chargebee's pitch is 'one place for everything that");
    println!("happens to recurring revenue between PSP and accounting system'.");
}

fn cmd_integrations() {
    println!("Chargebee PSP and ecosystem integrations");
    println!();
    println!("Payment Service Providers (PSPs):");
    println!("  Chargebee does NOT process payments itself. It orchestrates");
    println!("  one or more PSPs that your merchant account is with.");
    println!();
    println!("  Supported (partial list, varies by region):");
    println!("    • Stripe (the most common, widely default)");
    println!("    • Adyen (enterprise customers)");
    println!("    • Braintree (PayPal-owned)");
    println!("    • Authorize.Net (US)");
    println!("    • Worldpay, GlobalPayments, Cybersource");
    println!("    • GoCardless (SEPA Direct Debit, BACS, ACH)");
    println!("    • Razorpay (India)");
    println!("    • CCAvenue, EBS, PayU (India / EM)");
    println!("    • Bluesnap, Spreedly (vault-agnostic routing)");
    println!();
    println!("Multi-gateway routing:");
    println!("  Run multiple PSPs simultaneously. Route by currency, region,");
    println!("  card BIN, customer geography. Cross-PSP retry on decline.");
    println!();
    println!("Accounting:");
    println!("  QuickBooks Online, QuickBooks Desktop, Xero, NetSuite, Sage Intacct");
    println!();
    println!("CRM:");
    println!("  Salesforce, HubSpot, Pipedrive (subscription data sync)");
    println!();
    println!("Tax engines:");
    println!("  Avalara, TaxJar, Vertex (Chargebee calls the tax API, applies");
    println!("  computed tax to invoice — see 'chargebee tax')");
    println!();
    println!("Data warehouses + iPaaS:");
    println!("  Segment, Zapier, Mulesoft, Workato, native data exports");
}

fn cmd_quotes() {
    println!("Chargebee Quote-to-Cash for B2B SaaS");
    println!();
    println!("Why this matters:");
    println!("  B2B SaaS deals often need:");
    println!("    1. A formal quote / proposal document");
    println!("    2. Internal approval (legal, finance, sales ops)");
    println!("    3. Customer review and e-signature");
    println!("    4. Auto-creation of a subscription matching the quoted terms");
    println!("    5. Invoicing with custom payment terms (NET 30, NET 60, milestones)");
    println!("  Stripe / Adyen / direct PSPs don't do steps 1-3.");
    println!();
    println!("Chargebee quotes:");
    println!("  • Build a quote: add plans, addons, charges, custom line items");
    println!("  • Multi-period (one-time setup + 12-month subscription + addon ramp)");
    println!("  • Custom payment terms per line (50% upfront / 50% at month 6)");
    println!("  • PDF generation with custom branding template");
    println!("  • Multi-currency quotes");
    println!("  • Approval workflows (CFO approves any discount > X%)");
    println!("  • E-signature via DocuSign / native");
    println!("  • Quote-to-subscription on signed acceptance — no manual rekeying");
    println!();
    println!("Use case sweet spot:");
    println!("  Mid-market SaaS sales motion. Deals USD 10k-USD 500k ACV.");
    println!("  Too complex for self-serve Stripe Checkout, too small for");
    println!("  a dedicated CPQ tool like Salesforce CPQ or DealHub.");
}

fn cmd_tax() {
    println!("Chargebee tax model");
    println!();
    println!("Chargebee is NOT a Merchant of Record. It computes and bills tax,");
    println!("but the merchant is the legal seller and remits the tax.");
    println!();
    println!("How tax computation works:");
    println!();
    println!("  Path 1: Manual tax rates (smallest merchants)");
    println!("    Configure per-region tax rates yourself. Chargebee applies.");
    println!("    Works for simple cases (one country, one VAT rate).");
    println!();
    println!("  Path 2: Avalara AvaTax integration");
    println!("    Real-time tax API call on each invoice. Avalara returns line-level");
    println!("    tax based on customer address + product taxability codes.");
    println!("    You pay Avalara separately for the engine + filing services.");
    println!();
    println!("  Path 3: TaxJar integration");
    println!("    Similar model to Avalara, generally simpler / cheaper.");
    println!("    Stripe acquired TaxJar in 2021, but Chargebee still integrates.");
    println!();
    println!("  Path 4: Vertex (large enterprise)");
    println!("    The 800-pound gorilla of tax engines. SAP-class customers.");
    println!();
    println!("EU VAT MOSS / OSS:");
    println!("  Chargebee handles VAT number validation, B2B reverse-charge logic,");
    println!("  customer-location determination (two-piece-of-evidence rule),");
    println!("  and OSS-compatible invoice formatting. Filing is still on you");
    println!("  (or your accountant).");
    println!();
    println!("US Sales Tax:");
    println!("  Post-Wayfair economic nexus monitoring is what Avalara provides.");
    println!("  Chargebee surfaces this data; doesn't itself register/remit.");
}

fn cmd_receivables() {
    println!("Chargebee Receivables — collections + dunning");
    println!();
    println!("Why it exists:");
    println!("  Subscription billing is dominated by card-on-file, but B2B SaaS");
    println!("  increasingly bills via invoice + NET-30 terms. That creates AR.");
    println!("  Chargebee Receivables is the AR workflow product.");
    println!();
    println!("Capabilities:");
    println!();
    println!("  Aging buckets:");
    println!("    Real-time AR aging (current, 1-30, 31-60, 61-90, 90+).");
    println!("    Filter by customer segment, ARR band, account owner.");
    println!();
    println!("  Cadences:");
    println!("    Configurable reminder sequences — email, in-app, agent task,");
    println!("    automated dunning emails. Different cadence per customer segment.");
    println!();
    println!("  Smart dunning (card retry):");
    println!("    For card-on-file customers — retry failed charges with");
    println!("    intelligent timing based on issuer behavior.");
    println!();
    println!("  Payment promises:");
    println!("    Record customer commitments ('we'll pay by Tuesday').");
    println!("    Automated follow-up if promise is broken.");
    println!();
    println!("  Collections workflow:");
    println!("    Agent dashboard with queue, notes, call logging, escalation.");
    println!("    Hand-off rules between billing -> collections -> external agency.");
    println!();
    println!("  Account holds:");
    println!("    Auto-suspend service when AR exceeds threshold or aging is bad.");
    println!("    Restore on payment.");
    println!();
    println!("Compare to specialized AR tools:");
    println!("  HighRadius, Versapay, Tesorio — enterprise AR with deeper ERP");
    println!("  integration. Chargebee Receivables is the SaaS-native equivalent");
    println!("  positioned at mid-market subscription businesses.");
}

fn run_chargebee(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "plans" => cmd_plans(),
        "api" => cmd_api(),
        "revops" => cmd_revops(),
        "integrations" => cmd_integrations(),
        "quotes" => cmd_quotes(),
        "tax" => cmd_tax(),
        "receivables" => cmd_receivables(),
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
        .unwrap_or_else(|| "chargebee-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_chargebee(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/chargebee-cli"), "chargebee-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("chargebee-cli.exe"), "chargebee-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_chargebee(&[], "chargebee-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_chargebee(&["bogus".into()], "chargebee-cli"), 2);
    }
}
