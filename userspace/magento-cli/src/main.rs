#![deny(clippy::all)]

//! magento-cli — SlateOS Adobe Commerce / Magento (enterprise self-hosted e-commerce)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_magento(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: magento [OPTIONS]");
        println!("Adobe Commerce / Magento (SlateOS) — enterprise self-hosted + cloud e-commerce");
        println!();
        println!("Options:");
        println!("  --opensource           Magento Open Source (free, self-hosted)");
        println!("  --adobecommerce        Adobe Commerce (paid, on-prem or Adobe-hosted)");
        println!("  --cloud                Adobe Commerce Cloud (managed AWS hosting)");
        println!("  --b2b                  B2B Commerce (corporate accounts, quote-to-cash)");
        println!("  --pwa                  PWA Studio (headless React frontend framework)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adobe Commerce / Magento 2.4 (SlateOS)"); return 0; }
    println!("Adobe Commerce / Magento 2.4 (SlateOS)");
    println!("  Vendor: Adobe Inc. (San Jose, CA — NASDAQ:ADBE)");
    println!("  Originally: Magento Inc. acquired by eBay 2011 ($180M), spun out 2015, acquired by Adobe May 2018 ($1.68B)");
    println!("  Founders: Roy Rubin (CEO), Yoav Kutner (CTO), Bob Schwartz, 2008 — originally Varien LLC consultancy in 2007");
    println!("          Magento was the third commercial attempt by Varien (consulting business) at building open-source e-commerce");
    println!("          beat osCommerce + Zen Cart with a PHP 5 architecture + EAV product model + extensible event system");
    println!("          Kutner left 2012 to found Oro Inc. (OroCRM + OroCommerce) — competes with Adobe Commerce in B2B");
    println!("  Founded: 2008 (Magento 1) — Magento 2 launched 2015 (complete rewrite, breaking change)");
    println!("          Magento 1 end-of-life June 2020 — caused massive merchant migration crisis");
    println!("          Adobe acquired May 2018 to add commerce to Adobe Experience Cloud");
    println!("          rebranded 'Magento Commerce' → 'Adobe Commerce' (2020)");
    println!("          Magento Open Source still maintained as free version");
    println!("          ~150K live stores running Magento (down from peak ~250K)");
    println!("  Strategic position: 'enterprise commerce within Adobe Experience Cloud':");
    println!("                    Adobe sells it bundled with AEM + Marketo + Workfront + Target + Analytics");
    println!("                    primary head-to-head competitor: Salesforce Commerce Cloud (Demandware)");
    println!("                    pressure from: Shopify Plus going up-market, commercetools composable wave");
    println!("                    legacy strength: massive partner ecosystem (10K+ devs, 1K+ agencies)");
    println!("                    weakness: still PHP + complex deployments — hard to staff in 2024");
    println!("  Editions + pricing:");
    println!("    Magento Open Source — FREE, self-hosted, full source code (Apache 2.0)");
    println!("    Adobe Commerce — paid, license + your hosting (typically $22K-$190K+/yr depending on GMV)");
    println!("    Adobe Commerce Cloud — Adobe-hosted on AWS ($35K-$400K+/yr typical)");
    println!("    pricing scales by avg-GMV per year — opaque enterprise contracts");
    println!("    requires Solution Partner agency for implementation (typically $200K-$2M+ project)");
    println!("  Core architecture (PHP, MySQL, EAV product model):");
    println!("    - PHP 8.1+ + MySQL/MariaDB + Elasticsearch + Redis + Varnish");
    println!("    - EAV (Entity-Attribute-Value) catalog model — infinite product attributes");
    println!("    - Modular architecture: every feature is a 'module', overridable via DI + plugins");
    println!("    - GraphQL API + REST API + SOAP API (legacy)");
    println!("    - Event-driven extensibility (observers + plugins)");
    println!("    - Composer-based dependency management (modern Magento)");
    println!("    - notoriously complex — Magento certified developers command $150K+ salaries");
    println!("  Features included out-of-box (most complete of any platform):");
    println!("    - Multi-site, multi-store, multi-currency, multi-language native");
    println!("    - Tiered + grouped + bundled + downloadable products");
    println!("    - Complex catalog rules (cart price rules, catalog price rules)");
    println!("    - Native B2B: corporate accounts, quote system, requisition lists");
    println!("    - Inventory management with reservations + sources (multi-warehouse)");
    println!("    - Customer segmentation + targeted promotions (Commerce only)");
    println!("    - Page Builder (drag-and-drop CMS, Commerce only)");
    println!("    - Email reminders (cart abandonment, wishlist, etc.)");
    println!("  PWA Studio:");
    println!("    - Adobe-supported React-based PWA storefront framework");
    println!("    - Adobe's answer to Shopify Hydrogen + BigCommerce Catalyst");
    println!("    - Adoption modest — most agencies still build custom React frontends");
    println!("  Adobe integrations (post-2018 acquisition push):");
    println!("    - Adobe Experience Manager (AEM) for content");
    println!("    - Adobe Target for A/B testing + personalization");
    println!("    - Adobe Analytics for behavioral data");
    println!("    - Marketo Engage for B2B email + lead nurturing");
    println!("    - Sensei AI for product recommendations + visual search");
    println!("  Extension marketplace:");
    println!("    - 3,800+ extensions in Magento Marketplace");
    println!("    - extensions historically of varying quality (some malicious — security issues)");
    println!("    - extension conflicts are the #1 Magento operational pain point");
    println!("    - Adobe vetting extensions more aggressively post-acquisition");
    println!("  Customers: ~150,000 active stores (Open Source + Commerce combined)");
    println!("            HP, Nike (some regions), Olympus, Sigma Beauty, Land Rover, Ford (parts), Coca-Cola Bottlers");
    println!("            Helly Hansen, Bulgari, Christian Louboutin, Liverpool FC, Asics");
    println!("            sweet spot: enterprise B2B + complex catalog merchants + brands requiring full source code");
    println!("            historic weakness: SMB (too complex — most SMBs migrated to Shopify 2018-2023)");
    println!("  Critique: complexity tax: requires specialized devs, agencies, hosting partners");
    println!("           hosting costs are enterprise: $5K-$50K+/mo for Commerce Cloud on serious traffic");
    println!("           upgrade pain: 2.4.x version upgrades regularly break extensions");
    println!("           security: high-profile breaches (MageCart card-skimming attacks)");
    println!("           PHP perception: harder to attract young devs vs Node/Python/Go shops");
    println!("           Shopify Plus eating mid-market — Magento market share declining since 2020");
    println!("           Adobe sales motion (long enterprise cycles) doesn't match dev-led merchant adoption");
    println!("  Differentiator: most extensible self-hosted e-commerce + Adobe Experience Cloud bundle + native multi-store + B2B — the platform Fortune 500 brands buy when they need full control + Adobe stack");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "magento".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_magento(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_magento};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/magento"), "magento");
        assert_eq!(basename(r"C:\bin\magento.exe"), "magento.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("magento.exe"), "magento");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_magento(&["--help".to_string()], "magento"), 0);
        assert_eq!(run_magento(&["-h".to_string()], "magento"), 0);
        let _ = run_magento(&["--version".to_string()], "magento");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_magento(&[], "magento");
    }
}
