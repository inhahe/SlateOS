#![deny(clippy::all)]

//! prestashop-cli — SlateOS PrestaShop (French open-source e-commerce, EU favourite)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_presta(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: prestashop [OPTIONS]");
        println!("PrestaShop (Slate OS) — French open-source self-hosted e-commerce platform");
        println!();
        println!("Options:");
        println!("  --classic              Classic theme (free reference theme)");
        println!("  --hummingbird          Hummingbird (modern reference theme, 1.7+)");
        println!("  --modules              Modules marketplace (paid + free addons)");
        println!("  --account              PrestaShop Account (managed SaaS option, 2023+)");
        println!("  --multistore           Multi-store mode (run multiple shops from one back-office)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("PrestaShop 8.x (Slate OS)"); return 0; }
    println!("PrestaShop 8.x (Slate OS)");
    println!("  Vendor: PrestaShop SA (Paris, France — private, ex-Igil/MBO Partenaires backed)");
    println!("  Founders: Igor Schlumberger + Bruno Lévêque, 2005-2007");
    println!("          started as 'PhpOpenStore' student project at EPITECH (French IT school)");
    println!("          released 2007 as PrestaShop, open-sourced from day one");
    println!("  Founded: 2007 in Paris");
    println!("          received €9.3M Serie A from Seventure 2012");
    println!("          Sold majority stake to MBO Partenaires 2021 (French PE)");
    println!("          ~$30M+ ARR rough estimate (private, not disclosed)");
    println!("          ~200 employees");
    println!("          monetization shifted post-2021: more push on PrestaShop Account (SaaS) and Modules");
    println!("  Strategic position: 'European Magento' — open-source self-hosted commerce strong in EU + LATAM:");
    println!("                    primary competitor: Magento / Adobe Commerce (larger, more enterprise)");
    println!("                    competitor: WooCommerce (much larger globally), Shopify (SaaS)");
    println!("                    French-language + European focus: native VAT, GDPR, EU shipping integrations");
    println!("                    estimated 300K+ live stores, ~80% in Europe + LATAM (France, Spain, Italy especially)");
    println!("                    pitch: 'free, open-source, French-engineered alternative to Shopify'");
    println!("  Pricing:");
    println!("    Core platform — FREE, self-hosted, OSL 3.0 (Open Software License)");
    println!("    Hosting — your choice, $5-200+/mo (PrestaHosting, OVHcloud, IONOS, Hostinger)");
    println!("    Modules — $50-$500 each, several free");
    println!("    Themes — typically $80-300 (most stores use a custom theme)");
    println!("    PrestaShop Account (managed SaaS) — pricing varies, launching 2023+ for SMB ease");
    println!("  Architecture (PHP + MySQL, classical LAMP):");
    println!("    - PHP 8.1+ + Symfony components + MySQL/MariaDB");
    println!("    - Smarty templating (legacy) + Symfony Twig (newer back-office)");
    println!("    - Modular (one module = one feature) — easy to extend");
    println!("    - REST + GraphQL APIs (improving but not as full as Magento)");
    println!("    - Composer-based dep management (modern PrestaShop)");
    println!("    - much lighter than Magento — runs comfortably on €5/mo shared hosting");
    println!("  Core features (impressive for free):");
    println!("    - Multi-store mode from one back-office (run multiple brands, languages, currencies)");
    println!("    - Multi-currency + multi-language native");
    println!("    - Catalog: simple/virtual/packs/customizable products + variants + combinations");
    println!("    - Native VAT/tax management (heavy EU focus)");
    println!("    - Voucher + discount rules + 'price by quantity' tiers");
    println!("    - Loyalty/reward programs (module)");
    println!("    - Shipping carriers + complex rules (weight zones, etc.)");
    println!("    - Customer groups + B2B pricing");
    println!("    - Stats dashboard built into back-office");
    println!("  Modules marketplace (~3,500 modules, the revenue engine):");
    println!("    - Payment gateways: Stripe, PayPal, Mollie, HiPay, Adyen");
    println!("    - Shipping: Mondial Relay, Chronopost, Colissimo (EU carriers), Sendcloud");
    println!("    - Marketing: newsletter, Mailchimp sync, Klaviyo, social media");
    println!("    - SEO + URL rewrite + sitemap + structured data");
    println!("    - Marketplace + dropshipping modules");
    println!("    - Analytics: Google Analytics, GTM, Matomo");
    println!("    - quality varies — some modules are extremely buggy");
    println!("  Hummingbird theme (2023):");
    println!("    - New default theme replacing Classic");
    println!("    - Built with Tailwind CSS, mobile-first, faster Core Web Vitals");
    println!("    - PrestaShop's attempt to modernize default UX");
    println!("  PrestaShop Account (managed SaaS, 2023+ push):");
    println!("    - Hosted PrestaShop without self-hosting hassle");
    println!("    - PrestaShop's bet to compete with Shopify on ease");
    println!("    - Still maturing — pricing tiers not yet competitive with Shopify Basic");
    println!("  EU-specific strengths:");
    println!("    - Native EU VAT handling + EU One-Stop-Shop");
    println!("    - GDPR-compliant by default");
    println!("    - Carriers: Mondial Relay, Chronopost, Colissimo, DPD, GLS, DHL");
    println!("    - Payment: SEPA, Klarna, Apple Pay/Google Pay via PrestaShop Payments (Stripe-powered)");
    println!("    - Multi-lingual EU language packs all maintained");
    println!("    - French government uses PrestaShop for several public e-commerce projects");
    println!("  Customers: ~300,000 active stores worldwide");
    println!("            Le Slip Français, Toupargel, Smood, Krys (French), Galeries Lafayette (some lines)");
    println!("            many EU regional retailers, fashion boutiques, food/wine producers");
    println!("            sweet spot: EU SMB/mid-market wanting open-source + free + local features");
    println!("            very weak in: North America (Shopify dominates), enterprise (Magento/SFCC win)");
    println!("  Critique: smaller community than Magento or WooCommerce globally");
    println!("           module quality varies — combining many modules often causes conflicts");
    println!("           upgrade pain similar to Magento — breaks themes/modules");
    println!("           less attractive to North American merchants (brand recognition + ecosystem)");
    println!("           PrestaShop Account (SaaS) still early — Shopify has 17-year head start on SaaS UX");
    println!("           AI features lag US competitors significantly");
    println!("           perceived as 'French-centric' — less polished for English-speaking SMBs");
    println!("  Differentiator: free + open source + native EU features (VAT, GDPR, carriers, languages) + multi-store from day one — the default platform for European SMB e-commerce");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "prestashop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_presta(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_presta};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/prestashop"), "prestashop");
        assert_eq!(basename(r"C:\bin\prestashop.exe"), "prestashop.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("prestashop.exe"), "prestashop");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_presta(&["--help".to_string()], "prestashop"), 0);
        assert_eq!(run_presta(&["-h".to_string()], "prestashop"), 0);
        let _ = run_presta(&["--version".to_string()], "prestashop");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_presta(&[], "prestashop");
    }
}
