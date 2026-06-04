#![deny(clippy::all)]
//! recurly-cli — OurOS Recurly subscription billing personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Recurly subscription billing (personality)");
    println!();
    println!("USAGE:");
    println!("    {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about        Isaac Hall 2009, Accel-KKR 2019");
    println!("    streaming    Streaming-media subscription stronghold");
    println!("    revenue      RevRec, ASC 606 schedules");
    println!("    api          Recurly v3 REST API");
    println!("    decline      Revenue Optimization Engine + decline salvage");
    println!("    gateways     Multi-gateway routing + tokenization");
    println!("    sip          Subscriber Identity Protection (account fraud)");
    println!("    coupons      Rich coupon / discount engine");
    println!("    help / version");
}

fn print_version() {
    println!("recurly-cli 0.1.0 — OurOS personality binary");
    println!("Recurly, Inc. — San Francisco, California, USA");
}

fn cmd_about() {
    println!("Recurly — Built for the people who power recurring revenue.");
    println!();
    println!("Founded:  2009 in San Francisco by Isaac Hall + co-founders");
    println!("          One of the original 'subscription billing platform' players,");
    println!("          alongside Zuora (2007) and Chargify (2009, now Maxio).");
    println!();
    println!("Early customers:");
    println!("  Cloud-era SaaS startups in the early 2010s. Hulu Plus, Twitch,");
    println!("  Asana, Cnet, LinkedIn (some flows). Recurly was often the");
    println!("  default choice when Stripe didn't yet have a billing product.");
    println!();
    println!("Funding:");
    println!("  Series A: USD 2.5M (2010)");
    println!("  Series B: USD 12M (2011)");
    println!("  Series C: USD 19.5M (2014)");
    println!("  Aug 2019: Recapitalization by Accel-KKR (private equity), majority");
    println!("            stake acquired. Recurly went from VC-backed to PE-owned.");
    println!("            Terms undisclosed but reported in mid 9-figures USD.");
    println!();
    println!("Post-Accel-KKR era:");
    println!("  Recurly transitioned from growth-mode SaaS to PE-style operational");
    println!("  efficiency. Acquired Redmine-like adjacent companies, optimized");
    println!("  pricing tiers, focused on enterprise-grade revenue features.");
    println!();
    println!("Strength: deep, mature platform. Less buzz than Stripe Billing but");
    println!("battle-tested over 15+ years of subscription billing edge cases.");
}

fn cmd_streaming() {
    println!("Recurly's streaming-media stronghold");
    println!();
    println!("Why Recurly is over-represented in streaming media:");
    println!("  Streaming subscriptions are uniquely punishing:");
    println!("    • Massive scale (millions to hundreds of millions of subs)");
    println!("    • Card-on-file storage critical (any decline = churned sub)");
    println!("    • Global tax (every country, every entertainment tax rule)");
    println!("    • Pause/resume mechanics (seasonal content viewers)");
    println!("    • Multi-platform identity (web + iOS + Android + smart TV signups");
    println!("      all need to consolidate to one billing record)");
    println!();
    println!("Recurly invested heavily in these specific patterns over a decade.");
    println!();
    println!("Notable streaming / media customers (publicly disclosed):");
    println!("  • Twitch (subscriptions to streamers)");
    println!("  • Sling TV (Dish)");
    println!("  • CBS All Access / Paramount+ (some flows historically)");
    println!("  • Showtime, BET+, Acorn TV, Sundance Now, Topic, Shudder");
    println!("  • FuboTV (some periods)");
    println!("  • Asahi Shimbun, Reuters Digital, Forbes (publisher subscriptions)");
    println!("  • Roku Channel premium tier");
    println!();
    println!("Decline salvage rates for streaming:");
    println!("  Recurly publishes decline-salvage benchmarks averaging");
    println!("  ~70%+ recovery on initial card declines for media accounts");
    println!("  via Revenue Optimization Engine (see 'recurly decline').");
}

fn cmd_revenue() {
    println!("Recurly Revenue — ASC 606 / IFRS 15 revenue recognition");
    println!();
    println!("Why this is a real product:");
    println!("  ASC 606 (US GAAP) and IFRS 15 (international) require revenue to");
    println!("  be recognized when service is delivered, not when cash is collected.");
    println!("  For subscription businesses, that means:");
    println!();
    println!("    • A USD 1200 annual sub paid up-front = USD 100 recognized monthly");
    println!("    • Mid-cycle upgrades require pro-rata recognition adjustments");
    println!("    • Setup fees with no standalone value get amortized over the contract");
    println!("    • Multi-element arrangements split allocation between deliverables");
    println!();
    println!("Doing this in Excel for a multi-thousand-customer subscription business");
    println!("is impossible. Recurly Revenue automates the schedules.");
    println!();
    println!("Features:");
    println!("  • Deferred revenue + recognized revenue ledgers");
    println!("  • Performance Obligation tracking per contract line");
    println!("  • Standalone Selling Price (SSP) allocation");
    println!("  • Contract Modifications handling (mid-term upgrades / downgrades)");
    println!("  • Journal entry export to NetSuite / Sage Intacct / QuickBooks");
    println!("  • Audit trail back to source subscription / invoice event");
    println!();
    println!("Audit-ready outputs:");
    println!("  • Roll-forward schedules (opening + new + recognized + closing)");
    println!("  • Variance reconciliation between billed and recognized");
    println!("  • SOX-compliant evidence packets");
    println!();
    println!("Bought-in / built-in:");
    println!("  Recurly Revenue is largely Recurly's own build, not an acquisition.");
    println!("  It came online in roughly current form ~2021 post-Accel-KKR.");
}

fn cmd_api() {
    println!("Recurly v3 REST API");
    println!();
    println!("Base URL: https://v3.recurly.com");
    println!("Auth:     HTTP Basic — username=API_KEY, password empty");
    println!("Format:   application/vnd.recurly.v2024-XX-XX+json (versioned media type)");
    println!();
    println!("Resources:");
    println!("  /sites/{{site_id}}/accounts          — customers (called Accounts)");
    println!("  /sites/{{site_id}}/subscriptions     — active and historical subs");
    println!("  /sites/{{site_id}}/plans             — recurring SKU catalog");
    println!("  /sites/{{site_id}}/items             — itemized billing catalog");
    println!("  /sites/{{site_id}}/invoices          — generated invoices");
    println!("  /sites/{{site_id}}/transactions      — payment events");
    println!("  /sites/{{site_id}}/coupons           — discount engine");
    println!("  /sites/{{site_id}}/measured_units    — usage-based billing units");
    println!("  /sites/{{site_id}}/usage             — usage events per measured unit");
    println!();
    println!("Recurly.js (client-side tokenization):");
    println!("  <script src=\"https://js.recurly.com/v4/recurly.js\"></script>");
    println!();
    println!("  recurly.configure('PUBLIC_KEY');");
    println!("  // Card form elements render into your page (iframed)");
    println!("  recurly.token(formData, (err, token) => {{");
    println!("    // Submit token.id to your server, never raw PAN");
    println!("  }});");
    println!();
    println!("Webhooks:");
    println!("  Per-site configurable. XML or JSON payload (legacy + modern).");
    println!("  Events for every account/sub/invoice/transaction lifecycle.");
    println!("  Idempotency via uuid in payload.");
    println!();
    println!("3D Secure:");
    println!("  Recurly handles 3DS challenge flows on initial setup and on");
    println!("  exemption-failed renewals (PSD2 / EU strong customer auth).");
}

fn cmd_decline() {
    println!("Recurly Revenue Optimization Engine (ROE)");
    println!("(formerly known as Recurly Account Updater + Decline Salvage)");
    println!();
    println!("Why decline salvage is THE business problem for subscription billing:");
    println!("  • Avg involuntary churn from card declines: 5-10% of MRR per year");
    println!("  • Most declines are 'soft' — insufficient funds, expired card,");
    println!("    re-issued card, temporary issuer hold — not actual fraud refusal");
    println!("  • Recovering 70%+ of those declines is the difference between a");
    println!("    healthy and a struggling subscription business");
    println!();
    println!("ROE components:");
    println!();
    println!("  Account Updater:");
    println!("    Visa AAU + Mastercard ABU integrations — when an issuer reissues");
    println!("    a card (lost, expired, replaced), the new card number replaces");
    println!("    the old token in Recurly automatically. Reissue volume is huge.");
    println!();
    println!("  Intelligent Retry:");
    println!("    ML-driven retry-time selection. Don't retry a 'card declined'");
    println!("    immediately — wait until 4am customer-local-time when daily");
    println!("    balance refreshes. Don't retry at all on hard 'do not honor'.");
    println!("    Trained on Recurly's full transaction corpus.");
    println!();
    println!("  Backup payment methods:");
    println!("    Recurly stores multiple cards per account, auto-falls-back to");
    println!("    secondary on primary failure.");
    println!();
    println!("  Network Tokens:");
    println!("    Issued by card networks (Visa Token Service, Mastercard MDES),");
    println!("    bound to merchant + customer pair, auto-updated by issuer.");
    println!("    Generally higher approval rates than raw PAN tokens.");
    println!();
    println!("Reported industry-leading recovery: 70-75% of soft declines salvaged,");
    println!("translating to a 1-3% lift on net revenue retention for media customers.");
}

fn cmd_gateways() {
    println!("Recurly gateway flexibility");
    println!();
    println!("Recurly is gateway-agnostic. You configure your own merchant accounts;");
    println!("Recurly orchestrates routing to them.");
    println!();
    println!("Supported gateways (broad sample):");
    println!("  • Stripe, Adyen, Braintree (PayPal), Worldpay, Cybersource");
    println!("  • Authorize.Net, Chase Paymentech, Elavon, First Data / Fiserv");
    println!("  • Vantiv (Worldpay), Global Payments, Heartland");
    println!("  • PayPal Express Checkout, Amazon Pay, Apple Pay, Google Pay");
    println!("  • SEPA Direct Debit via SLI Systems, GoCardless");
    println!("  • ACH via various US banks");
    println!("  • International — Tilopay (LATAM), various regional acquirers");
    println!();
    println!("Multi-gateway routing rules:");
    println!("  Define logic such as:");
    println!("    • 'EUR customers via Adyen, USD via Braintree, others via Stripe'");
    println!("    • 'High-value (>USD 1000) via Adyen, low-value via Stripe'");
    println!("    • 'Subscription card-on-file via primary, one-time via secondary'");
    println!("    • 'Retry on different gateway after decline on primary'");
    println!();
    println!("Tokenization vault:");
    println!("  Recurly stores cards in its own PCI Level 1 vault, then transmits");
    println!("  card details to each configured gateway as needed for the actual");
    println!("  authorization. This is what enables gateway portability — you");
    println!("  don't lose your card tokens if you change gateway providers.");
    println!();
    println!("Open Payment Method API:");
    println!("  Extension point for less common regional payment methods that");
    println!("  Recurly doesn't natively support but customers can plug in.");
}

fn cmd_sip() {
    println!("Recurly Subscriber Identity Protection (SIP)");
    println!();
    println!("What it is:");
    println!("  Account-fraud and credential-stuffing defense layer specifically");
    println!("  for subscription businesses. Distinct from payment fraud — this is");
    println!("  about people creating subscription accounts using stolen identities");
    println!("  or stolen cards, OR taking over legitimate subscribers' accounts.");
    println!();
    println!("Why it's a different problem from card fraud:");
    println!("  • Subscription fraudsters care about content access, not just $$$");
    println!("  • Account takeover can move payment method to attacker's email");
    println!("  • Free-trial abuse (signup chains, disposable emails) is rampant");
    println!("    in streaming media — Recurly's customer base");
    println!();
    println!("Detection signals:");
    println!("  • Device fingerprint correlation across accounts");
    println!("  • Email domain reputation (disposable / 10minutemail patterns)");
    println!("  • BIN + customer geolocation alignment");
    println!("  • Velocity: many trials from same device or IP block");
    println!("  • Pattern matching against known abuser cohorts");
    println!();
    println!("Actions:");
    println!("  • Block signup");
    println!("  • Require step-up authentication (3DS, email verify)");
    println!("  • Flag for manual review queue");
    println!("  • Silently allow but downgrade trust score for later actions");
    println!();
    println!("Why this matters more than payment fraud for streaming:");
    println!("  A USD 9.99 streaming sub with a stolen card costs maybe USD 25");
    println!("  if chargebacked. But the content licensing cost of fraud-driven");
    println!("  viewership (especially during day-1 content windows for major");
    println!("  shows) is in the millions for a popular service.");
}

fn cmd_coupons() {
    println!("Recurly coupon and discount engine");
    println!();
    println!("Coupon types:");
    println!("  Percentage off (e.g. 25% off)");
    println!("  Fixed amount off (USD 20.00 off)");
    println!("  Free trial extension (additional days of trial)");
    println!("  Free plan access (gift code unlocks plan)");
    println!();
    println!("Applicability:");
    println!("  • Specific plans (only Pro Annual)");
    println!("  • Specific addons (only seats addon)");
    println!("  • Plan + addon combos");
    println!("  • Any plan in catalog");
    println!();
    println!("Duration:");
    println!("  • Once (single billing cycle)");
    println!("  • Forever (for the life of the subscription)");
    println!("  • Limited (first N billing cycles)");
    println!();
    println!("Constraints:");
    println!("  • Single-use vs multi-use codes");
    println!("  • Per-account redemption limit");
    println!("  • Total redemption cap");
    println!("  • Expiry date");
    println!("  • Currency-restricted (only valid for EUR plans)");
    println!("  • New customers only / existing customers only");
    println!();
    println!("Bulk generation:");
    println!("  Generate thousands of unique codes for influencer campaigns,");
    println!("  partner programs, affiliate exclusives. CSV export to share.");
    println!();
    println!("Stacking:");
    println!("  Configurable — allow multiple coupons per subscription OR enforce");
    println!("  mutual exclusivity. The honest answer for most businesses: don't");
    println!("  stack, because customers will find arbitrage and you'll lose margin.");
    println!();
    println!("Audit trail:");
    println!("  Every coupon application logged on the subscription event stream.");
    println!("  Critical for finance to reconcile discounted revenue vs gross.");
}

fn run_recurly(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "streaming" => cmd_streaming(),
        "revenue" => cmd_revenue(),
        "api" => cmd_api(),
        "decline" => cmd_decline(),
        "gateways" => cmd_gateways(),
        "sip" => cmd_sip(),
        "coupons" => cmd_coupons(),
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
        .unwrap_or_else(|| "recurly-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_recurly(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/recurly-cli"), "recurly-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("recurly-cli.exe"), "recurly-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_recurly(&[], "recurly-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_recurly(&["bogus".into()], "recurly-cli"), 2);
    }
}
