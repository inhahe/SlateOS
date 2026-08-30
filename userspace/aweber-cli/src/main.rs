#![deny(clippy::all)]
//! aweber-cli — personality CLI for AWeber, one of the original autoresponder
//! companies still operating today.
//!
//! Founded 1998 by Tom Kulzer in Chalfont, Pennsylvania — the same year as
//! GetResponse and ConstantContact, putting it firmly in the founding
//! cohort of commercial email marketing software. AWeber is famous for
//! having largely stayed bootstrapped + privately held for its entire
//! ~27 year history while many of its peers were acquired into bigger
//! marketing-cloud parents. Has remained a small-to-midsize company
//! optimised for SMB ease-of-use over enterprise feature breadth.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — AWeber 1998-era SMB email personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Tom Kulzer 1998 Pennsylvania, bootstrapped");
    println!("    autoresponders Original product, still core");
    println!("    smartdesigner Auto-pull brand + colors from a website URL");
    println!("    landingpages  Hosted page builder with stock photo library");
    println!("    ai            AI Writing Assistant + email subject suggestions");
    println!("    integrations  PayPal, Etsy, WordPress, Wix, Shopify, etc.");
    println!("    pricing       Free up to 500 subs, paid above");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("aweber-cli 0.1.0 (founding-cohort autoresponder personality build)"); }

fn run_about() {
    println!("AWeber Communications, Inc.");
    println!("  Founded:    1998, Chalfont, Pennsylvania.");
    println!("  Founder:    Tom Kulzer (still CEO).");
    println!("  Funding:    Bootstrapped + privately held for ~27 years.");
    println!("  Pioneer:    One of the very first autoresponder vendors;");
    println!("              shipped scheduled drip campaigns before the term");
    println!("              'email marketing' existed in its current sense.");
    println!("  Posture:    SMB + solopreneur focus; opinion is that small biz");
    println!("              owners do not want a Salesforce-grade tool.");
    println!("  Footprint:  100K+ paying customers historically.");
}

fn run_autoresponders() {
    println!("Autoresponders — the original product.");
    println!("  Sequences of pre-written emails sent on schedule after signup.");
    println!("  Classic use case: lead magnet -> welcome series -> sales pitch.");
    println!("  Time-based delays in days or hours; condition-based branching");
    println!("  added in later product generations.");
    println!("  Broadcasts: one-off newsletter sends, with simple A/B testing.");
}

fn run_smartdesigner() {
    println!("AWeber Smart Designer.");
    println!("  Paste a website URL; AWeber pulls logo, brand colours, fonts,");
    println!("  images, and generates a coherent set of email templates branded");
    println!("  to that site automatically.");
    println!("  Aimed at users who don't want to spend an hour in a drag-drop");
    println!("  editor to get the company colours right.");
    println!("  Generated templates are then editable.");
}

fn run_landingpages() {
    println!("Landing pages + hosted forms.");
    println!("  Drag-drop builder, hosted at *.aweber.page or custom domain.");
    println!("  Stock photo + GIF library built in.");
    println!("  Forms: inline, popup, slide-in, exit-intent triggers.");
    println!("  Signups go directly into AWeber lists with no integration setup.");
    println!("  Conversion analytics per page + per form.");
}

fn run_ai() {
    println!("AI Writing Assistant.");
    println!("  Generate full email drafts from a topic + audience prompt.");
    println!("  Subject line suggestions + scoring.");
    println!("  AI image generation for in-email visuals.");
    println!("  Tone shift (formal/casual/witty) for any drafted block.");
    println!("  Backed by third-party LLM APIs under the hood.");
}

fn run_integrations() {
    println!("Integrations library.");
    println!("  PayPal: trigger sequences off purchases, refunds, subscriptions.");
    println!("  Etsy + Shopify + WooCommerce: ecom contact + order sync.");
    println!("  WordPress + Wix + Squarespace: signup form embeds.");
    println!("  Zapier + Make: thousands of indirect integrations.");
    println!("  REST API: documented, OAuth2, decent SDK coverage.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free          up to 500 subscribers, basic features, AWeber branding.");
    println!("  Lite          ~$15/mo, unlimited subscribers, no branding.");
    println!("  Plus          per-contact tier, unlimited everything + advanced.");
    println!("  Unlimited     enterprise contract, dedicated rep, unlimited.");
    println!("  All plans: unlimited sends. No charge for subscriber import.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Tens of thousands of US SMBs, churches, non-profits, restaurants,");
    println!("  consultants, info-marketers — the long tail of US small business.");
    println!("  Internet-marketing community: heavy use historically among the");
    println!("  affiliate-marketer + course-creator crowd.");
    println!("  Long product longevity vs newer entrants = stable lock-in base.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "aweber-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "autoresponders" => run_autoresponders(),
        "smartdesigner" => run_smartdesigner(),
        "landingpages" => run_landingpages(),
        "ai" => run_ai(),
        "integrations" => run_integrations(),
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
        run_autoresponders();
        run_smartdesigner();
        run_landingpages();
        run_ai();
        run_integrations();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("aweber-cli");
        print_version();
    }
}
