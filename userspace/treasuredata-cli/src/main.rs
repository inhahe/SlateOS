#![deny(clippy::all)]

//! treasuredata-cli — SlateOS Treasure Data (enterprise CDP, MV + Tokyo, Arm-owned then SoftBank)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_td(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: td [OPTIONS]");
        println!("Treasure Data (SlateOS) — enterprise CDP (Japan-rooted, global)");
        println!();
        println!("Options:");
        println!("  --plazma               Plazma — proprietary columnar query engine");
        println!("  --fluentd              Fluentd-based ingestion (Treasure Data created Fluentd)");
        println!("  --cdp                  Customer Data Platform (audiences, identity, activations)");
        println!("  --ml                   Treasure Insights ML auto-modeling");
        println!("  --dataops              Workflows (Digdag) + scheduled queries");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Treasure Data 2024 (SlateOS)"); return 0; }
    println!("Treasure Data 2024 (SlateOS) — Enterprise CDP");
    println!("  Vendor: Treasure Data, Inc. (Mountain View + Tokyo)");
    println!("  Founders: Hiro Yoshikawa (former CEO) + Kazuki Ohta (CTO) + Sadayuki Furuhashi, 2011");
    println!("          all three Japanese — founded in Tokyo, moved HQ to Bay Area");
    println!("          Kazuki + Sadayuki: created Fluentd (open-source log collector, CNCF graduated)");
    println!("          MessagePack co-creators (efficient binary serialization format)");
    println!("          one of the few Japan-rooted enterprise data companies with global presence");
    println!("  Ownership history:");
    println!("         Acquired by Arm Holdings Aug 2018 for ~$600M (Arm wanted IoT data platform)");
    println!("         When SoftBank sold Arm to Nvidia attempt failed (2020-2022), TD was spun out");
    println!("         Spun off back to SoftBank-led group Sep 2018 — actually back to Arm/SoftBank for some years");
    println!("         Since ~2023: independent again, majority-owned by SoftBank Vision Fund");
    println!("         complicated ownership history but consistently independent operations");
    println!("  Strategic position: 'enterprise CDP — Japanese engineering rigor':");
    println!("                    pitch: 'unified customer data + AI activations + 170+ pre-built integrations'");
    println!("                    target: large global enterprises (Fortune 500 + Japan/APAC dominant)");
    println!("                    primary competitor: Segment, mParticle, Tealium, Adobe Experience Platform");
    println!("                    TD's wedge: Japanese enterprise dominance + ML auto-modeling + Plazma engine");
    println!("                    sales motion: enterprise direct + heavy SI channel (Accenture, Dentsu, Hakuhodo)");
    println!("                    nearly every major Japanese brand uses TD");
    println!("  Pricing:");
    println!("    no free tier — enterprise sales-led");
    println!("    Standard — $50K-200K/yr");
    println!("    Enterprise — $200K-$5M+/yr (Fortune 500 + large Japan deals)");
    println!("    pricing pegged to records ingested + queries + activations");
    println!("  Plazma engine (proprietary columnar storage + query):");
    println!("    - Custom columnar DB built for time-series customer events");
    println!("    - Trillion-row scale per customer");
    println!("    - Hive-compatible SQL + Presto-compatible Hivemall ML integration");
    println!("    - Petabyte-scale tested at Japanese telco + financial customers");
    println!("    - Predates Snowflake/BigQuery — TD's own bet on columnar");
    println!("  Fluentd (open-source heritage):");
    println!("    - TD founders created Fluentd in 2011 — now CNCF-graduated project");
    println!("    - 10K+ GitHub stars; default in EFK stack (Elasticsearch + Fluentd + Kibana)");
    println!("    - TD CDP uses Fluentd as primary event ingestion path");
    println!("    - Massive credibility in data-engineering communities");
    println!("  Digdag (workflow engine, OSS):");
    println!("    - TD-created workflow orchestrator (think Airflow + dbt + scheduler)");
    println!("    - Used internally + by some external teams");
    println!("    - YAML-based DAG definitions");
    println!("    - Pre-dates Airflow's dominance");
    println!("  Customer Data Platform features:");
    println!("    - Audience builder + segmentation");
    println!("    - Identity resolution (deterministic + probabilistic)");
    println!("    - Real-time activation to ads + marketing channels");
    println!("    - 360-degree customer profiles");
    println!("    - Privacy + Consent management (GDPR, APPI Japan, CCPA)");
    println!("  Treasure Insights (ML auto-modeling):");
    println!("    - Auto-generated predictive models (purchase propensity, churn, LTV)");
    println!("    - Hivemall integration (machine learning in SQL)");
    println!("    - Looker-style dashboards for marketing teams");
    println!("    - Compete with Adobe Customer Journey Analytics + Salesforce Einstein");
    println!("  Integrations (170+):");
    println!("    - Heavy Japan-specific: LINE, Rakuten, Yahoo Japan, NTT, KDDI integrations");
    println!("    - Global: Salesforce, Adobe, Marketo, Iterable, Braze, Facebook, Google");
    println!("    - Warehouses: Snowflake, BigQuery, Redshift (export from TD to these)");
    println!("    - Analytics: GA4, Adobe Analytics, Mixpanel, Amplitude");
    println!("    - Ads: Google, Facebook, TikTok, LINE Ads, Yahoo Japan Ads, Criteo");
    println!("  Treasure Data CLI usage:");
    println!("    td account create");
    println!("    td db:create marketing_db");
    println!("    td table:create marketing_db events");
    println!("    td query -d marketing_db 'SELECT count(*) FROM events'");
    println!("    td workflow:push my_pipeline");
    println!("    td connector:guess seed.yml -o load.yml");
    println!("  Customers (~600+ paying enterprise):");
    println!("    - Subaru, Mazda, Nissan, Toyota (most major Japan auto)");
    println!("    - Muji, Uniqlo, Pokémon Company, Capcom (Japan retail + gaming)");
    println!("    - Mitsubishi, Sumitomo, Mitsui (Japan conglomerates)");
    println!("    - Globally: AB InBev, Wish, AccorHotels, Yum! Brands, Mizuno");
    println!("    - sweet spot: large enterprise — automotive, retail, financial services, telco");
    println!("    - 60%+ of Japan's Nikkei 225 use TD");
    println!("  Critique: less brand recognition in US/EU than Segment/mParticle/Tealium");
    println!("           Plazma proprietary engine adds onboarding complexity (vs warehouse-native)");
    println!("           UX less polished than newer CDPs (Hightouch, RudderStack)");
    println!("           expensive — minimum 6-figure ACV");
    println!("           ownership history confusion hurt enterprise sales conversations");
    println!("           Snowflake-native CDPs (Hightouch) attacking from composable angle");
    println!("           dependence on Japanese SI channel slows global expansion");
    println!("           open-source projects (Fluentd, Digdag) reach > commercial product");
    println!("  Differentiator: Fluentd-creator engineering pedigree + Plazma trillion-row engine + dominant Japan/APAC enterprise footprint + 170+ pre-built integrations including Japan-specific platforms — the CDP choice for global enterprises with significant APAC presence");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "treasuredata".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_td(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_td};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/treasuredata"), "treasuredata");
        assert_eq!(basename(r"C:\bin\treasuredata.exe"), "treasuredata.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("treasuredata.exe"), "treasuredata");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_td(&["--help".to_string()], "treasuredata"), 0);
        assert_eq!(run_td(&["-h".to_string()], "treasuredata"), 0);
        let _ = run_td(&["--version".to_string()], "treasuredata");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_td(&[], "treasuredata");
    }
}
