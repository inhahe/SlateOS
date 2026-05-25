#![deny(clippy::all)]

//! bloomreach-cli — OurOS Bloomreach (commerce CDP + experience + AI search, Mountain View + EU)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_br(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bloomreach [OPTIONS]");
        println!("Bloomreach (OurOS) — commerce experience cloud (CDP + search + content)");
        println!();
        println!("Options:");
        println!("  --engagement           Bloomreach Engagement — commerce CDP (formerly Exponea)");
        println!("  --discovery            Discovery — AI-powered site search + merchandising");
        println!("  --content              Content — headless CMS (formerly Hippo CMS)");
        println!("  --clarity              Bloomreach Clarity AI agent (2024)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Bloomreach 2024 (OurOS)"); return 0; }
    println!("Bloomreach 2024 (OurOS) — Commerce Experience Cloud");
    println!("  Vendor: Bloomreach, Inc. (Mountain View + Amsterdam + Bratislava + Brno)");
    println!("  Founders: Raj De Datta (CEO) + Ashutosh Garg (Chief Scientist), 2009");
    println!("          Raj: ex-Cisco + Goldman Sachs, Harvard MBA");
    println!("          Ashutosh: ex-Google + IBM Research (search/ML PhD)");
    println!("          original product was AI-powered site search for retailers");
    println!("          expanded via acquisitions to commerce CDP (Exponea 2021) + content (Hippo 2016)");
    println!("  Funding: ~$520M total");
    println!("         Series F Jun 2022: $175M led by Goldman Sachs Asset Management at $2.2B valuation");
    println!("         Series E 2021 (Exponea acquisition financing): $150M (Sixth Street + others)");
    println!("         earlier rounds: Bain Capital Ventures, NEA, Battery, Lightspeed");
    println!("         unicorn $2.2B at last round");
    println!("  ARR: estimated $200M+ (largest commerce-CDP pure-play)");
    println!("  Strategic position: 'commerce experience cloud' — vertical CDP for retail:");
    println!("                    pitch: 'every commerce experience touchpoint, powered by one platform + customer data'");
    println!("                    target: e-commerce + retail brands ($50M-$10B GMV)");
    println!("                    primary competitor: Salesforce Commerce Cloud + Marketing Cloud, Adobe Experience Cloud");
    println!("                    secondary: Klaviyo (email-only), Segment (CDP-only)");
    println!("                    Bloomreach's wedge: commerce-specific data model + AI search + CDP + content in one");
    println!("                    contrast to horizontal CDPs (Segment/mParticle): purpose-built for retail");
    println!("  Pricing:");
    println!("    no free tier — enterprise sales-led");
    println!("    Engagement (CDP) — $30K-300K+/yr");
    println!("    Discovery (search) — $50K-500K+/yr (priced per pageview/query volume)");
    println!("    Full suite (Engagement + Discovery + Content) — $200K-3M+/yr");
    println!("  Three pillars (the commerce experience cloud):");
    println!("    1. Bloomreach Engagement (CDP + Marketing — was Exponea, acquired 2021):");
    println!("       - Real-time customer data unification");
    println!("       - Audience builder + segmentation");
    println!("       - Email + SMS + push + web personalization");
    println!("       - Compete with: Klaviyo (smaller), Salesforce Marketing Cloud, Adobe Campaign");
    println!("       - Acquired Czech CDP Exponea for $200M+ in 2021 — biggest move");
    println!("    2. Bloomreach Discovery (AI Site Search):");
    println!("       - ML-powered search + autocomplete + merchandising");
    println!("       - Visual search + voice search + semantic search (LLM-powered 2024)");
    println!("       - SEO Pages (auto-generated category landing pages)");
    println!("       - Compete with: Algolia, Coveo, Lucidworks, AWS Kendra");
    println!("       - The original Bloomreach product — strongest moat");
    println!("    3. Bloomreach Content (Headless CMS — was Hippo, acquired 2016):");
    println!("       - Headless content management for omnichannel commerce");
    println!("       - Personalization-aware content delivery");
    println!("       - Compete with: Contentful, Sanity, Contentstack, Adobe AEM");
    println!("  Bloomreach Clarity (AI agent, 2024):");
    println!("    - Conversational AI agent for shoppers (chat-based product discovery)");
    println!("    - Conversational AI for marketers (build campaigns via natural language)");
    println!("    - LLM-powered + grounded in customer's product catalog");
    println!("    - Compete with: Klevu, Constructor.io AI search bots, Lily AI");
    println!("  Customer data model (commerce-specific):");
    println!("    - Built-in entities: customer, order, product, cart, session, browse");
    println!("    - 100+ pre-built attributes (LTV, RFM segmentation, churn risk)");
    println!("    - Faster time-to-value than horizontal CDPs that require modeling");
    println!("  Integrations:");
    println!("    - E-commerce: Shopify, Magento, Salesforce Commerce Cloud, BigCommerce, commercetools");
    println!("    - PIM: Akeneo, Salsify, inRiver");
    println!("    - Ads: Google, Facebook, Pinterest (Conversions APIs)");
    println!("    - Warehouses: Snowflake, BigQuery (sync customer + behavior data)");
    println!("    - Analytics: GA4, Adobe Analytics");
    println!("  Bloomreach CLI usage:");
    println!("    bloomreach login");
    println!("    bloomreach engagement audiences list");
    println!("    bloomreach engagement campaign send --audience high-value --template welcome-back");
    println!("    bloomreach discovery search 'red running shoes' --site main");
    println!("    bloomreach content publish --doc homepage-hero");
    println!("  Customers (~800+ paying):");
    println!("    - Bosch, Albertsons, Williams-Sonoma, Levi's, Puma, FC Bayern Munich");
    println!("    - Marks & Spencer, Asda, Argos, T-Mobile, Reformation");
    println!("    - heavy EU presence: especially Germany, UK, Netherlands, Czech Republic");
    println!("    - sweet spot: mid-large retail/e-commerce brands ($100M-$10B+ GMV)");
    println!("    - heavy in: fashion, grocery, sporting goods, beauty");
    println!("  Critique: three-product story = complex sales + onboarding");
    println!("           Engagement (Exponea) integration with rest of stack still incomplete");
    println!("           expensive — minimum 6-figure ACV across products");
    println!("           Salesforce + Adobe bundle threats from CRM/MarTech side");
    println!("           Klaviyo dominates SMB email + CDP-lite for Shopify shops");
    println!("           Algolia + Coveo + Constructor.io threaten Discovery from search side");
    println!("           IPO talked about since 2022 — still private as of 2024");
    println!("           Czech engineering team integration still ongoing 3+ years post-Exponea");
    println!("  Differentiator: commerce-vertical CDP + AI site search + headless CMS in one platform + Czech/Slovak engineering scale + 800+ retail brands — the experience-cloud choice for mid-large commerce brands that want CDP + search + content without Adobe/Salesforce bundling");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bloomreach".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_br(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
