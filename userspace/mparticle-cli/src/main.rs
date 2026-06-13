#![deny(clippy::all)]

//! mparticle-cli — SlateOS mParticle (enterprise CDP, NYC, acquired by Rokt 2024)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mparticle [OPTIONS]");
        println!("mParticle (Slate OS) — enterprise CDP (acquired by Rokt Jul 2024)");
        println!();
        println!("Options:");
        println!("  --inputs               300+ inputs (mobile/web SDKs + server + feeds)");
        println!("  --outputs              350+ outputs (warehouses, ads, ESPs, BI)");
        println!("  --idsync               IDSync identity resolution (deterministic + probabilistic)");
        println!("  --audiences            Real-time audiences");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mParticle 2024 (Slate OS)"); return 0; }
    println!("mParticle 2024 (Slate OS) — Enterprise CDP");
    println!("  Vendor: mParticle, Inc. (NYC) — ACQUIRED by Rokt Jul 2024 for ~$300-400M");
    println!("  Founders: Michael Katz (CEO) + Andy Katz + Dave Myers, 2013");
    println!("          Michael: ex-AOL/HuffPost mobile exec");
    println!("          founded as mobile-first CDP (vs Segment's web-first origins)");
    println!("          mobile DNA still showed in product depth a decade later");
    println!("  Funding pre-acquisition: ~$300M total");
    println!("         Series E Mar 2022: $150M at ~$1.1B+ valuation (Permira)");
    println!("         Series D Jun 2018: $35M (Bain Capital Ventures)");
    println!("         total of ~$305M raised before exit");
    println!("  Acquisition Jul 2024:");
    println!("         Rokt acquired mParticle for reported ~$300-400M");
    println!("         strategic: Rokt's commerce media + mParticle's CDP = full marketing data stack");
    println!("         Michael Katz transitioned to advisor; ~600 mParticle employees joined Rokt");
    println!("         Rokt is Australian e-commerce-media co (formerly NASDAQ:ROKT trajectory, then private)");
    println!("  Strategic position: 'enterprise CDP with mobile DNA':");
    println!("                    pitch: 'real-time customer data infrastructure for mobile-first brands'");
    println!("                    target: large enterprise + Fortune 500 (sweet spot vs Segment's startup base)");
    println!("                    primary competitor: Segment, Tealium, Adobe Experience Platform, Salesforce CDP");
    println!("                    mParticle's wedge: better mobile SDK depth + enterprise governance + IDSync");
    println!("                    historically closed bigger enterprise deals than Segment");
    println!("  Pricing (enterprise sales-led):");
    println!("    no free tier — minimum ~$50K/yr typical");
    println!("    Enterprise — $100K-$5M+/yr (Fortune 500 deals)");
    println!("    pricing pegged to MAUs (monthly active users) + outputs + data volume");
    println!("    typically more expensive than Segment for large enterprise");
    println!("  Core architecture:");
    println!("    - Inputs: 300+ (deep mobile SDKs, web JS, server REST, batch feeds, cloud sources)");
    println!("    - Outputs: 350+ (analytics, marketing, ads, attribution, BI, warehouses)");
    println!("    - Server-side rules: filter, sample, transform events before forwarding");
    println!("    - Privacy + Consent management built-in (GDPR/CCPA frameworks)");
    println!("    - Data Quality Suite: schema validation, error monitoring");
    println!("  IDSync (identity resolution — the differentiator):");
    println!("    - Deterministic: email, customer_id, phone, MAID, IDFA");
    println!("    - Probabilistic: device fingerprint, IP, behavior matching");
    println!("    - Cross-device + cross-channel + cross-region identity");
    println!("    - Privacy-aware: consent-respecting identity merging");
    println!("    - Considered industry-best for mobile-heavy use cases");
    println!("  Audiences:");
    println!("    - Real-time audience builder (sub-second propagation)");
    println!("    - Conditions: events + properties + computed attributes");
    println!("    - Distribute to ad platforms (Facebook CAPI, Google CAPI, TikTok)");
    println!("    - Distribute to ESPs (Iterable, Braze, Customer.io)");
    println!("    - Sync to warehouses for further analysis");
    println!("  Smart Suggestions (ML):");
    println!("    - AI-powered audience recommendations");
    println!("    - Lookalike audience generation");
    println!("    - Churn prediction signals");
    println!("    - Personalization recommendation engine");
    println!("  Privacy + Consent:");
    println!("    - Consent Management: track + enforce consent across destinations");
    println!("    - Data Subject Requests (GDPR Article 15-17 — access/erasure)");
    println!("    - Regional data residency (EU, US, APAC clusters)");
    println!("    - HIPAA + GDPR + CCPA + LGPD compliance frameworks");
    println!("  mParticle CLI usage:");
    println!("    mparticle login");
    println!("    mparticle events upload --file events.json");
    println!("    mparticle audiences list");
    println!("    mparticle idsync match --email user@example.com");
    println!("    mparticle outputs status");
    println!("  Customers (~700+ paying enterprise):");
    println!("    - NBCUniversal, Spotify, Burger King, JetBlue, Postmates");
    println!("    - Airbnb, Etsy, Overstock, ABC News, Venmo (PayPal)");
    println!("    - Daily Burn, McGraw-Hill, JackThreads");
    println!("    - sweet spot: large enterprise + mobile-first brands");
    println!("    - heavy in: media/streaming, retail, travel, restaurants/QSR");
    println!("  Critique (legacy + acquisition era):");
    println!("           Rokt acquisition: future depends on integration with Rokt's media platform");
    println!("           layoffs in 2023 prior to acquisition (typical post-2022 zirp adjustment)");
    println!("           expensive — locks out mid-market vs Segment/RudderStack alternatives");
    println!("           composable CDP (Hightouch + Snowflake) erodes packaged CDP value");
    println!("           Segment more developer-friendly + larger ecosystem");
    println!("           Adobe + Salesforce CDPs threaten via CRM bundling");
    println!("           mobile-first DNA less unique as Segment built up mobile SDKs");
    println!("  Differentiator: enterprise-grade IDSync identity resolution + deepest mobile SDK suite + real-time audiences + privacy/consent depth + Fortune 500 install base — now backed by Rokt's commerce media for full marketing data stack");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mparticle".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mparticle"), "mparticle");
        assert_eq!(basename(r"C:\bin\mparticle.exe"), "mparticle.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mparticle.exe"), "mparticle");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mp(&["--help".to_string()], "mparticle"), 0);
        assert_eq!(run_mp(&["-h".to_string()], "mparticle"), 0);
        let _ = run_mp(&["--version".to_string()], "mparticle");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mp(&[], "mparticle");
    }
}
