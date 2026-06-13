#![deny(clippy::all)]

//! informatica-cli — SlateOS Informatica (IDMC platform, Redwood City CA, NYSE:INFA — Salesforce acq 2025)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_informatica(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: informatica [OPTIONS]");
        println!("Informatica (Slate OS) — IDMC + PowerCenter (cloud + on-prem ETL/iPaaS/MDM)");
        println!();
        println!("Options:");
        println!("  --idmc                 Intelligent Data Management Cloud (the cloud platform)");
        println!("  --powercenter          PowerCenter (legacy on-prem ETL)");
        println!("  --mdm                  Master Data Management");
        println!("  --data-quality         Data Quality + Profiling");
        println!("  --data-governance      Axon Data Governance + Enterprise Data Catalog");
        println!("  --claire               CLAIRE (AI metadata engine)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Informatica IDMC 2024 (Slate OS)"); return 0; }
    println!("Informatica 2024 (Slate OS) — Intelligent Data Management Cloud (IDMC)");
    println!("  Vendor: Informatica, LLC (Redwood City, CA — NYSE:INFA 2021-2025, Salesforce acq pending)");
    println!("  Founder: Gaurav Dhillon + Diaz Nesamoney, 1993");
    println!("          founded as data integration pioneer — defined the ETL category");
    println!("          PowerCenter (1998) was the dominant enterprise ETL tool for 2 decades");
    println!("          went public NASDAQ:INFA 1999, peaked late dotcom");
    println!("          taken private 2015 by Permira + CPP Investment Board for $5.3B");
    println!("          IPO again Oct 2021 at $29/share on NYSE:INFA");
    println!("          Amit Walia: long-time CEO (since 2020)");
    println!("  Public market history (NYSE:INFA):");
    println!("         Second IPO Oct 2021 — raised $841M");
    println!("         peak ~$32 in late 2024");
    println!("         FY2024 revenue: ~$1.6B (largest pure-play data integration vendor)");
    println!("         Market cap: ~$8-9B");
    println!("         Salesforce announced acquisition May 2025 for ~$8B");
    println!("         Strategic fit: Salesforce 'Data + AI' play with MuleSoft + Tableau + Data Cloud + Informatica");
    println!("         Cited as Salesforce's biggest data-fabric move since MuleSoft 2018 ($6.5B)");
    println!("  Strategic position: '#1 in enterprise data management — cloud, on-prem, hybrid':");
    println!("                    pitch: 'the data management leader for the AI era — IDMC + CLAIRE for everything'");
    println!("                    target: large enterprise (Fortune 500 sweet spot)");
    println!("                    primary competitor: Talend (acquired by Qlik), Microsoft Fabric, AWS Glue, Fivetran (modern stack)");
    println!("                    secondary: Matillion, dbt, Snowflake (overlap on ELT), MuleSoft (iPaaS), Boomi (iPaaS)");
    println!("                    Informatica's wedge: largest enterprise install base + CLAIRE AI metadata + breadth of capabilities");
    println!("                    challenge: cloud-native disruption from Fivetran/dbt/modern stack");
    println!("  Pricing:");
    println!("    PowerCenter (on-prem): $100K-$5M+/yr (legacy, declining)");
    println!("    IDMC (cloud): $50K-$3M+/yr — consumption-based 'IPU' (Informatica Processing Unit) model");
    println!("    MDM: $200K-$5M+/yr");
    println!("    Cloud Data Quality: $50K-$1M+/yr");
    println!("    Axon Data Governance: $100K-$2M+/yr");
    println!("    typically the most expensive per-seat in data integration — enterprise-grade premium");
    println!("  Product portfolio (IDMC = Intelligent Data Management Cloud):");
    println!("    1. Cloud Data Integration (CDI — ETL/ELT):");
    println!("       - Cloud-native ETL/ELT pipelines");
    println!("       - Successor to PowerCenter");
    println!("       - Pushdown to Snowflake, Databricks, BigQuery for ELT");
    println!("    2. Cloud Data Quality (CDQ):");
    println!("       - Data profiling, validation, cleansing rules");
    println!("       - Match + merge, address standardization");
    println!("    3. Master Data Management (MDM) Cloud:");
    println!("       - Customer 360, Product 360, Supplier 360");
    println!("       - Survivorship rules, hierarchy management");
    println!("       - Compete with: Reltio, Stibo, Boomi MDH");
    println!("    4. Enterprise Data Catalog (EDC) + Axon (data governance):");
    println!("       - Data discovery, lineage, glossary");
    println!("       - Compete with: Collibra, Alation, Atlan, Microsoft Purview");
    println!("    5. Cloud Application Integration (iPaaS):");
    println!("       - API mgmt + workflow integration");
    println!("       - Compete with: MuleSoft, Boomi, Workato");
    println!("    6. Data Privacy + Protection:");
    println!("       - PII discovery, dynamic data masking, compliance");
    println!("       - Compete with: BigID, Privacera, Immuta");
    println!("    7. Cloud Mass Ingestion:");
    println!("       - Bulk + streaming ingestion at scale");
    println!("       - CDC (change data capture) for databases");
    println!("    8. PowerCenter (legacy on-prem):");
    println!("       - Still ~30% of revenue (declining)");
    println!("       - Maintenance + migration to IDMC");
    println!("    9. CLAIRE (AI metadata engine — the differentiator):");
    println!("       - Active metadata graph across all Informatica products");
    println!("       - ML-driven schema inference, anomaly detection, recommendations");
    println!("       - CLAIRE GPT (2023) = natural-language interface to data");
    println!("       - Lineage automation, smart impact analysis");
    println!("    10. CLAIRE Copilot (2024):");
    println!("       - Natural-language pipeline building");
    println!("       - LLM-augmented data engineering");
    println!("  CLAIRE strategy (the AI bet):");
    println!("    - 'Active metadata' graph populated by all Informatica tools");
    println!("    - ML/LLM-powered recommendations + impact analysis");
    println!("    - CLAIRE GPT exposed as conversational interface");
    println!("    - Key differentiator vs cloud-native data tools");
    println!("    - Strategic asset for Salesforce post-acquisition (feeds Einstein/Data Cloud)");
    println!("  Integrations (5,000+ data sources):");
    println!("    - Databases: Oracle, SQL Server, DB2, Teradata, PostgreSQL, MySQL, MongoDB");
    println!("    - Cloud DW: Snowflake, Databricks, BigQuery, Redshift, Synapse, Fabric");
    println!("    - Apps: Salesforce, Workday, NetSuite, ServiceNow, SAP, Oracle EBS");
    println!("    - Files: SFTP, FTP, S3, Azure Blob, GCS, HDFS, ADLS");
    println!("    - Messaging: Kafka, Kinesis, Event Hubs, Pub/Sub, JMS");
    println!("    - Legacy: Mainframe (DB2 z/OS, IMS, VSAM), iSeries, COBOL Copybooks");
    println!("    - Streaming: Kafka CDC, Oracle GoldenGate, SQL Server CDC");
    println!("  Informatica CLI usage:");
    println!("    informatica idmc login --org my-org");
    println!("    informatica idmc job list --status running");
    println!("    informatica idmc mapping deploy --mapping-id ABC123 --env prod");
    println!("    informatica mdm process customer --batch-id 456");
    println!("    informatica edc lineage --asset 'sales.orders' --direction downstream");
    println!("    informatica claire chat --query 'find PII in customer dataset'");
    println!("  Customers (~10,500+):");
    println!("    - 87% of Fortune 100");
    println!("    - All top 25 global banks");
    println!("    - 8 of top 10 healthcare companies");
    println!("    - U.S. federal: DoD, IRS, civilian agencies");
    println!("    - International: heavy in Europe + Japan");
    println!("    - sweet spot: Fortune 1000 + global 2000 + government");
    println!("  Critique: cloud transition still incomplete — ~30% PowerCenter legacy revenue");
    println!("           cloud-native disruptors (Fivetran, dbt, Matillion) eating modern-stack share");
    println!("           Microsoft Fabric + AWS Glue + Azure Data Factory threaten cloud-only customers");
    println!("           IDMC pricing (IPU) confusion for buyers");
    println!("           CLAIRE marketing exceeds capabilities reality");
    println!("           Salesforce acquisition 2025 = integration risk + go-to-market disruption");
    println!("           innovation pace slower than newer modern-stack vendors");
    println!("           Talend, IBM, Microsoft, AWS bundling pressure on prices");
    println!("  Differentiator: largest enterprise data integration vendor + IDMC unified platform (ETL + MDM + DQ + governance + iPaaS + privacy) + CLAIRE AI metadata engine + 5,000+ data source connectivity (including mainframe + legacy) + 87% Fortune 100 install base + $1.6B revenue + Salesforce acquisition 2025 ($8B) for 'Data + AI' strategy alongside MuleSoft + Tableau + Data Cloud — the data management leader that big enterprises trust for cloud + on-prem + hybrid + legacy data integration");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "informatica".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_informatica(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_informatica};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/informatica"), "informatica");
        assert_eq!(basename(r"C:\bin\informatica.exe"), "informatica.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("informatica.exe"), "informatica");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_informatica(&["--help".to_string()], "informatica"), 0);
        assert_eq!(run_informatica(&["-h".to_string()], "informatica"), 0);
        let _ = run_informatica(&["--version".to_string()], "informatica");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_informatica(&[], "informatica");
    }
}
