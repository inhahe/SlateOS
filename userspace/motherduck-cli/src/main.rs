#![deny(clippy::all)]

//! motherduck-cli — SlateOS MotherDuck (DuckDB cloud, hybrid execution, Seattle, private 2022)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_md(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: motherduck [OPTIONS]");
        println!("MotherDuck (Slate OS) — DuckDB-in-the-cloud, hybrid local/cloud query execution");
        println!();
        println!("Options:");
        println!("  --hybrid               Hybrid execution (local DuckDB + MotherDuck cloud)");
        println!("  --shares               Database sharing (instant collaboration)");
        println!("  --extensions           DuckDB extensions (httpfs, parquet, json, iceberg)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("MotherDuck 2024 (Slate OS) — DuckDB CLI 1.x with MotherDuck extension"); return 0; }
    println!("MotherDuck 2024 (Slate OS) — DuckDB-in-the-Cloud (Hybrid Local + Cloud)");
    println!("  Vendor: MotherDuck, Inc. (Seattle, WA — private since 2022)");
    println!("  Founders: Jordan Tigani (ex-BigQuery founding engineer) + Tino Tereshko (ex-BigQuery PM), 2022");
    println!("          Jordan Tigani: famous '$1 query' BigQuery talks, blog 'Big Data is Dead' (2023)");
    println!("          Hannes Mühleisen (DuckDB co-creator): MotherDuck CTO + Chief Duck");
    println!("          Mark Raasveldt (DuckDB co-creator): MotherDuck CDO");
    println!("          'The data is moving to where the user is, not the other way around'");
    println!("          Tagline: 'serverless analytics that's actually serverless'");
    println!("  Funding:");
    println!("         Series A Nov 2022: $47.5M (Andreessen Horowitz, Madrona, Redpoint)");
    println!("         Series B Sep 2024: $52M (Felicis Ventures lead, total $100M+ raised)");
    println!("         Valuation: $400M (2024)");
    println!("  Strategic position: 'small data is normal — most workloads fit on a laptop, just need cloud assist':");
    println!("                    pitch: 'Big Data is dead — your data fits in DuckDB on a laptop; cloud is for sharing + scale-up only when needed'");
    println!("                    target: data analysts + dev teams + data engineers tired of Snowflake bills");
    println!("                    primary competitor: Snowflake, BigQuery (for small/medium analytical workloads)");
    println!("                    secondary: ClickHouse, BigQuery Cheap Tier, Trino+Iceberg");
    println!("                    MotherDuck's wedge: DuckDB credibility + hybrid execution + dirt-cheap pricing for small data");
    println!("                    Jordan Tigani's 'Big Data is Dead' blog crystallized the thesis");
    println!("                    network effect: DuckDB community 100K+ devs love it = MotherDuck on-ramp");
    println!("  Pricing (notably cheap):");
    println!("    Free tier: 10 GB storage + 10 CUs/month (Compute Units, ~10 hours of small queries)");
    println!("    Standard: $25/mo + $0.25/CU (effectively pay-as-you-query)");
    println!("    Enterprise: custom (SSO, audit, SLAs)");
    println!("    typically 5-10x cheaper than Snowflake for sub-100GB workloads");
    println!("    hybrid execution: queries that run locally cost nothing");
    println!("  Architecture (the hybrid model):");
    println!("    - Built on DuckDB (open-source columnar analytical OLAP engine)");
    println!("    - 'MotherDuck' = cloud service hosting DuckDB databases");
    println!("    - Hybrid execution: query decides whether to run local or cloud per operator");
    println!("    - Storage: cloud parquet files + DuckDB-format files");
    println!("    - 'Friend Mode': databases shared instantly with collaborators");
    println!("    - 'Wide Mode': scale to larger machines on-demand");
    println!("    - DuckDB extensions: httpfs, parquet, json, iceberg, postgres_scanner, etc.");
    println!("  Product portfolio:");
    println!("    1. MotherDuck cloud database:");
    println!("       - Hosted DuckDB databases in cloud (AWS-backed)");
    println!("       - Persistent storage in MotherDuck (no laptop required)");
    println!("       - REST + DuckDB wire protocol access");
    println!("    2. Hybrid query execution (the key innovation):");
    println!("       - Query split across local + cloud automatically");
    println!("       - Local: small filters, joins; Cloud: large aggregations, scans");
    println!("       - Reduces data movement + speeds queries");
    println!("       - The 'best of both worlds' pitch");
    println!("    3. Database sharing ('Shares'):");
    println!("       - Instantly share a database with a teammate (URL-based)");
    println!("       - Read-only + read-write modes");
    println!("       - Compete with Snowflake Data Sharing (but free + instant)");
    println!("    4. dbt-motherduck integration:");
    println!("       - First-class dbt adapter");
    println!("       - 'dbt + DuckDB' = popular pattern for small/medium teams");
    println!("    5. Notebook integration:");
    println!("       - Jupyter, Hex, VSCode native");
    println!("       - Pandas DataFrames ↔ MotherDuck zero-copy");
    println!("    6. AI Cell (LLM-powered SQL):");
    println!("       - PROMPT() SQL function (LLM in SQL)");
    println!("       - Embed Gemini/OpenAI inside queries");
    println!("       - 'NL to SQL' assistant");
    println!("    7. DuckDB extension marketplace:");
    println!("       - vss (vector similarity search)");
    println!("       - h3 (Uber spatial indexing)");
    println!("       - postgres / mysql / sqlite scanners");
    println!("       - delta / iceberg connectors");
    println!("       - 50+ official + community extensions");
    println!("    8. ATTACH (multi-database queries):");
    println!("       - ATTACH 'md:my_db' AS shared_data");
    println!("       - Query across local files + cloud DBs in one query");
    println!("       - The hybrid model materialized in SQL");
    println!("  DuckDB (the open-source engine — CWI Amsterdam, 2018+):");
    println!("    - Created by Hannes Mühleisen + Mark Raasveldt at CWI (Dutch national research institute)");
    println!("    - Embedded analytical DB (like SQLite but columnar)");
    println!("    - C++ single binary, no server, embeds in Python/R/Node");
    println!("    - Reads Parquet/CSV/JSON natively");
    println!("    - 'SQLite for analytics' = standard framing");
    println!("    - 4M+ downloads/week (PyPI 2024)");
    println!("    - 22K+ GitHub stars, beloved by data community");
    println!("    - DuckDB Labs (the foundation) owns the open-source");
    println!("    - MotherDuck = the commercial cloud company built around it");
    println!("  Jordan Tigani's 'Big Data is Dead' thesis (Feb 2023 blog):");
    println!("    - 'Most analytical queries scan less data than fits in a phone's RAM'");
    println!("    - 'The largest 90% of data warehouse queries scan <1TB'");
    println!("    - 'Data is growing slower than compute is getting cheaper'");
    println!("    - Implication: distributed cloud DW overkill for most workloads");
    println!("    - Foundational thesis for MotherDuck's market positioning");
    println!("    - Sparked industry debate — competitors (Snowflake/Databricks) pushed back");
    println!("  Integrations:");
    println!("    - DuckDB CLI (open source) + MotherDuck extension");
    println!("    - SDKs: Python (duckdb), R, Node, Go, Rust, Java, Julia");
    println!("    - dbt-duckdb / dbt-motherduck adapters");
    println!("    - Jupyter + Hex + Mode + VSCode native");
    println!("    - Tableau, Power BI, Mode, Metabase via JDBC/ODBC");
    println!("    - Iceberg + Delta + Hudi read support");
    println!("    - Direct cloud storage: S3, GCS, Azure Blob, R2");
    println!("    - Postgres + MySQL + SQLite scanners (query in place)");
    println!("    - AI Cell with OpenAI + Gemini + Anthropic");
    println!("  MotherDuck CLI usage:");
    println!("    duckdb md:                                               # connect to MotherDuck");
    println!("    duckdb md:my_db                                          # connect to a database");
    println!("    .open md:my_db");
    println!("    ATTACH 'md:shared_db';                                   # attach a remote shared DB");
    println!("    CREATE TABLE sales AS SELECT * FROM read_parquet('s3://bucket/sales/*.parquet');");
    println!("    SELECT region, SUM(amount) FROM sales GROUP BY region;");
    println!("    .show                                                    # show current settings");
    println!("    CREATE SHARE sales_share FROM sales;                     # share a table");
    println!("    SELECT PROMPT('Categorize: ' || description, model:='gpt-4') FROM tickets;");
    println!("    SET motherduck_database_size_limit='10GB';");
    println!("    .mode duckbox                                            # the iconic ducky output");
    println!("  Customers (data teams, analysts, scale-ups):");
    println!("    - Hex Notebook (deep MotherDuck partnership)");
    println!("    - Dagster, Mode, Hex, Definite (BI tool partnerships)");
    println!("    - Mostly small-to-mid teams + analytics consultancies");
    println!("    - Strong with: dbt users, Python/notebook analysts, Mage/Dagster users");
    println!("    - 2,000+ customer accounts (estimate 2024)");
    println!("  Critique: young company (2022) — operational risk for enterprise");
    println!("           Snowflake/BigQuery have much larger ecosystems");
    println!("           hybrid execution can be surprising (where exactly does each op run?)");
    println!("           depends on DuckDB ecosystem health (CWI-spinoff governance question)");
    println!("           'Big Data is Dead' polemic alienates some enterprise buyers with real big data");
    println!("           limited support tier — community-focused, not 24/7 enterprise SLA yet");
    println!("           pricing model (Compute Units) requires learning");
    println!("           ecosystem of integrations smaller than Snowflake/BigQuery");
    println!("  Differentiator: DuckDB-in-the-cloud (built on the columnar analytical engine 4M+ weekly downloads) + Jordan Tigani founder (ex-BigQuery founding engineer + 'Big Data is Dead' thesis) + Hannes Mühleisen + Mark Raasveldt (DuckDB co-creators as CTO + CDO) + hybrid local/cloud execution (queries run wherever cheapest) + 5-10x cheaper than Snowflake for sub-100GB + instant DB sharing + AI Cell SQL-embedded LLMs + dbt-motherduck + 50+ DuckDB extensions + $100M raised + $400M valuation — the cloud DW for the post-'Big Data' era where most analytical workloads fit on a laptop and the cloud is just for sharing and occasional scale-up");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "motherduck".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_md(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_md};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/motherduck"), "motherduck");
        assert_eq!(basename(r"C:\bin\motherduck.exe"), "motherduck.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("motherduck.exe"), "motherduck");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_md(&["--help".to_string()], "motherduck"), 0);
        assert_eq!(run_md(&["-h".to_string()], "motherduck"), 0);
        let _ = run_md(&["--version".to_string()], "motherduck");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_md(&[], "motherduck");
    }
}
