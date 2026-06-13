#![deny(clippy::all)]

//! bigcommerce-cli — SlateOS BigCommerce (NASDAQ:BIGC, open-platform e-commerce)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bigcommerce [OPTIONS]");
        println!("BigCommerce (SlateOS) — open SaaS e-commerce platform (Shopify alternative)");
        println!();
        println!("Options:");
        println!("  --standard             Standard — $39/mo");
        println!("  --plus                 Plus — $105/mo");
        println!("  --pro                  Pro — $399/mo");
        println!("  --enterprise           Enterprise — custom (Shopify Plus alternative)");
        println!("  --b2b                  BigCommerce B2B Edition (B2B + wholesale)");
        println!("  --multistorefront      Multi-Storefront (multiple brands one backend)");
        println!("  --headless             Headless commerce + Catalyst (Next.js storefront)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("BigCommerce 2024 (SlateOS)"); return 0; }
    println!("BigCommerce 2024 (SlateOS)");
    println!("  Vendor: BigCommerce Holdings, Inc. (Austin, TX — NASDAQ:BIGC)");
    println!("  Founders: Eddie Machaalani + Mitchell Harper, 2009");
    println!("          both Australian — built original BigCommerce in Sydney before relocating to Austin");
    println!("          'opposite of Shopify' positioning from very early days: open APIs, no lock-in");
    println!("  Founded: 2009 in Sydney, then HQ to Austin TX");
    println!("          IPO Aug 2020 NASDAQ:BIGC at $24 (peaked ~$160 in early 2021)");
    println!("          now ~$5-7, market cap ~$400M (rough years 2022-2024)");
    println!("          FY2024 revenue ~$330M (low single-digit growth)");
    println!("          ~1,200 employees post-2023 layoffs");
    println!("  Strategic position: 'open SaaS' — the headless-friendly Shopify alternative:");
    println!("                    primary head-to-head competitor: Shopify (much larger)");
    println!("                    enterprise: Salesforce Commerce Cloud, Adobe Commerce");
    println!("                    headless/composable: commercetools (more enterprise), Saleor, Medusa, Vue Storefront");
    println!("                    differentiator: no transaction fees ever, fewer locked-down APIs, multi-storefront from one backend");
    println!("                    pitch: 'composable' commerce for mid-market + enterprise");
    println!("  Pricing (transparent, simpler than Shopify):");
    println!("    Standard — $39/mo (sales threshold $50K/yr)");
    println!("    Plus — $105/mo ($180K/yr threshold)");
    println!("    Pro — $399/mo ($400K/yr threshold)");
    println!("    Enterprise — custom (typically $1K-$30K+/mo committed)");
    println!("    NO transaction fees regardless of payment processor used (vs Shopify's 0.5-2%)");
    println!("    online sales caps per tier — go over, you auto-upgrade");
    println!("    Multi-storefront pricing: typically Enterprise-only add-on");
    println!("  Open APIs + extensibility (the big differentiator):");
    println!("    - REST + GraphQL Storefront API (no rate limits as restrictive as Shopify)");
    println!("    - 'Open SaaS' philosophy: fewer restrictions on checkout customization vs Shopify");
    println!("    - Storefront fully customizable on Standard tier (Shopify gates this to Plus)");
    println!("    - Webhooks for orders, inventory, customer, abandoned cart, etc.");
    println!("    - Stencil theme framework (Handlebars-based templating)");
    println!("    - Page Builder visual editor");
    println!("    - 1,200+ apps in marketplace (much smaller than Shopify's 8,000+)");
    println!("  Catalyst (BigCommerce's reference Next.js storefront, 2023+):");
    println!("    - Open-source React storefront on Next.js");
    println!("    - Combines BigCommerce + Makeswift (visual page builder) + GraphQL");
    println!("    - BigCommerce's answer to Shopify Hydrogen");
    println!("    - Goal: catch the headless wave that's growing among mid-market merchants");
    println!("  Multi-Storefront (key wedge vs Shopify):");
    println!("    - Run multiple storefronts/brands from one BigCommerce account");
    println!("    - Shared inventory + customers + reporting across storefronts");
    println!("    - Shopify requires separate Shopify Plus shops + custom dev to unify");
    println!("    - Powerful for: multi-brand holdcos, geographic expansion, B2B + B2C split");
    println!("  B2B Edition (acquired BundleB2B 2022 to build this):");
    println!("    - Company accounts + corporate hierarchies + buyer roles");
    println!("    - Quote-to-cash workflows");
    println!("    - Custom pricing per company + tiered pricing");
    println!("    - Punchout catalogs (CXML, OCI integration with SAP Ariba, Coupa, Oracle)");
    println!("    - This is BigCommerce's strongest 'beats Shopify' moat — Plus B2B is newer");
    println!("  Payment processing:");
    println!("    - Use any processor: Stripe, PayPal, Braintree, Adyen, Klarna, Affirm, Authorize.net");
    println!("    - 100+ payment gateways supported");
    println!("    - No proprietary 'BigCommerce Payments' that locks merchants in");
    println!("  Channel integrations:");
    println!("    - Amazon (BigCommerce was first SaaS platform with deep Amazon channel)");
    println!("    - eBay, Walmart Marketplace, Etsy, Google Shopping, Meta, TikTok");
    println!("    - Channel manager built-in (Shopify acquired Channable + Shop App for this)");
    println!("    - POS via Square integration (no first-party POS unlike Shopify)");
    println!("  Customers: ~45,000 stores, ~6,000 enterprise ($600K+ GMV)");
    println!("            Ben & Jerry's, Skullcandy, Toyota, Burrow, Solo Stove, SC Johnson, Yeti (B2B)");
    println!("            sweet spot: B2B brands, multi-brand holdcos, mid-market wanting headless flexibility");
    println!("            very weak in: indie maker/SMB (Shopify dominates here)");
    println!("  Critique: smaller app/theme ecosystem than Shopify (1,200 vs 8,000+ apps, 100 vs 1,000+ themes)");
    println!("           growth has stalled — single-digit % YoY while Shopify still 25%+");
    println!("           brand recognition far weaker than Shopify outside enterprise B2B circles");
    println!("           stock down ~90% from peak, raising concerns about long-term independence");
    println!("           rumors of take-private/acquisition perennial (no deal announced)");
    println!("           UI/admin feels dated vs Shopify's polish");
    println!("           Catalyst + Page Builder + Makeswift integration still maturing");
    println!("  Differentiator: open APIs, no transaction fees, native B2B + multi-storefront, headless-friendly — the platform mid-market enterprises pick over Shopify Plus");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bigcommerce".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bigcommerce"), "bigcommerce");
        assert_eq!(basename(r"C:\bin\bigcommerce.exe"), "bigcommerce.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bigcommerce.exe"), "bigcommerce");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bc(&["--help".to_string()], "bigcommerce"), 0);
        assert_eq!(run_bc(&["-h".to_string()], "bigcommerce"), 0);
        let _ = run_bc(&["--version".to_string()], "bigcommerce");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bc(&[], "bigcommerce");
    }
}
