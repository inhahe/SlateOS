#![deny(clippy::all)]

//! jitterbit-cli — OurOS Jitterbit (Harmony iPaaS + API mgmt, Alameda CA, PE-owned)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_jitterbit(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jitterbit [OPTIONS]");
        println!("Jitterbit (OurOS) — Harmony iPaaS + Vinyl + EDI (PE-owned, Alameda CA)");
        println!();
        println!("Options:");
        println!("  --harmony              Harmony (the core iPaaS)");
        println!("  --api-manager          Harmony API Manager");
        println!("  --vinyl                Vinyl (low-code app builder)");
        println!("  --edi                  EDI Integration");
        println!("  --jitterbit-ai         Jitterbit AI (NLP-driven integration assistant)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Jitterbit Harmony 2024 (OurOS)"); return 0; }
    println!("Jitterbit 2024 (OurOS) — Harmony Integration Platform");
    println!("  Vendor: Jitterbit, Inc. (Alameda, CA — PE-owned by KKR + Vista since 2019)");
    println!("  Founders: Sharam Sasson + Ilan Sehayek, 2003");
    println!("          early data integration vendor — initially on-prem ETL");
    println!("          pivoted to cloud iPaaS in mid-2010s");
    println!("          Bill Conner: CEO (since 2022 — security-industry veteran from SonicWall)");
    println!("          George Gallegos: long-time CEO before Conner");
    println!("  Corporate history:");
    println!("         Acquired by KKR + Vista Equity Partners 2019 (terms undisclosed)");
    println!("         As of 2024 owned by Vista Equity Partners (PE)");
    println!("         private + PE-managed for growth + margin expansion");
    println!("         estimated $100-150M ARR (private)");
    println!("  Strategic position: 'unified API + integration + low-code platform for mid-market':");
    println!("                    pitch: 'integrate, automate, orchestrate — one platform for it all'");
    println!("                    target: mid-market + enterprise (especially Salesforce/NetSuite shops)");
    println!("                    primary competitor: Boomi, Workato, MuleSoft (high-end), Celigo (NetSuite)");
    println!("                    secondary: Jitterbit Vinyl competes with OutSystems, Mendix, Microsoft Power Apps");
    println!("                    Jitterbit's wedge: combined iPaaS + low-code + API mgmt + EDI in one platform");
    println!("                    PE-driven 'platform play' — bolt-on acquisitions over time");
    println!("  Pricing:");
    println!("    Standard Edition: $35K-$75K/yr");
    println!("    Professional: $75K-$200K/yr");
    println!("    Enterprise: $200K-$1M+/yr");
    println!("    Vinyl (low-code): $50K-$500K+/yr per app");
    println!("    typically priced below MuleSoft, competitive with Boomi");
    println!("  Product portfolio:");
    println!("    1. Jitterbit Harmony (the iPaaS):");
    println!("       - Cloud integration platform + on-prem agents");
    println!("       - Visual designer (Cloud Studio)");
    println!("       - 300+ pre-built connectors");
    println!("       - Real-time + batch + event-driven flows");
    println!("    2. Harmony API Manager:");
    println!("       - API lifecycle, gateway, design");
    println!("       - Compete with: Kong, Apigee, MuleSoft API Mgr");
    println!("    3. Jitterbit Vinyl (low-code app builder, acquired 2022):");
    println!("       - Acquired from eBuilder Solutions / Vinyl");
    println!("       - Web + mobile app builder");
    println!("       - Compete with: OutSystems, Mendix, Microsoft Power Apps");
    println!("    4. EDI Integration:");
    println!("       - X12, EDIFACT, RosettaNet, AS2");
    println!("       - Trading partner mgmt");
    println!("       - Compete with: IBM Sterling, Cleo, OpenText");
    println!("    5. Jitterbit AI (2023):");
    println!("       - NLP-driven integration assistant");
    println!("       - 'Describe what you want' → suggested mappings + flow steps");
    println!("    6. Jitterbit Marketplace:");
    println!("       - Pre-built process templates");
    println!("       - Industry-specific recipes (manufacturing, retail, healthcare)");
    println!("    7. Citizen Integrator program:");
    println!("       - Lighter-weight UX for business users");
    println!("       - Approval workflows for IT governance");
    println!("    8. Harmony Cloud Agent + Private Agent:");
    println!("       - Cloud-hosted runtime (default)");
    println!("       - Self-hosted Private Agent for on-prem + air-gapped scenarios");
    println!("  Vinyl integration (post-acquisition strategy):");
    println!("    - Combined low-code + iPaaS = differentiator");
    println!("    - 'Build the app + integrate the data' in one platform");
    println!("    - Strategy: own the workflow from UI to backend integration");
    println!("    - Compete with: OutSystems + MuleSoft combo, ServiceNow App Engine + Integration");
    println!("  Integrations (300+ connectors):");
    println!("    - Salesforce, NetSuite, SAP, Oracle EBS, Microsoft Dynamics, Workday");
    println!("    - HCM: BambooHR, ADP, UKG, SAP SuccessFactors");
    println!("    - CRM: Salesforce (deep), HubSpot, Microsoft Dynamics");
    println!("    - Database: Oracle, SQL Server, PostgreSQL, MySQL, Snowflake, Redshift");
    println!("    - Cloud: AWS, Azure, GCP services");
    println!("    - Marketing: Marketo, Eloqua, Mailchimp, HubSpot");
    println!("    - Storage: SFTP, FTP, S3, Azure Blob, GCS, Box, Dropbox");
    println!("    - EDI: X12 standards, EDIFACT subsets, AS2");
    println!("    - Industry-specific: Epicor, Plex, Infor, SAGE for manufacturing");
    println!("  Jitterbit CLI usage:");
    println!("    jitterbit login --org my-workspace");
    println!("    jitterbit project list --env production");
    println!("    jitterbit operation deploy --operation-id ABC123 --env prod");
    println!("    jitterbit agent install --type private --org my-workspace");
    println!("    jitterbit api deploy --api-name Orders-v2 --env prod");
    println!("    jitterbit vinyl app deploy --app-id ABC123");
    println!("    jitterbit edi setup --partner WALMART --transaction 850");
    println!("  Customers (~5,000+):");
    println!("    - Heavy in: manufacturing, retail, distribution, healthcare");
    println!("    - Sweet spot: $50M-$2B revenue mid-market");
    println!("    - Salesforce ecosystem strong");
    println!("    - International: significant EMEA + APAC presence");
    println!("    - Major: NHS, Cisco, Tata Steel, Sumitomo, Bose");
    println!("  Critique: PE-owned trajectory = margin focus over innovation pace");
    println!("           low-code (Vinyl) acquisition still being integrated — go-to-market overlap unclear");
    println!("           AI features behind MuleSoft/Workato in capabilities");
    println!("           connector count (300) below Workato (1,000) and Zapier (7,000)");
    println!("           brand awareness lower than Boomi/MuleSoft in marquee enterprise deals");
    println!("           private + PE ownership = unclear IPO path");
    println!("           Citizen Integrator program competes with simpler iPaaS (Zapier, Make) — unclear positioning");
    println!("  Differentiator: combined iPaaS + low-code (Vinyl) + API mgmt + EDI in single platform — rare across competitors + 5K+ customers heavy in mid-market manufacturing/retail/distribution + PE-owned by Vista (capital + M&A capacity) + Salesforce ecosystem strength + Harmony Private Agent for hybrid deployments — the consolidated 'one platform for integration + apps + APIs + EDI' iPaaS for mid-market companies that don't want 4 separate vendors");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "jitterbit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_jitterbit(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_jitterbit};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/jitterbit"), "jitterbit");
        assert_eq!(basename(r"C:\bin\jitterbit.exe"), "jitterbit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("jitterbit.exe"), "jitterbit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_jitterbit(&["--help".to_string()], "jitterbit"), 0);
        assert_eq!(run_jitterbit(&["-h".to_string()], "jitterbit"), 0);
        assert_eq!(run_jitterbit(&["--version".to_string()], "jitterbit"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_jitterbit(&[], "jitterbit"), 0);
    }
}
