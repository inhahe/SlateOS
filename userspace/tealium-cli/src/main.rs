#![deny(clippy::all)]

//! tealium-cli — SlateOS Tealium (CDP + tag management, San Diego, bootstrapped-ish)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tealium(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tealium [OPTIONS]");
        println!("Tealium (Slate OS) — CDP + Tag Management leader, San Diego");
        println!();
        println!("Options:");
        println!("  --iq                   Tealium iQ — tag management (the original product)");
        println!("  --eventstream          EventStream — server-side data layer");
        println!("  --audiencestream       AudienceStream — CDP (audience builder)");
        println!("  --predict              Predict — ML predictive insights");
        println!("  --consent              Consent Manager (privacy + GDPR)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Tealium 2024 (Slate OS)"); return 0; }
    println!("Tealium 2024 (Slate OS) — CDP + Tag Management");
    println!("  Vendor: Tealium, Inc. (San Diego, CA)");
    println!("  Founders: Mike Anderson (CEO until 2024) + Ali Behnam + others, 2008");
    println!("          Mike Anderson: founder of WebSideStory (web analytics, sold to Visual Sciences/Adobe)");
    println!("          Tealium founded post-Visual Sciences exit — Mike's second act");
    println!("          Jeff Lunsford joined as CEO 2024 (ex-Tealium board, Tableau)");
    println!("          original product was Tealium iQ — tag management system (pre-CDP era)");
    println!("          evolved into 'data layer' then full CDP");
    println!("  Funding: ~$192M total");
    println!("         Series F 2020: $96M at ~$1B valuation (Silver Lake, Tibco)");
    println!("         Series E 2017: $30M");
    println!("         earlier: Battery Ventures, Georgian Partners");
    println!("         unicorn at last round; private since");
    println!("  ARR: estimated $100M-200M+ (mature, growing slower)");
    println!("  Strategic position: 'turnkey enterprise CDP — privacy + governance first':");
    println!("                    pitch: 'data privacy, identity, and activation in one trusted platform'");
    println!("                    target: enterprise + regulated industries (financial, healthcare, retail)");
    println!("                    primary competitor: Segment, mParticle, Adobe Experience Platform, Salesforce CDP");
    println!("                    Tealium's wedge: long-standing tag management heritage + consent depth + EU-ready");
    println!("                    more 'IT-friendly' positioning than developer-first Segment");
    println!("                    sales motion: enterprise direct + heavy partner channel");
    println!("  Pricing:");
    println!("    no free tier — enterprise sales-led");
    println!("    Tealium iQ standalone: $30K-100K/yr typical");
    println!("    Full CDP suite: $100K-1M+/yr (Fortune 500 deals common)");
    println!("    pricing pegged to data volume + sites + users");
    println!("  Product portfolio (the suite):");
    println!("    1. Tealium iQ Tag Management:");
    println!("       - The original product — manage all marketing/analytics tags via web UI");
    println!("       - Server-side tagging option (load fewer client tags = faster pages)");
    println!("       - 1,300+ pre-built tag integrations");
    println!("       - Compete with: Google Tag Manager (free), Adobe Launch");
    println!("    2. EventStream API Hub:");
    println!("       - Server-side data collection (vs client-side via iQ)");
    println!("       - Real-time event routing to 1,300+ destinations");
    println!("       - Streaming Hub: persistent event stream");
    println!("    3. AudienceStream CDP:");
    println!("       - Real-time audience builder (sub-second updates)");
    println!("       - 360-degree customer profiles");
    println!("       - Cross-channel audience activation");
    println!("    4. Tealium Predict:");
    println!("       - ML-based propensity scoring (purchase, churn, conversion)");
    println!("       - Auto-generated predictive audiences");
    println!("    5. Consent Manager:");
    println!("       - GDPR + CCPA + LGPD + APPI consent collection");
    println!("       - Cookie management + consent banners");
    println!("       - Compete with: OneTrust, TrustArc, Cookiebot");
    println!("    6. Tealium DataAccess:");
    println!("       - Push customer data to warehouses (Snowflake, BigQuery, Redshift)");
    println!("       - Compete with reverse-ETL (Hightouch, Census)");
    println!("  Integrations (1,300+ destinations — highest in market):");
    println!("    - Analytics: GA4, Adobe Analytics, Mixpanel, Amplitude");
    println!("    - Marketing: Salesforce Marketing Cloud, Oracle Eloqua, Adobe Campaign, Marketo");
    println!("    - Ads: Facebook, Google, TikTok, LinkedIn, Microsoft Ads, The Trade Desk");
    println!("    - CRM: Salesforce, Dynamics, HubSpot, SAP CX");
    println!("    - Warehouses: Snowflake, BigQuery, Redshift, Synapse, Databricks");
    println!("    - Personalization: Adobe Target, Optimizely, Dynamic Yield");
    println!("    - Support: Zendesk, Genesys, NICE inContact");
    println!("  Tealium CLI usage:");
    println!("    tealium login");
    println!("    tealium iq profile list");
    println!("    tealium audiences create --name 'high-value-customers' --rules ...");
    println!("    tealium eventstream publish --topic orders --data '...'");
    println!("    tealium consent status --visitor-id v-123");
    println!("  Customers (~850+ paying enterprise):");
    println!("    - Hertz, Lufthansa, IHG (Intercontinental Hotels), United Airlines");
    println!("    - Bank of America, T-Mobile, BMW, Subaru, Cathay Pacific");
    println!("    - Mercedes-Benz, Wells Fargo, Estée Lauder, Lego");
    println!("    - sweet spot: large enterprise — financial services, travel/hospitality, retail, automotive");
    println!("    - heavily international (strong EU + APAC presence vs Segment's US tilt)");
    println!("  Critique: complex platform — long onboarding (weeks-months)");
    println!("           UX dated vs Segment / RudderStack");
    println!("           expensive — minimum 6-figure ACV");
    println!("           tag management heritage = legacy positioning to some buyers");
    println!("           composable CDP (Hightouch + warehouse) erodes packaged CDP value");
    println!("           Adobe + Salesforce CDPs threaten via CRM bundling");
    println!("           growth slowing — late-stage CDP market consolidation");
    println!("           CEO change 2024 = uncertainty in product direction");
    println!("  Differentiator: 1,300+ destination catalog (largest in market) + tag management heritage + enterprise consent depth + AudienceStream real-time + strong international presence — the enterprise CDP choice for regulated industries and global brands");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tealium".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tealium(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tealium};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tealium"), "tealium");
        assert_eq!(basename(r"C:\bin\tealium.exe"), "tealium.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tealium.exe"), "tealium");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tealium(&["--help".to_string()], "tealium"), 0);
        assert_eq!(run_tealium(&["-h".to_string()], "tealium"), 0);
        let _ = run_tealium(&["--version".to_string()], "tealium");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tealium(&[], "tealium");
    }
}
