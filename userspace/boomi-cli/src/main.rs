#![deny(clippy::all)]

//! boomi-cli — OurOS Boomi (iPaaS pioneer, Conshohocken PA, ex-Dell, now PE-owned)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_boomi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: boomi [OPTIONS]");
        println!("Boomi (OurOS) — AtomSphere iPaaS (was Dell Boomi)");
        println!();
        println!("Options:");
        println!("  --atomsphere           AtomSphere Integration (the flagship iPaaS)");
        println!("  --flow                 Boomi Flow (workflow + low-code app builder)");
        println!("  --master-data-hub      Master Data Hub (MDM)");
        println!("  --api-management       Boomi API Management (gateway + lifecycle)");
        println!("  --b2b-edi              B2B/EDI Management");
        println!("  --boomi-ai             Boomi AI (generative integration design)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Boomi 2024 (OurOS) — AtomSphere"); return 0; }
    println!("Boomi 2024 (OurOS) — AtomSphere iPaaS");
    println!("  Vendor: Boomi, LP (Conshohocken, PA — private after 2021 Dell carve-out)");
    println!("  Founder: Rick Nucci, 2000 (in basement-startup style)");
    println!("          early SaaS-delivered integration platform — pioneered the iPaaS category");
    println!("          'Atom' = lightweight runtime concept — could run in cloud, on-prem, or hybrid");
    println!("          Steve Lucas: current CEO (since 2023, came from Marketo + iCIMS)");
    println!("          Chris McNabb: long-time CEO (2018-2023)");
    println!("  Corporate history:");
    println!("         Acquired by Dell Nov 2010 (terms not disclosed, ~$70-100M est)");
    println!("         Dell rebranded as 'Dell Boomi' from 2010-2020");
    println!("         Dell sold Boomi May 2021 to Francisco Partners + TPG for $4B");
    println!("         post-spinout: focused growth + reorganized");
    println!("         possible IPO 2024-2025 path");
    println!("         revenue: ~$300M+ ARR (private, estimated)");
    println!("  Strategic position: 'connect everything — pioneer of cloud-native iPaaS':");
    println!("                    pitch: 'the leading iPaaS for AnyApp connectivity — cloud, on-prem, edge'");
    println!("                    target: mid-market + enterprise (broadest base of any iPaaS)");
    println!("                    primary competitor: MuleSoft, Workato, Microsoft Power Automate, Informatica");
    println!("                    secondary: Celigo, Jitterbit, Tray.io, SnapLogic, IBM webMethods");
    println!("                    Boomi's wedge: 24-year history + Atom architecture + broadest install base in iPaaS");
    println!("                    'pioneer' positioning (since 2000) vs newer challengers like Workato");
    println!("  Pricing:");
    println!("    Professional plan: $50K-$200K/yr (starting tier)");
    println!("    Enterprise plan: $150K-$1M+/yr");
    println!("    Enterprise Plus: $500K-$3M+/yr (high-volume + API + MDM bundle)");
    println!("    Per-connection pricing + per-Atom + per-process — somewhat complex");
    println!("    typically priced below MuleSoft (the 'value enterprise iPaaS' position)");
    println!("  Product portfolio (Boomi Platform):");
    println!("    1. AtomSphere Integration (the flagship):");
    println!("       - Visual integration designer (drag-and-drop)");
    println!("       - 200+ connectors (Salesforce, NetSuite, Workday, SAP, etc.)");
    println!("       - Cloud + on-prem deployment via Atoms");
    println!("       - 'Suggest' AI/ML features for mapping recommendations");
    println!("    2. Boomi Atom (the runtime):");
    println!("       - Lightweight JVM runtime (~200MB)");
    println!("       - Can run in: Boomi Cloud, customer cloud (AWS/Azure/GCP), on-prem, edge");
    println!("       - 'Molecule' = clustered Atom for HA/load");
    println!("       - 'Cloud' = Boomi-hosted multi-tenant runtime");
    println!("    3. Boomi Flow (low-code workflow):");
    println!("       - Workflow + business app builder");
    println!("       - Compete with: Microsoft PowerApps, Salesforce Lightning, OutSystems");
    println!("    4. Master Data Hub (MDM):");
    println!("       - Customer/product/employee MDM");
    println!("       - Compete with: Informatica MDM, Reltio, Stibo");
    println!("    5. Boomi API Management:");
    println!("       - API gateway, design, lifecycle, developer portal");
    println!("       - Compete with: Kong, Apigee, MuleSoft Anypoint API Mgr");
    println!("    6. Boomi B2B/EDI Management:");
    println!("       - EDI processing (X12, EDIFACT)");
    println!("       - Trading partner mgmt");
    println!("       - Compete with: IBM Sterling, Cleo, OpenText Trading Grid");
    println!("    7. Boomi Event Streams (Kafka-as-a-service):");
    println!("       - Managed event streaming");
    println!("       - Integration with AtomSphere flows");
    println!("    8. Boomi AI (the 2023+ initiative):");
    println!("       - 'Boomi GPT' generative integration design");
    println!("       - 'Boomi AI Agent' natural-language process building");
    println!("       - 'Suggest' ML-powered mapping recommendations (long-running)");
    println!("    9. Boomi DataHub (the data layer):");
    println!("       - Reference data + lookup tables");
    println!("       - 'Synchronization' across systems");
    println!("    10. Boomi Discover (data + integration discovery):");
    println!("       - Catalog of all integrations + dependencies");
    println!("       - Impact analysis when changing connectors");
    println!("  Atom architecture (the differentiator):");
    println!("    - Lightweight (~200MB JVM-based) runtime engine");
    println!("    - Single binary deployed wherever connectivity is needed");
    println!("    - Can run in Boomi Cloud (most customers) or self-hosted (regulated industries)");
    println!("    - Hybrid orchestration: cloud-managed control plane + distributed runtime");
    println!("    - Allows iPaaS in air-gapped + on-prem-only environments (unique vs pure-SaaS competitors)");
    println!("  Integrations (200+ pre-built connectors):");
    println!("    - SaaS: Salesforce, NetSuite, Workday, ServiceNow, HubSpot, Marketo");
    println!("    - ERP: SAP, Oracle, Microsoft Dynamics, Sage Intacct, JD Edwards");
    println!("    - HCM: Workday, BambooHR, ADP, SuccessFactors, UKG");
    println!("    - CRM: Salesforce (deep), HubSpot, Microsoft Dynamics CRM");
    println!("    - Database: Oracle, SQL Server, PostgreSQL, MySQL, Snowflake, Redshift");
    println!("    - Cloud: AWS, Azure, GCP, OCI native services");
    println!("    - Messaging: Kafka, RabbitMQ, IBM MQ, JMS, Solace");
    println!("    - Files: SFTP, FTP, S3, Azure Blob, GCS, Box, Dropbox");
    println!("    - B2B: AS2, EDI X12, EDIFACT, RosettaNet");
    println!("  Boomi CLI usage:");
    println!("    boomi process list --filter folder=Production");
    println!("    boomi process deploy --process-id ABC123 --atom-id 'prod-cluster'");
    println!("    boomi atom create --name 'prod-cluster-1' --type cloud-elastic");
    println!("    boomi connection test --connection-id salesforce-prod");
    println!("    boomi mdh model deploy --model-name Customer");
    println!("    boomi api deploy --api-name Orders-v2 --gateway prod");
    println!("  Customers (~20,000+):");
    println!("    - One of the largest iPaaS customer bases globally");
    println!("    - Heavy in: mid-market manufacturing, retail, education, healthcare");
    println!("    - American Express, Toyota, Cisco, Moderna, Pearson, MGM Resorts");
    println!("    - U.S. federal: limited (vs MuleSoft strength there)");
    println!("    - International: very strong in EMEA + APAC mid-market");
    println!("    - Education sweet spot: ~700+ universities use Boomi");
    println!("  Critique: Atom architecture feels dated next to serverless competitors");
    println!("           UI/UX functional but lagging cloud-native challengers (Workato)");
    println!("           Boomi AI a follower, not a leader, in generative AI integration");
    println!("           Dell era (2010-2021) was slow innovation period — catching up post-spinout");
    println!("           pricing complexity (Atoms + connections + processes) frustrating to estimate");
    println!("           private-equity ownership pressure for margin growth vs new product investment");
    println!("           Workato + Microsoft Power Automate eating mid-market share");
    println!("           expected IPO timing uncertain in current market");
    println!("  Differentiator: pioneered iPaaS category in 2000 (24-year history) + Atom architecture (cloud + on-prem + edge deployment) + 20K+ customers (one of broadest iPaaS bases) + Master Data Hub MDM + B2B/EDI strong + sold by Dell for $4B in 2021 to PE — the most-established and broadly-deployed iPaaS for organizations that want hybrid deployment flexibility, particularly mid-market manufacturing, education, and retail");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "boomi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_boomi(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_boomi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/boomi"), "boomi");
        assert_eq!(basename(r"C:\bin\boomi.exe"), "boomi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("boomi.exe"), "boomi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_boomi(&["--help".to_string()], "boomi"), 0);
        assert_eq!(run_boomi(&["-h".to_string()], "boomi"), 0);
        let _ = run_boomi(&["--version".to_string()], "boomi");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_boomi(&[], "boomi");
    }
}
