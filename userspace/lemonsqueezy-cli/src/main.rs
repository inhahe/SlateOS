#![deny(clippy::all)]
//! lemonsqueezy-cli — OurOS Lemon Squeezy indie-MoR personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Lemon Squeezy (when life gives you payments)");
    println!();
    println!("USAGE:");
    println!("    {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about         JR Farr, 2021, indie hackers' best friend");
    println!("    mor           Merchant-of-Record for digital goods");
    println!("    stripe        The Stripe acquisition (Jul 2024)");
    println!("    products      Digital products + subscriptions + licenses");
    println!("    api           Lemon Squeezy REST API");
    println!("    affiliates    Built-in affiliate program");
    println!("    fees          Pricing model (5% + 50c US)");
    println!("    help / version");
}

fn print_version() {
    println!("lemonsqueezy-cli 0.1.0 — OurOS personality binary");
    println!("Lemon Squeezy (acquired by Stripe, Jul 2024)");
}

fn cmd_about() {
    println!("Lemon Squeezy — All-in-one platform for selling digital products.");
    println!();
    println!("Founded:  2021 by JR Farr + co-founders");
    println!("          JR Farr was previously known in the indie hacker / Twitter");
    println!("          design world (Make Lemonade, GitHub avatar designer).");
    println!();
    println!("Original positioning:");
    println!("  'Stripe for indie hackers' — but with built-in:");
    println!("    • Merchant of Record (handles global tax for you)");
    println!("    • Hosted checkout pages");
    println!("    • License key generation + management");
    println!("    • Affiliate marketing system");
    println!("    • Subscription billing");
    println!("    • Email receipts + customer portal");
    println!();
    println!("Vibe:");
    println!("  Bright yellow brand. Lemon mascot. Marketing tone of voice that");
    println!("  reads like an indie hacker Twitter account. Aggressively friendly");
    println!("  to small creators selling templates, plugins, ebooks, courses,");
    println!("  and small SaaS — the sub-USD-1M-ARR segment.");
    println!();
    println!("Growth:");
    println!("  Reached ~USD 21M annualized GMV-share revenue by early 2024.");
    println!("  ~30,000 stores / merchants. Mostly indie + small SaaS.");
    println!();
    println!("Exit: see 'lemonsqueezy-cli stripe' for the Jul 2024 acquisition.");
}

fn cmd_mor() {
    println!("Lemon Squeezy as Merchant of Record");
    println!();
    println!("Same legal structure as Paddle, FastSpring, DigitalRiver, etc:");
    println!("  Lemon Squeezy is the legal seller; you supply the digital good.");
    println!("  Customer's card statement says 'LEMONSQUEEZY*<yourbrand>'.");
    println!();
    println!("What Lemon Squeezy handles:");
    println!("  • US sales tax (45 states, marketplace facilitator laws)");
    println!("  • EU VAT (OSS scheme, customer-location based)");
    println!("  • UK VAT (post-Brexit)");
    println!("  • Australia GST, Singapore GST, Norway VAT, Switzerland VAT,");
    println!("    South Korea VAT, Taiwan VAT, India GST, NZ GST, JP JCT, etc.");
    println!("  • Total: ~40+ jurisdictions monitored and remitted");
    println!("  • Chargebacks (LS absorbs the EUR 25-30 chargeback fee)");
    println!("  • Refund processing");
    println!("  • Customer support for billing issues (first-line)");
    println!();
    println!("What you keep:");
    println!("  • Your customer relationship (LS gives you their email)");
    println!("  • Product fulfillment (file delivery, license keys, account setup)");
    println!("  • Marketing, support for product itself");
    println!();
    println!("Sweet spot for LS-as-MoR:");
    println!("  • Indie developers selling globally");
    println!("  • Less-than-USD-1M ARR — too small to want to deal with sales tax");
    println!("  • Digital products (templates, themes, plugins, ebooks, courses)");
    println!("  • Indie SaaS where billing complexity > price difference vs Stripe");
}

fn cmd_stripe() {
    println!("The Stripe acquisition — Jul 9, 2024");
    println!();
    println!("Announcement:");
    println!("  Stripe acquires Lemon Squeezy. Terms undisclosed. JR Farr and");
    println!("  team join Stripe. Lemon Squeezy continues to operate as a");
    println!("  standalone product within Stripe.");
    println!();
    println!("The strategic rationale (from Stripe's side):");
    println!("  Stripe historically said 'we don't do MoR' — partly principled");
    println!("  (devs should own their merchant relationships), partly because");
    println!("  MoR conflicts with Stripe's existing payment-facilitator model.");
    println!();
    println!("  Buying Lemon Squeezy lets Stripe:");
    println!("    1. Offer MoR to merchants who explicitly want it");
    println!("    2. Compete with Paddle and FastSpring head-on");
    println!("    3. Capture the indie hacker segment (Stripe's natural audience");
    println!("       but increasingly going to LS for tax reasons)");
    println!("    4. Acquire a polished checkout UX (LS's hosted checkout is good)");
    println!();
    println!("The strategic rationale (from LS's side):");
    println!("  • Hard to compete on payment routing with Stripe as the underlying");
    println!("    PSP (LS was built on Stripe rails)");
    println!("  • Acquisition price likely a premium on USD 21M ARR");
    println!("  • Team gets to keep building the product with vastly more resources");
    println!();
    println!("Post-acquisition:");
    println!("  Lemon Squeezy remains a separate product under the lemonsqueezy.com");
    println!("  domain. Some Stripe-side branding starts appearing ('Lemon Squeezy");
    println!("  is now part of Stripe'). Roadmap continues largely intact through");
    println!("  end of 2024 / early 2025.");
}

fn cmd_products() {
    println!("Lemon Squeezy product types");
    println!();
    println!("Digital downloads:");
    println!("  Single-purchase files (PDFs, ZIPs, themes, fonts, audio, video).");
    println!("  Time-limited and IP-limited download links. License key optional.");
    println!();
    println!("Subscriptions:");
    println!("  Recurring billing on monthly / yearly / weekly / quarterly cadence.");
    println!("  Multi-variant pricing (e.g. Basic / Pro / Team tiers).");
    println!("  Free trials with or without card capture.");
    println!("  Coupon + discount support.");
    println!();
    println!("Pay What You Want:");
    println!("  Customer-set pricing with optional minimum floor.");
    println!("  Popular for tip-jar style indie creator monetization.");
    println!();
    println!("Bundles:");
    println!("  Multiple products sold as one SKU with discount.");
    println!();
    println!("License Keys:");
    println!("  Auto-generated unique keys per purchase.");
    println!("  Instance-based activation (limit concurrent activations).");
    println!("  License validation API for your app to call back to LS.");
    println!();
    println!("Memberships / SaaS:");
    println!("  Use subscriptions + license API to gate a SaaS app.");
    println!("  Customer portal lets users manage their own subscription.");
    println!();
    println!("Lemon.fm (deprecated / spun out):");
    println!("  Briefly tried a music platform angle. Quietly retired.");
}

fn cmd_api() {
    println!("Lemon Squeezy REST API");
    println!();
    println!("Base URL: https://api.lemonsqueezy.com/v1");
    println!("Auth:     Bearer token (PAT — Personal Access Token)");
    println!("Format:   JSON:API spec (resource objects, includes, relationships)");
    println!();
    println!("Core resources:");
    println!("  GET  /stores              — your stores");
    println!("  GET  /products            — products in a store");
    println!("  GET  /variants            — variants (price tiers) of a product");
    println!("  POST /checkouts           — create a one-time checkout session");
    println!("  GET  /orders              — order history");
    println!("  GET  /subscriptions       — recurring subscriptions");
    println!("  POST /subscriptions/{{id}}/cancel — cancel a subscription");
    println!("  GET  /license-keys        — issued license keys");
    println!("  POST /license-keys/activate — activate / verify a license");
    println!();
    println!("Checkout flow:");
    println!("  1. POST /checkouts with productOptions + checkoutData (email, etc.)");
    println!("  2. Response includes a hosted checkout URL");
    println!("  3. Redirect customer or embed via Lemon.js overlay");
    println!();
    println!("Webhooks:");
    println!("  Signed (HMAC-SHA256, X-Signature header) events:");
    println!("    order_created, order_refunded,");
    println!("    subscription_created, subscription_updated, subscription_cancelled,");
    println!("    subscription_resumed, subscription_expired, subscription_paused,");
    println!("    subscription_unpaused, subscription_payment_failed,");
    println!("    license_key_created, license_key_updated");
}

fn cmd_affiliates() {
    println!("Lemon Squeezy affiliate program");
    println!();
    println!("Why it exists in-product:");
    println!("  Indie creators often want a partner/affiliate program but the");
    println!("  tooling (Rewardful, FirstPromoter, PartnerStack) is expensive");
    println!("  and overkill for small operations. Lemon Squeezy bakes it in.");
    println!();
    println!("Features:");
    println!("  • Per-store affiliate signup pages (whitelabel-friendly)");
    println!("  • Per-product OR store-wide commission rates");
    println!("  • Recurring commissions on subscription products (lifetime or N-month)");
    println!("  • Cookie attribution window (configurable, default 30 days)");
    println!("  • Auto-generated trackable links per affiliate");
    println!("  • Coupon-based attribution (affiliate code as discount)");
    println!("  • Built-in payout via PayPal or Wise");
    println!();
    println!("How it works for the merchant:");
    println!("  1. Enable affiliate program in store settings");
    println!("  2. Set commission rates and policies");
    println!("  3. Share affiliate signup URL");
    println!("  4. Affiliates self-register, get a dashboard, share links");
    println!("  5. LS tracks attribution and accrues commissions per sale");
    println!("  6. Merchant approves payouts; LS handles the actual disbursement");
    println!();
    println!("Note: affiliate fees come out of merchant's net revenue, not LS's.");
    println!("LS's take rate is unchanged whether you have affiliates or not.");
}

fn cmd_fees() {
    println!("Lemon Squeezy pricing");
    println!();
    println!("No monthly fee. No setup fee. No PCI fee. Purely per-transaction.");
    println!();
    println!("Standard rate:");
    println!("  5% + USD 0.50 per transaction (US-issued cards)");
    println!();
    println!("Why ~5%? Because LS-as-MoR includes:");
    println!("  • Underlying card processing (~2.5-3% to Stripe / acquirer)");
    println!("  • Tax registration + filing across 40+ jurisdictions");
    println!("  • Chargeback absorption");
    println!("  • Tax liability");
    println!("  • Hosted checkout + customer support infrastructure");
    println!();
    println!("Compare:");
    println!("  • Stripe direct:    2.9% + USD 0.30  (you handle tax + chargebacks)");
    println!("  • Lemon Squeezy:    5%   + USD 0.50  (MoR — they handle all of it)");
    println!("  • Paddle:           5%   + USD 0.50  (MoR — established competitor)");
    println!("  • FastSpring:       5.9% + USD 0.95  (legacy MoR, higher rate)");
    println!();
    println!("The take rate gap (5% vs 2.9%) is roughly 2 percentage points —");
    println!("for most digital sellers below USD 1M ARR, that's much less than");
    println!("the engineering cost of building in-house tax compliance.");
    println!("Above ~USD 10M ARR, the math flips and direct integrations win.");
}

fn run_lemon(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "mor" => cmd_mor(),
        "stripe" => cmd_stripe(),
        "products" => cmd_products(),
        "api" => cmd_api(),
        "affiliates" => cmd_affiliates(),
        "fees" => cmd_fees(),
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
        .unwrap_or_else(|| "lemonsqueezy-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_lemon(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lemonsqueezy-cli"), "lemonsqueezy-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lemonsqueezy-cli.exe"), "lemonsqueezy-cli");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_lemon(&[], "lemonsqueezy-cli"), 0);
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_lemon(&["bogus".into()], "lemonsqueezy-cli"), 2);
    }
}
