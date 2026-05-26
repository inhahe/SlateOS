#![deny(clippy::all)]
//! omnisend-cli — personality CLI for Omnisend, the Lithuanian ecommerce
//! email + SMS + push automation platform.
//!
//! Founded 2014 in Vilnius, Lithuania by Rytis Lauris and Justas Kriukas
//! as Soundest (a Shopify newsletter plugin), then renamed Omnisend in 2017
//! to reflect the move to multi-channel — email, SMS, web push, Facebook
//! Messenger, even direct mail. Self-funded in early years and then took
//! modest growth investment. Headcount runs around 600 people, most based
//! in Vilnius. Like Klaviyo, the wedge is Shopify ecommerce stores that
//! want pre-built ecom workflows rather than a generic ESP.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Omnisend Lithuanian ecom-multichannel personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Lauris+Kriukas 2014 Vilnius, ex-Soundest");
    println!("    channels      Email + SMS + push + Messenger + direct mail");
    println!("    automations   Pre-built ecom workflows library");
    println!("    forms         Popup, signup, wheel-of-fortune gamified");
    println!("    segments      Lifecycle + RFM + predictive segments");
    println!("    sms           Per-message TCPA-compliant SMS in same flow");
    println!("    pricing       Free up to 500 emails, contact-based tiers");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("omnisend-cli 0.1.0 (multichannel-ecom personality build)"); }

fn run_about() {
    println!("Omnisend (Omnisend Ltd).");
    println!("  Founded:    2014, Vilnius, Lithuania (originally as Soundest).");
    println!("  Founders:   Rytis Lauris (CEO), Justas Kriukas.");
    println!("  Rebrand:    Soundest -> Omnisend in 2017 with multi-channel pivot.");
    println!("  Funding:    Self-funded early; modest growth rounds since.");
    println!("  Posture:    Ecommerce-only positioning; refused enterprise generic-ESP framing.");
    println!("  Headcount:  ~600, predominantly Vilnius office.");
    println!("  Footprint:  100K+ ecommerce brands, mostly Shopify + WooCommerce.");
}

fn run_channels() {
    println!("Channels — multi by design, not bolt-on.");
    println!("  Email     drag-drop editor, ecom-aware product blocks.");
    println!("  SMS       global SMS + MMS, TCPA + GDPR consent built-in.");
    println!("  Web push  browser push (Chrome/Firefox) without an app.");
    println!("  Facebook Messenger sponsored messages where the policy allows.");
    println!("  Direct mail Postcard integrations via partner postcard APIs.");
    println!("  All channels usable as nodes inside the same workflow.");
}

fn run_automations() {
    println!("Pre-built workflow library.");
    println!("  Welcome series, abandoned cart, browse abandonment,");
    println!("  order confirmation, shipping notification, post-purchase upsell,");
    println!("  cross-sell, win-back, replenishment reminders, birthday.");
    println!("  Each ships as a ready-to-launch template, editable in canvas view.");
    println!("  Splits + filters: tag, segment, channel availability, A/B.");
}

fn run_forms() {
    println!("Forms + popups.");
    println!("  Standard signup popups, slide-ins, embed forms, sticky bars.");
    println!("  Gamified: 'Wheel of Fortune' style popup that boosts conversion.");
    println!("  Multi-step: email first, SMS opt-in second screen.");
    println!("  Targeting: time-on-page, exit-intent, scroll depth, URL, country.");
    println!("  Inline + popup conversion analytics per form.");
}

fn run_segments() {
    println!("Segments — RFM + lifecycle.");
    println!("  Lifecycle: new, active, at-risk, lost, VIP — auto-classified.");
    println!("  RFM: recency / frequency / monetary scoring.");
    println!("  Predictive: customer lifetime value, churn probability.");
    println!("  Behavioural: viewed product, clicked email, opened SMS,");
    println!("              abandoned checkout, completed purchase.");
    println!("  Combine into dynamic segments that recompute on event ingest.");
}

fn run_sms() {
    println!("SMS as first-class.");
    println!("  Global coverage with country-specific compliance presets.");
    println!("  TCPA + GDPR consent capture during signup or checkout.");
    println!("  Per-message pricing, included credit at higher tiers.");
    println!("  SMS analytics: delivery, click, conversion, revenue.");
    println!("  Two-way SMS for replies + auto-responses.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free        500 emails/mo + 60 SMS, up to 250 contacts.");
    println!("  Standard    contact-based, indicative ~$16/mo for 500 contacts.");
    println!("  Pro         adds push + advanced reports + dedicated success.");
    println!("  SMS         credits separate from email pricing tiers.");
    println!("  No SMS or send caps on paid tiers (within fair-use).");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  100K+ ecommerce brands, mostly Shopify and WooCommerce.");
    println!("  Strong base of Eastern European, UK, and Australian merchants.");
    println!("  Niche: DTC apparel, beauty, home goods, supplements.");
    println!("  Public case studies on omnisend.com show typical Shopify GMV lift.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "omnisend-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "channels" => run_channels(),
        "automations" => run_automations(),
        "forms" => run_forms(),
        "segments" => run_segments(),
        "sms" => run_sms(),
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
        run_channels();
        run_automations();
        run_forms();
        run_segments();
        run_sms();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("omnisend-cli");
        print_version();
    }
}
