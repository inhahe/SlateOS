#![deny(clippy::all)]

//! teradata-cli — SlateOS Teradata (Vantage data warehouse, the OG MPP DW, San Diego, NYSE:TDC)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_td(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: teradata [OPTIONS]");
        println!("Teradata (SlateOS) — Vantage MPP data warehouse (the original 1979 MPP DW, NYSE:TDC)");
        println!();
        println!("Options:");
        println!("  --vantage              VantageCloud (cloud-native managed Teradata)");
        println!("  --bynet                BYNET interconnect (the iconic MPP fabric)");
        println!("  --clearscape           ClearScape Analytics (in-DB ML + analytics functions)");
        println!("  --querygrid            QueryGrid (federated query across DWs/lakes)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Teradata 2024 (SlateOS) — bteq / tdload / Studio CLI"); return 0; }
    println!("Teradata 2024 (SlateOS) — The Original MPP Data Warehouse (since 1979)");
    println!("  Vendor: Teradata Corporation (San Diego, CA — NYSE:TDC since 2007)");
    println!("  Founders: Jack Shemer + Walter Muir + Carroll Reed + Jerold Modes + Phil Neches + others, 1979");
    println!("          Founded at Caltech research initiative — 'tera' = 10^12 = trillion (bytes)");
    println!("          One of the first companies built around MPP (Massively Parallel Processing)");
    println!("          Coined the 'data warehouse' category in 1980s");
    println!("          AT&T's NCR acquired Teradata 1991 → spun off Sep 2007 (NYSE IPO)");
    println!("          Steve McMillan: CEO 2020+ (turnaround focus, ex-Rackspace)");
    println!("  Public market (NYSE:TDC):");
    println!("         IPO Sept 2007 spin-off from NCR");
    println!("         FY2024 revenue: ~$1.83B (-2% YoY, declining)");
    println!("         Market cap: $2-4B range");
    println!("         Once $10B+ in early 2010s — eroded by cloud DWs");
    println!("         Recurring revenue ~$1.5B (cloud + subscription)");
    println!("         Activist investor Elliott Management 2023 pushing changes");
    println!("  Strategic position: 'enterprise data warehouse for the world's largest companies, evolving to cloud':");
    println!("                    pitch: 'enterprise-class analytics at massive scale — no other DW handles your size'");
    println!("                    target: Fortune 500 (banks, telcos, retailers) with petabyte-scale workloads");
    println!("                    primary competitor: Snowflake, Databricks, BigQuery (all eroding Teradata's base)");
    println!("                    secondary: Oracle Exadata, Microsoft Synapse, Redshift");
    println!("                    Teradata's wedge: 40+ years engineering, proven petabyte-scale, deep enterprise relationships");
    println!("                    challenge: customers slowly migrating to Snowflake/BQ for new workloads");
    println!("                    counter: VantageCloud + multi-cloud + AI/ML features");
    println!("  Pricing (enterprise-only, custom contracts):");
    println!("    VantageCloud Enterprise: from $20K/mo (multi-cloud, AWS/Azure/GCP)");
    println!("    VantageCloud Lake (lakehouse): consumption-based by capacity units");
    println!("    On-prem IntelliFlex: custom (typically $1M-$50M+/yr large customers)");
    println!("    Subscription: 3-year/5-year contracts standard");
    println!("    typically 2-5x more expensive than Snowflake equivalent");
    println!("    justified to enterprises by: workload manager + concurrency + advanced SQL");
    println!("  Architecture (the MPP pioneer):");
    println!("    - Shared-nothing MPP across AMPs (Access Module Processors)");
    println!("    - BYNET: proprietary high-speed interconnect (was network fabric innovation)");
    println!("    - Hash-based distribution across AMPs");
    println!("    - Each AMP owns slice of data + queries run in parallel");
    println!("    - Optimizer: 40+ years of cost-based optimization heritage");
    println!("    - Workload Management (TASM): industrial-grade priority + concurrency control");
    println!("    - 1000+ concurrent queries supported (vs Snowflake/BQ less concurrent before queueing)");
    println!("  Product portfolio:");
    println!("    1. VantageCloud (the cloud product):");
    println!("       - VantageCloud Enterprise: multi-cloud (AWS, Azure, GCP)");
    println!("       - VantageCloud Lake: lakehouse architecture (Iceberg-based)");
    println!("       - Migrated 1000+ customer workloads from on-prem to cloud");
    println!("       - Object storage tier for cold data");
    println!("    2. ClearScape Analytics (in-DB analytics):");
    println!("       - In-database ML functions (regression, classification, clustering)");
    println!("       - Time series + path/sequence analysis (sessionization, attribution)");
    println!("       - Text analytics, geospatial functions");
    println!("       - bring-your-own model + scoring in DB");
    println!("    3. QueryGrid (federation):");
    println!("       - Federated query across multiple data warehouses + Hadoop + cloud DWs");
    println!("       - Push-down to Snowflake, Hive, Presto, Aster, Db2");
    println!("       - 'Connected analytics' positioning");
    println!("    4. Aster (graph/text analytics — legacy):");
    println!("       - Acquired Aster Data 2011");
    println!("       - SQL-MapReduce (proto-Spark)");
    println!("       - Integrated into Vantage as ClearScape");
    println!("    5. IntelliFlex / IntelliCloud (on-prem):");
    println!("       - Modern on-prem hardware appliance");
    println!("       - PetaScale appliance (multi-PB customer deployments)");
    println!("       - Most customers migrating off these to cloud");
    println!("    6. Teradata Express (free dev edition):");
    println!("       - Single-node VM for development/testing");
    println!("       - On-ramp for new developers");
    println!("    7. Teradata Studio + SQL Assistant:");
    println!("       - GUI clients (Eclipse-based)");
    println!("       - SQL development + admin");
    println!("    8. Open Lake (Iceberg integration):");
    println!("       - Native Iceberg table support");
    println!("       - Query S3/ADLS data in place");
    println!("       - Bidirectional with Snowflake/Databricks Iceberg");
    println!("    9. Teradata ASK (NLP query interface):");
    println!("       - Natural language to SQL (2024 AI push)");
    println!("       - LLM-powered query assistance");
    println!("       - Compete with Snowflake Copilot, Databricks Assistant");
    println!("  The BYNET legacy:");
    println!("    - BYNET = Bynet (the interconnect since early 1990s)");
    println!("    - Proprietary high-speed network fabric across AMPs");
    println!("    - Innovation for its era: dedicated parallel-processing network");
    println!("    - In modern VantageCloud: virtual BYNET over cloud networks");
    println!("    - Trade-off: cloud-native vendors don't carry this legacy");
    println!("  The Snowflake/BigQuery erosion:");
    println!("    - 2014-2024: cloud DW market shifted to Snowflake + Databricks + BigQuery");
    println!("    - Teradata's revenue declined as customers migrated NEW workloads to cloud");
    println!("    - Defensive cloud product (VantageCloud 2017+) — late but improving");
    println!("    - Strategy: monetize 40+ year Fortune 500 customer base while modernizing");
    println!("    - Many enterprises still use Teradata for highest-scale + most-mature workloads");
    println!("    - Migration off Teradata is genuinely hard (thousands of stored procedures + workflows)");
    println!("  Integrations:");
    println!("    - bteq (Basic TEradata Query — the iconic CLI since 1980s)");
    println!("    - tdload / FastLoad / MultiLoad (bulk loaders)");
    println!("    - Teradata SDK: Python (teradataml), R, Java, JDBC, ODBC");
    println!("    - dbt-teradata adapter");
    println!("    - Tableau, Power BI, Cognos, Business Objects, MicroStrategy");
    println!("    - REST APIs for VantageCloud control plane");
    println!("    - Spark connector + Hive QueryGrid bridges");
    println!("    - Airflow + Informatica + Talend integration");
    println!("  Teradata CLI usage:");
    println!("    bteq                                                     # interactive SQL shell (since 1980s)");
    println!("    bteq <<EOF");
    println!("    .LOGON tdprod/user,password");
    println!("    SELECT COUNT(*) FROM customer;");
    println!("    .QUIT");
    println!("    EOF");
    println!("    tdload -j load_customers control.tdl                     # bulk load");
    println!("    fastload <fastload-script.fl>                            # FastLoad bulk import");
    println!("    multiload <multiload-script.ml>                          # MultiLoad updates");
    println!("    bteq -e 'SHOW TABLE my_db.my_table;'                     # show DDL");
    println!("    bteq -e 'HELP TABLE my_db.my_table;'                     # describe");
    println!("    tdwallet add MY_DB_PASSWORD                              # secure credential storage");
    println!("    teradata-ask 'show me sales by region last quarter'     # NLP query (2024)");
    println!("  Customers (Fortune 500 + global enterprise):");
    println!("    - Walmart (one of the world's largest Teradata installations)");
    println!("    - eBay (early big customer)");
    println!("    - American Airlines, Delta, UPS, FedEx");
    println!("    - Banking: Wells Fargo, Bank of America, Citi");
    println!("    - Telcos: Vodafone, Telefonica, T-Mobile");
    println!("    - 1,500+ enterprise customers");
    println!("    - >$1M average annual contract value");
    println!("    - 75% top 100 banks, 80% top 50 telcos");
    println!("  Critique: revenue declining (-2% FY2024) as cloud DWs erode workloads");
    println!("           VantageCloud late to market vs Snowflake/BQ");
    println!("           expensive vs Snowflake for new workloads (2-5x)");
    println!("           legacy SQL dialect (Teradata SQL) — non-standard quirks");
    println!("           on-prem hardware appliances increasingly obsolete model");
    println!("           difficulty attracting young developers familiar with cloud DWs");
    println!("           workload manager + concurrency strengths underappreciated");
    println!("           activist investor pressure (Elliott Management 2023) shows market doubt");
    println!("           churn risk: large customers eventually plan exits");
    println!("  Differentiator: 45+ year MPP data warehouse pioneer (since 1979) + invented data warehouse category + BYNET interconnect heritage + 1000+ concurrent query support + petabyte-scale proven at Walmart/eBay/airlines/banks + workload manager (TASM) industrial-grade priority control + 1,500+ Fortune 500 customers + VantageCloud multi-cloud (AWS/Azure/GCP) + ClearScape in-DB ML + Open Lake Iceberg + Teradata ASK NLP-to-SQL + $1.83B revenue with deep enterprise relationships — the original MPP data warehouse that still handles the largest workloads at the world's largest companies, evolving to cloud while monetizing 40+ years of Fortune 500 entrenchment");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "teradata".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_td(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_td};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/teradata"), "teradata");
        assert_eq!(basename(r"C:\bin\teradata.exe"), "teradata.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("teradata.exe"), "teradata");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_td(&["--help".to_string()], "teradata"), 0);
        assert_eq!(run_td(&["-h".to_string()], "teradata"), 0);
        let _ = run_td(&["--version".to_string()], "teradata");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_td(&[], "teradata");
    }
}
