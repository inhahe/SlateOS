#![deny(clippy::all)]

//! shopify-cli — OurOS Shopify (NYSE/TSX:SHOP, the de-facto e-commerce platform)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_shopify(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: shopify [OPTIONS]");
        println!("Shopify (OurOS) — turn-key e-commerce platform powering ~10%+ of US online retail");
        println!();
        println!("Options:");
        println!("  --basic                Basic Shopify — $39/mo");
        println!("  --shopify              Shopify — $105/mo");
        println!("  --advanced             Advanced Shopify — $399/mo");
        println!("  --plus                 Shopify Plus — from $2,300/mo (enterprise)");
        println!("  --starter              Starter — $5/mo (social/link-in-bio)");
        println!("  --pos                  Shopify POS (in-person + omnichannel)");
        println!("  --payments             Shopify Payments + Shop Pay accelerated checkout");
        println!("  --capital              Shopify Capital (merchant cash advance)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Shopify 2024 (OurOS)"); return 0; }
    println!("Shopify 2024 (OurOS)");
    println!("  Vendor: Shopify Inc. (Ottawa, Ontario, Canada — NYSE/TSX:SHOP)");
    println!("  Founders: Tobi Lütke (CEO), Daniel Weinand, Scott Lake, 2006");
    println!("          Lütke is German-born, moved to Ottawa to be with his then-girlfriend (now wife)");
    println!("          built first version of Shopify to sell snowboards (Snowdevil) because existing e-com tools sucked");
    println!("          Snowdevil pivoted into a SaaS platform — Shopify the platform was born");
    println!("          Lütke is a vocal Rails advocate + maintainer + a notable HEY contributor");
    println!("          known for 'no meetings' Mondays + 'crew' culture + opinionated technical leadership");
    println!("  Founded: 2006 in Ottawa — went public 2015 NYSE:SHOP at $17 ($28B+ valuation today)");
    println!("          peaked at ~$176 in 2021 (post-stock-split-adjusted), down to ~$60-110");
    println!("          FY2024 revenue ~$8.9B (+25% YoY), GMV ~$280B+");
    println!("          ~8,000 employees post-2023 layoff (cut ~20% of staff)");
    println!("          GMV ~$280B+ = >10% of US online retail flows through Shopify");
    println!("          one of Canada's most valuable companies (peaked above RBC)");
    println!("  Strategic position: 'arming the rebels' against Amazon — DTC + omnichannel commerce OS:");
    println!("                    primary competitor: BigCommerce (smaller), Wix Stores, Squarespace Commerce");
    println!("                    enterprise: Salesforce Commerce Cloud + Adobe Commerce (ex-Magento)");
    println!("                    headless/composable: commercetools + Saleor + Medusa");
    println!("                    'unified commerce' pitch: online + POS + B2B + wholesale + marketplaces");
    println!("                    2022 strategic pivot: focus on enterprise (Plus) + Shop App consumer side");
    println!("                    2023 divested fulfillment biz (sold Deliverr/6 River to Flexport for stock + 13% Flexport stake)");
    println!("  Pricing (transparent — by tier + transaction rates):");
    println!("    Starter — $5/mo (social link-in-bio + chat checkout, no full storefront)");
    println!("    Basic Shopify — $39/mo (full storefront, 2 staff, basic reports)");
    println!("    Shopify — $105/mo (5 staff, professional reports, gift cards)");
    println!("    Advanced Shopify — $399/mo (15 staff, advanced reports, custom calculated shipping)");
    println!("    Shopify Plus — from $2,300/mo (committed enterprise, dedicated launch eng)");
    println!("       Plus also negotiates custom revenue-share for huge merchants");
    println!("    transaction fees: 0% with Shopify Payments, ~0.5-2% if using third-party gateway");
    println!("    credit card rates: 2.9% + 30¢ (Basic) down to 2.4% + 30¢ (Advanced) for online cards");
    println!("  Shopify Plus (enterprise tier — the growth engine):");
    println!("    - From $2,300/mo committed (most deals $20K-200K+/mo for big brands)");
    println!("    - Wholesale channel + B2B (added 2023)");
    println!("    - Launch engineers + merchant success manager dedicated");
    println!("    - Shopify Functions (Rust-based commerce logic extensions)");
    println!("    - Higher API rate limits + checkout customization (was unique selling point)");
    println!("    - Customers: Allbirds, Gymshark, Mattel, Hasbro, Steve Madden, FIGS, Glossier, Heinz, Nestle, Coca-Cola (D2C lines)");
    println!("  Shopify Payments + Shop Pay:");
    println!("    - Shopify's payment processor (built on Stripe under the hood, white-labeled)");
    println!("    - Shop Pay = accelerated one-click checkout (saves card + address, like Apple Pay)");
    println!("    - Shop Pay reportedly 1.7x higher conversion than guest checkout");
    println!("    - 0% additional transaction fee for using Shopify Payments");
    println!("    - Now available on non-Shopify sites (Facebook/Instagram + Google checkout)");
    println!("  Shop App (consumer-facing):");
    println!("    - 150M+ downloads, ~50M monthly users");
    println!("    - Order tracking + carbon offset + Shop Cash rewards");
    println!("    - 'Shop Mini' apps for embedded merchant experiences");
    println!("    - Direct competitor to: Amazon app, Pinterest discovery");
    println!("  Shopify POS:");
    println!("    - Native iOS/Android POS app for in-person sales");
    println!("    - Shopify POS Go (handheld 5G device) + Shopify Tap+Chip card reader");
    println!("    - Unified inventory across online + in-person");
    println!("    - Competes with Square (Shopify's primary POS competitor)");
    println!("  Shopify Markets + Markets Pro:");
    println!("    - Multi-region selling: currency conversion, localized prices, country-specific URLs");
    println!("    - Markets Pro: Merchant of Record service (Shopify handles duties + taxes + compliance internationally)");
    println!("    - Built on Global-e acquisition partnership initially");
    println!("  Shopify Magic (AI features, 2023+):");
    println!("    - Product description generator");
    println!("    - Sidekick (AI assistant in admin)");
    println!("    - Generate FAQs + email subject lines + image backgrounds");
    println!("    - Powered by various LLMs (OpenAI + own models)");
    println!("  Liquid templating + Theme:");
    println!("    - Liquid (open-source by Shopify) — go-to e-commerce templating language");
    println!("    - Dawn theme = reference theme (Web Components based, no jQuery)");
    println!("    - 100+ free + paid themes in Theme Store");
    println!("    - Customers can use any theme + customize via theme editor");
    println!("    - Hydrogen (React) + Oxygen for headless storefronts");
    println!("  Apps + extensibility:");
    println!("    - 8,000+ apps in App Store (revenue share with developers)");
    println!("    - REST + GraphQL Admin API + Storefront API");
    println!("    - Shopify Functions (Rust + WASM logic extensions running at edge)");
    println!("    - Webhooks + EventBridge integration");
    println!("    - Custom apps via Shopify CLI for dev workflow");
    println!("  Acquisitions:");
    println!("    - Shopify Capital (cash advance), Hydrogen (React), Oxygen (hosting)");
    println!("    - Klaviyo investment ($100M 2022, 11% stake)");
    println!("    - 6 River Systems (acquired 2019, divested to Flexport 2023)");
    println!("    - Deliverr (acquired May 2022 $2.1B, divested to Flexport 2023)");
    println!("    - Remix (React framework, Oct 2022 — keeps Remix team intact + open source)");
    println!("  Customers: 4M+ active stores, 1.75M+ paying monthly subs");
    println!("            Allbirds, Gymshark, Heinz, Mattel, Hasbro, Nestle, Glossier, FIGS, Steve Madden, Decathlon");
    println!("            Lindt, Coca-Cola (D2C), Kylie Cosmetics, Tesla (merch)");
    println!("            sweet spot: from indie maker → $500M+ DTC brand");
    println!("            very weak in: $1B+ retailer with complex omnichannel (those use SFCC or commercetools)");
    println!("  Critique: cost adds up: subscription + 10-20+ paid apps + Plus rev-share + Payments fee");
    println!("           lock-in via Liquid + apps + Shopify Payments — switching is painful");
    println!("           B2B/wholesale lags BigCommerce + Adobe Commerce + commercetools historically");
    println!("           checkout customization restricted on non-Plus (intentional moat)");
    println!("           rate limits (40 req/s REST) frustrating for high-traffic operations");
    println!("           2023 layoff + executive churn shook merchant confidence");
    println!("  Differentiator: best out-of-box DTC commerce experience + Shop Pay + 8K-app ecosystem + Plus enterprise stack — the platform Amazon fears");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "shopify".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_shopify(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_shopify};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/shopify"), "shopify");
        assert_eq!(basename(r"C:\bin\shopify.exe"), "shopify.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("shopify.exe"), "shopify");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_shopify(&["--help".to_string()], "shopify"), 0);
        assert_eq!(run_shopify(&["-h".to_string()], "shopify"), 0);
        assert_eq!(run_shopify(&["--version".to_string()], "shopify"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_shopify(&[], "shopify"), 0);
    }
}
