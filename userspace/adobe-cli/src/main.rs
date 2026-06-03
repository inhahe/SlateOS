#![deny(clippy::all)]

//! adobe-cli — OurOS Adobe Experience Platform (enterprise CDP + experience cloud, San Jose)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_adobe(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: adobe [OPTIONS]");
        println!("Adobe Experience Platform (OurOS) — enterprise CDP + Experience Cloud");
        println!();
        println!("Options:");
        println!("  --aep                  Adobe Experience Platform (CDP foundation)");
        println!("  --rt-cdp               Real-Time CDP (the flagship CDP product)");
        println!("  --analytics            Adobe Analytics (web/app analytics)");
        println!("  --target               Adobe Target (A/B testing + personalization)");
        println!("  --campaign             Adobe Campaign (orchestrated marketing)");
        println!("  --journey-optimizer    Adobe Journey Optimizer (real-time orchestration)");
        println!("  --sensei               Adobe Sensei AI (ML + GenAI Firefly)");
        println!("  --xdm                  Experience Data Model (XDM schema standard)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adobe Experience Platform 2024 (OurOS)"); return 0; }
    println!("Adobe Experience Platform 2024 (OurOS) — Enterprise Experience Cloud");
    println!("  Vendor: Adobe, Inc. (San Jose, CA — NASDAQ:ADBE)");
    println!("  Founded: 1982 by John Warnock + Charles Geschke (PARC veterans)");
    println!("          original product: PostScript (page description language) — desktop publishing revolution");
    println!("          1980s: PostScript + Illustrator + Photoshop");
    println!("          1990s: Acrobat + PDF (open standard 2008) + InDesign");
    println!("          2009: Acquired Omniture for $1.8B — beginning of marketing cloud");
    println!("          2018: Acquired Magento ($1.7B) + Marketo ($4.75B) — full martech stack");
    println!("          2024: Failed Figma acquisition ($20B) blocked by UK + EU regulators (Dec 2023)");
    println!("  Financials (NASDAQ:ADBE):");
    println!("    FY2024 revenue: $21.5B+ (Creative Cloud + Document Cloud + Digital Experience)");
    println!("    Digital Experience segment: ~$5B+ (the experience cloud + marketing cloud)");
    println!("    Market cap: ~$200B+");
    println!("    Operating margin: ~35%+");
    println!("    One of the most profitable software companies");
    println!("  Strategic position: 'the experience-cloud incumbent — bundled CDP + analytics + marketing':");
    println!("                    pitch: 'every customer experience touchpoint, powered by one Adobe stack'");
    println!("                    target: Fortune 500 enterprises with global brand + complex martech");
    println!("                    primary competitor: Salesforce Marketing Cloud + Data Cloud, Oracle, SAP CX");
    println!("                    secondary: pure-play CDPs (Segment, Tealium, mParticle), Bloomreach");
    println!("                    Adobe's wedge: end-to-end suite (creative + content + analytics + CDP + marketing)");
    println!("                    moat: 40+ years brand + Creative Cloud cross-sell + Fortune 500 footprint");
    println!("  Pricing (enterprise sales-led — opaque):");
    println!("    Adobe Analytics — starts ~$30K/yr, large deals $500K-$2M+/yr");
    println!("    Adobe Target — $50K-500K+/yr");
    println!("    Adobe Campaign — $100K-$2M+/yr");
    println!("    Real-Time CDP — $200K-$5M+/yr");
    println!("    Journey Optimizer — $150K-$3M+/yr");
    println!("    Adobe Experience Cloud full bundle: $1M-$20M+/yr (Fortune 500 deals)");
    println!("    very expensive — Adobe sales notorious for 6-12 month enterprise cycles");
    println!("  Experience Cloud products (the marketing/experience stack):");
    println!("    1. Adobe Experience Platform (AEP) — the data foundation");
    println!("       - Schema (XDM) + identity resolution + data lake");
    println!("       - 'real-time customer profile' built on Spark + Iceberg + Azure");
    println!("    2. Real-Time CDP (built on AEP):");
    println!("       - Enterprise CDP — segments, audiences, activation");
    println!("       - Compete head-on with Segment, Tealium, mParticle");
    println!("    3. Adobe Analytics (the legacy core — was Omniture SiteCatalyst):");
    println!("       - Web + mobile + app analytics");
    println!("       - Compete with: Google Analytics 4 (free), Mixpanel, Amplitude");
    println!("    4. Customer Journey Analytics (CJA):");
    println!("       - Cross-channel journey analysis on top of AEP");
    println!("       - Replacing classic Analytics for many customers");
    println!("    5. Adobe Target:");
    println!("       - A/B testing + experimentation + personalization");
    println!("       - Compete with: Optimizely, VWO, Convert, AB Tasty");
    println!("    6. Adobe Campaign:");
    println!("       - Cross-channel campaign orchestration (email, push, SMS, direct mail)");
    println!("       - Acquired Neolane 2013");
    println!("    7. Adobe Journey Optimizer (AJO):");
    println!("       - Real-time omnichannel orchestration (the modern Campaign successor)");
    println!("       - Compete with: Braze, Iterable");
    println!("    8. Marketo Engage (acquired 2018, $4.75B):");
    println!("       - B2B marketing automation");
    println!("    9. Adobe Commerce (Magento, acquired 2018, $1.7B):");
    println!("       - E-commerce platform");
    println!("    10. Adobe Experience Manager (AEM):");
    println!("       - Content management + DAM (digital asset management)");
    println!("       - One of the most-used enterprise CMS platforms");
    println!("    11. Adobe Workfront (acquired 2020, $1.5B):");
    println!("       - Work management for marketing teams");
    println!("    12. Adobe Mix Modeler (2024) — marketing-mix modeling + attribution");
    println!("  Adobe Sensei + GenAI Firefly (2023+):");
    println!("    - Sensei: classic ML/AI for Adobe products (since 2016)");
    println!("    - Firefly: GenAI for image + design generation (2023)");
    println!("    - Firefly Services: programmatic GenAI APIs");
    println!("    - AEM Assets + Photoshop + Illustrator GenAI features");
    println!("    - Compete with: OpenAI DALL-E, Midjourney, Stability AI (Adobe's commercial-safe twist: trained on licensed/owned content)");
    println!("  XDM (Experience Data Model):");
    println!("    - Adobe's customer-data schema standard");
    println!("    - Open source: github.com/adobe/xdm");
    println!("    - Used across AEP, Analytics, Target, AJO");
    println!("    - Adobe's bet to standardize customer data — partial uptake outside Adobe");
    println!("  Adobe CLI (aio):");
    println!("    npm install -g @adobe/aio-cli");
    println!("    aio login");
    println!("    aio cloudmanager list-programs");
    println!("    aio app deploy");
    println!("    aio aem list-templates");
    println!("    aio analytics report --report-suite production");
    println!("  Customers (~95% of Fortune 100):");
    println!("    - Disney, Marriott, Sony, T-Mobile, Walmart, Best Buy, Target, Home Depot");
    println!("    - Lufthansa, HSBC, Volkswagen, Toyota, Pfizer, Nestle, Coca-Cola");
    println!("    - 25K+ enterprise customers across Experience Cloud products");
    println!("    - dominant in Fortune 500 marketing + experience tech stacks");
    println!("  Critique: incredibly expensive — many customers spend $5M-$50M/yr across products");
    println!("           complex to deploy — 6-18 month implementations common");
    println!("           Adobe sales motion notorious for high-pressure + multi-year contracts");
    println!("           AEP technical depth gap vs Snowflake/Databricks for raw data workloads");
    println!("           composable CDP (Hightouch + warehouse) erodes packaged Adobe CDP value");
    println!("           Salesforce Marketing Cloud + Data Cloud competing head-on for big enterprise");
    println!("           Figma blocked acquisition (2023) = strategic blow to design + product strategy");
    println!("           Creative Cloud + AI features face new competition (Canva, Figma, Midjourney)");
    println!("           regulatory scrutiny: EU + UK + US blocked Figma; Acrobat now under EU CSP review");
    println!("  Differentiator: 40+ years brand + Creative Cloud cross-sell + most-complete enterprise experience suite (analytics + CDP + content + marketing + commerce + workflow) + Sensei/Firefly AI + 95% Fortune 100 footprint — the experience-cloud incumbent that owns the enterprise marketer's desktop");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "adobe".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_adobe(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_adobe};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/adobe"), "adobe");
        assert_eq!(basename(r"C:\bin\adobe.exe"), "adobe.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("adobe.exe"), "adobe");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_adobe(&["--help".to_string()], "adobe"), 0);
        assert_eq!(run_adobe(&["-h".to_string()], "adobe"), 0);
        assert_eq!(run_adobe(&["--version".to_string()], "adobe"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_adobe(&[], "adobe"), 0);
    }
}
