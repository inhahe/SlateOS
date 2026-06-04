#![deny(clippy::all)]

//! ecwid-cli — OurOS Ecwid by Lightspeed (embeddable e-commerce widget)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ecwid(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ecwid [OPTIONS]");
        println!("Ecwid by Lightspeed (OurOS) — embeddable e-commerce store you can drop into any existing site");
        println!();
        println!("Options:");
        println!("  --free                 Free — up to 5 products, basic features");
        println!("  --venture              Venture — $19/mo (100 products, basic commerce)");
        println!("  --business             Business — $39/mo (2,500 products, abandoned cart)");
        println!("  --unlimited            Unlimited — $99/mo");
        println!("  --instant              Instant Site (free landing page if you don't have a site)");
        println!("  --facebook             Facebook + Instagram shop sync");
        println!("  --mobile               iOS/Android shopping apps for your store");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ecwid by Lightspeed 2024 (OurOS)"); return 0; }
    println!("Ecwid by Lightspeed 2024 (OurOS)");
    println!("  Vendor: Lightspeed Commerce Inc. (Montreal, Canada — NYSE/TSX:LSPD)");
    println!("  Original: Ecwid Inc. (Ulyanovsk, Russia / Encinitas CA — founded 2009 by Ruslan Fazlyev)");
    println!("  Acquired: by Lightspeed for $500M (mix cash+stock), closed Aug 2021");
    println!("  Founders: Ruslan Fazlyev, 2009 (also founder of X-Cart, an earlier PHP cart platform)");
    println!("          built Ecwid as a JavaScript embed widget — radically different model from Shopify");
    println!("  Founded: 2009 in Ulyanovsk (founder relocated team to US post-2014 Ukraine tensions)");
    println!("          bootstrapped to ~$30M ARR before Lightspeed acquisition");
    println!("          ~150K paying customers + 1M+ free stores at time of acquisition");
    println!("          rebranded post-acquisition: 'Ecwid by Lightspeed' (kept Ecwid name as sub-brand)");
    println!("  Strategic position: 'add a store to a site you already have':");
    println!("                    DIFFERENT pitch from Shopify: 'don't replace your site, add commerce to it'");
    println!("                    primary competitor: Shopify Buy Button + Shopify Lite (discontinued), Snipcart");
    println!("                    competitor for SMBs: Square Online (free + Square processing), GoDaddy Online Store");
    println!("                    sweet spot: existing site (WordPress / Squarespace / Wix / Webflow / custom) wanting commerce");
    println!("                    post-Lightspeed: positioned as 'commerce extension of Lightspeed Retail + Restaurant POS'");
    println!("  Pricing (transparent, lower than Shopify):");
    println!("    Free — up to 5 products, basic store, Ecwid branding, no app market apps");
    println!("    Venture — $19/mo — 100 products, Facebook/Instagram sync, multilingual");
    println!("    Business — $39/mo — 2,500 products, abandoned cart, advanced reports, subscriptions");
    println!("    Unlimited — $99/mo — unlimited products + B2B features");
    println!("    annual discounts ~30%");
    println!("    NO transaction fees (uses your payment processor)");
    println!("    Lightspeed Payments — 2.6% + 10¢ for in-person, 2.6% + 30¢ online");
    println!("  Core architecture (the embeddable widget):");
    println!("    - JavaScript widget loaded into any HTML page via a single <script> tag");
    println!("    - Renders the storefront on the host page — your site keeps its design");
    println!("    - Cloud-hosted backend (Ecwid handles servers, DB, payments)");
    println!("    - REST API + JS Storefront API + iframe + native iOS/Android SDKs");
    println!("    - Storefront is responsive + customizable via CSS overrides + apps");
    println!("    - Headless option via API for fully custom storefronts");
    println!("  Where you can embed Ecwid:");
    println!("    - WordPress (official plugin + WooCommerce-like experience)");
    println!("    - Squarespace, Wix (via embed code)");
    println!("    - Webflow, Carrd, Tilda, Joomla, Drupal");
    println!("    - Custom HTML + plain JS");
    println!("    - Instant Site (Ecwid-hosted landing if you have NO site)");
    println!("    - Facebook Page, Instagram Shopping, TikTok Shop");
    println!("  Channels + sales surfaces:");
    println!("    - Your existing website (any platform)");
    println!("    - Instant Site (free landing page)");
    println!("    - Facebook + Instagram shops (deep integration, BuyButton via Meta API)");
    println!("    - Google Shopping (free organic + paid)");
    println!("    - TikTok Shop");
    println!("    - Amazon, eBay (via apps)");
    println!("    - Mobile shopping apps (iOS + Android) — Ecwid generates these for your store!");
    println!("    - POS via Ecwid POS (mobile) or Lightspeed Retail (omnichannel)");
    println!("  Features:");
    println!("    - Multilingual storefront (51 languages, auto-localized)");
    println!("    - Multi-currency");
    println!("    - Digital products + downloadables");
    println!("    - Subscriptions (Business+ tier)");
    println!("    - Abandoned cart auto-emails");
    println!("    - Discount codes + automatic discounts");
    println!("    - Shipping calculator (USPS, FedEx, UPS, Canada Post, Royal Mail)");
    println!("    - Tax: TaxJar integration + manual rates");
    println!("    - Inventory tracking + low-stock alerts");
    println!("  Lightspeed integration (post-2021 acquisition):");
    println!("    - Lightspeed Retail POS + Ecwid: unified inventory online + in-store");
    println!("    - Lightspeed Restaurant + Ecwid: online ordering for restaurants");
    println!("    - Lightspeed Payments: integrated processor");
    println!("    - target: restaurants + retail SMBs that want SaaS POS + simple online presence");
    println!("  Customers: 1M+ active stores, ~130K+ paying");
    println!("            massive long-tail of SMBs in 175 countries");
    println!("            popular with: musicians selling merch, indie creators, café/bakery online ordering, hobbyist sellers");
    println!("            'most popular store on WordPress' for hobby-sellers who don't want WooCommerce complexity");
    println!("            sweet spot: $0-$50K/yr GMV SMBs with existing site");
    println!("  Critique: widget-based architecture limits design control vs full Shopify storefront");
    println!("           customization (theme depth) far weaker than Shopify or BigCommerce");
    println!("           reporting basic vs Shopify Analytics");
    println!("           apps marketplace small (~200 apps vs Shopify's 8,000+)");
    println!("           post-Lightspeed acquisition: roadmap focus shifted to omnichannel POS integration vs pure SaaS commerce growth");
    println!("           Lightspeed itself struggling with stock (~$10 vs $100+ peak) — affects long-term Ecwid investment confidence");
    println!("           brand awareness much lower than Shopify or BigCommerce");
    println!("  Differentiator: drop-in embeddable widget that adds commerce to ANY existing site + mobile apps generated for free + Lightspeed POS integration — for SMBs who don't want to migrate their site");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ecwid".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ecwid(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ecwid};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ecwid"), "ecwid");
        assert_eq!(basename(r"C:\bin\ecwid.exe"), "ecwid.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ecwid.exe"), "ecwid");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ecwid(&["--help".to_string()], "ecwid"), 0);
        assert_eq!(run_ecwid(&["-h".to_string()], "ecwid"), 0);
        let _ = run_ecwid(&["--version".to_string()], "ecwid");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ecwid(&[], "ecwid");
    }
}
