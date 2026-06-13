#![deny(clippy::all)]

//! singlestore-cli — SlateOS SingleStore (formerly MemSQL, real-time HTAP, San Francisco, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_s2(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: singlestore [OPTIONS]");
        println!("SingleStore (Slate OS) — real-time distributed HTAP database (formerly MemSQL)");
        println!();
        println!("Options:");
        println!("  --universal-storage    Universal Storage (rowstore + columnstore unified)");
        println!("  --vector               Native vector search (HNSW + cosine/dot/Euclidean)");
        println!("  --pipelines            Pipelines (continuous ingest from Kafka/S3/Pulsar/etc.)");
        println!("  --workspaces           Workspaces (isolated compute on shared storage)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SingleStore 2024 (Slate OS) — singlestore CLI 1.x"); return 0; }
    println!("SingleStore 2024 (Slate OS) — Real-Time Distributed HTAP Database");
    println!("  Vendor: SingleStore, Inc. (San Francisco, CA — private)");
    println!("          Renamed from MemSQL in October 2020");
    println!("  Founders: Eric Frenkiel + Nikita Shamgunov, 2011 (ex-Facebook engineers)");
    println!("          Eric Frenkiel: ex-Facebook, ex-Oracle, original CEO until 2019");
    println!("          Nikita Shamgunov: CTO, deep DB systems background, now Neon (Postgres)");
    println!("          Raj Verma: CEO 2019+ (ex-TIBCO president)");
    println!("          'MemSQL' = MySQL-compatible in-memory database");
    println!("          Renaming to SingleStore signaled HTAP positioning (one DB, transactional + analytical)");
    println!("  Funding:");
    println!("         Total raised: ~$464M across rounds");
    println!("         Series F May 2022: $116M at $1.3B+ valuation");
    println!("         Goldman Sachs, GV, IA Ventures, Khosla Ventures investors");
    println!("         Profitable as of 2024 — preparing for eventual IPO");
    println!("         Revenue ~$100M+ (estimate 2024, ~50% YoY growth)");
    println!("  Strategic position: 'one database for transactions + analytics + AI — HTAP for real-time':");
    println!("                    pitch: 'no more Postgres+Snowflake+vector DB — one engine, real-time everything'");
    println!("                    target: real-time analytics + dashboards + AI applications");
    println!("                    primary competitor: Snowflake + Postgres + Pinecone (the assembly)");
    println!("                    secondary: Clickhouse, Rockset (acquired by OpenAI), Pinot, Druid");
    println!("                    SingleStore's wedge: true HTAP — concurrent reads/writes with no replication delay");
    println!("                    Universal Storage (rowstore + columnstore in one table) = unique");
    println!("                    AI/vector push 2023+: native HNSW + JSON + relational + vector");
    println!("  Pricing (consumption-based on cloud, custom for self-managed):");
    println!("    Free tier: 'Shared Tier' free starter workspace");
    println!("    Cloud Standard: $0.90/credit/hr (S00 compute), storage $0.07/GB-month");
    println!("    Cloud Premium: $1.30/credit/hr (more I/O + faster networking)");
    println!("    Self-managed: per-node licensing, custom pricing");
    println!("    typically 30-50% cheaper than Snowflake for sustained mixed OLTP+OLAP workloads");
    println!("    pitch: 'reduce 3 DB licenses to 1'");
    println!("  Architecture (the HTAP design):");
    println!("    - Distributed shared-nothing across aggregator + leaf nodes");
    println!("    - Aggregator: SQL compilation, query routing, transaction coordination");
    println!("    - Leaf: data storage + execution");
    println!("    - Universal Storage: every table can be rowstore (OLTP) or columnstore (OLAP) or both");
    println!("    - In-memory rowstore (lock-free, MVCC)");
    println!("    - Compiled query plans → C++ → native machine code (cached)");
    println!("    - MySQL wire protocol (drop-in replacement-ish)");
    println!("    - Workspaces: isolated compute pools sharing storage");
    println!("  Product portfolio:");
    println!("    1. SingleStoreDB (the core engine):");
    println!("       - Distributed SQL with MySQL compatibility");
    println!("       - Universal Storage (rowstore + columnstore)");
    println!("       - JSON + geospatial + full-text + vector support");
    println!("       - Real-time ingest + analytics in same engine");
    println!("    2. Pipelines (the real-time ingest):");
    println!("       - Continuous data ingestion from Kafka, S3, Azure Blob, GCS, Pulsar, Filesystem");
    println!("       - Exactly-once semantics");
    println!("       - In-flight SQL transformations");
    println!("       - 'Stream + batch unified in one engine' pitch");
    println!("    3. Vector + Hybrid Search:");
    println!("       - DOT_PRODUCT, COSINE_SIMILARITY, EUCLIDEAN_DISTANCE built-in");
    println!("       - HNSW index for ANN search");
    println!("       - Combine vector + filter + full-text in one SQL query");
    println!("       - GA 2023, expanded 2024");
    println!("    4. Workspaces (compute isolation):");
    println!("       - Multiple workspaces share same data");
    println!("       - Isolate dashboard queries from ingest queries");
    println!("       - Scale workspaces independently");
    println!("    5. Notebooks (built-in):");
    println!("       - Jupyter-style notebooks in SingleStore Cloud UI");
    println!("       - Python + SQL interleaved");
    println!("       - 2023+ feature, increasingly capable");
    println!("    6. Helios (the cloud platform):");
    println!("       - Multi-cloud (AWS, Azure, GCP)");
    println!("       - Customer-managed VPC option (BYOC)");
    println!("       - Auto-scale + auto-pause idle workspaces");
    println!("    7. Studio (DBA + dev UI):");
    println!("       - Visual schema designer + query builder");
    println!("       - Performance + workload analyzer");
    println!("    8. Free Tier + Singular Notebooks:");
    println!("       - Always-free shared workspace");
    println!("       - On-ramp for evaluation");
    println!("    9. AI/RAG SDK + integrations:");
    println!("       - Direct integration with OpenAI/Anthropic embeddings");
    println!("       - SQL functions for embedding generation");
    println!("       - LangChain + LlamaIndex connectors");
    println!("  Universal Storage (the key innovation):");
    println!("    - Single table type that's row-based + column-based simultaneously");
    println!("    - Rowstore optimized for OLTP (point lookups, inserts, updates)");
    println!("    - Columnstore optimized for analytics (aggregations, scans)");
    println!("    - Hybrid: hot data rowstore, cold data columnstore (automatic)");
    println!("    - Per-table mode: pure rowstore, pure columnstore, or universal");
    println!("    - This eliminates the OLTP/OLAP split that forces 2 DBs in most architectures");
    println!("  The MemSQL → SingleStore rename (Oct 2020):");
    println!("    - 'MemSQL' = in-memory MySQL-compatible (original 2011 positioning)");
    println!("    - As columnstore + Universal Storage matured, name became misleading (not just memory anymore)");
    println!("    - 'SingleStore' = positions as 'one store for everything' (HTAP message)");
    println!("    - Smart rebrand — captured HTAP + vector momentum 2021-2024");
    println!("  The AI bet (2023+):");
    println!("    - Native vector + hybrid search before Pinecone competition heated up");
    println!("    - RAG-as-a-database positioning");
    println!("    - 'You don't need a separate vector DB' = positioning vs Pinecone/Weaviate");
    println!("    - Integrated with major embedding providers + LangChain");
    println!("  Integrations:");
    println!("    - singlestore CLI (Python-based)");
    println!("    - mysql client (wire-compatible)");
    println!("    - SDKs: Python, JS, Go, Java, .NET, Ruby (MySQL drivers work)");
    println!("    - dbt-singlestore adapter");
    println!("    - Spark connector, Kafka connector, Flink connector");
    println!("    - Tableau, Power BI, Looker, Metabase, Grafana");
    println!("    - LangChain + LlamaIndex vector store");
    println!("  SingleStore CLI usage:");
    println!("    singlestore login                                        # auth");
    println!("    singlestore workspace-group create --name=my-wg --provider=aws --region=us-east-1");
    println!("    singlestore workspace create --name=my-ws --size=S-2 --workspace-group-id=ID");
    println!("    mysql -h ws-host -P 3306 -u admin -p                     # MySQL-compatible connection");
    println!("    CREATE DATABASE my_db;");
    println!("    USE my_db;");
    println!("    CREATE TABLE docs (id INT, content TEXT, embedding VECTOR(1536));");
    println!("    CREATE INDEX docs_emb USING HNSW ON docs (embedding) WITH (metric='COSINE');");
    println!("    SELECT id FROM docs ORDER BY embedding <-> :query_vec LIMIT 10;");
    println!("    CREATE PIPELINE sales AS LOAD DATA KAFKA 'kafka://topic' INTO TABLE sales;");
    println!("    START PIPELINE sales;");
    println!("  Customers (real-time + ad-tech + fintech + AI):");
    println!("    - Goldman Sachs (real-time risk + trade analytics)");
    println!("    - Comcast, Nucleus Security, Akamai");
    println!("    - Adtech: SOLVE, Outbrain, Criteo (some)");
    println!("    - AI startups using as RAG vector store");
    println!("    - 1,000+ enterprise customers");
    println!("    - Use cases: real-time dashboards, fraud detection, IoT analytics, RAG/LLM apps");
    println!("  Critique: Snowflake + Postgres + Pinecone assembly remains common despite HTAP pitch");
    println!("           MySQL wire compat is partial (not 100% drop-in for app code)");
    println!("           perception: 'in-memory only' lingers from MemSQL days");
    println!("           less open-source presence than Postgres/Clickhouse");
    println!("           rebrand consumed marketing attention 2020-2022");
    println!("           competition: ClickHouse + Snowflake + Postgres + Pinecone each strong in their lanes");
    println!("           vector search came after Pinecone/Weaviate already established");
    println!("           closed-source business model in an open-source-friendly DB world");
    println!("  Differentiator: HTAP architecture (Universal Storage rowstore + columnstore in same table) + real-time ingest via Pipelines + native vector search (HNSW + cosine/dot/Euclidean) + MySQL wire protocol compatibility + distributed SQL across aggregator/leaf nodes + Workspaces compute isolation + compiled query plans (C++ codegen) + ex-Facebook founders (Eric Frenkiel + Nikita Shamgunov, 2011) + $464M raised + $1.3B+ valuation + 'one DB for transactions + analytics + vector' positioning + Goldman/Comcast/Akamai customers + ~$100M revenue with 50% growth — the HTAP database that replaces Postgres + Snowflake + Pinecone with one engine for real-time apps + AI/RAG workloads");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "singlestore".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_s2(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_s2};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/singlestore"), "singlestore");
        assert_eq!(basename(r"C:\bin\singlestore.exe"), "singlestore.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("singlestore.exe"), "singlestore");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_s2(&["--help".to_string()], "singlestore"), 0);
        assert_eq!(run_s2(&["-h".to_string()], "singlestore"), 0);
        let _ = run_s2(&["--version".to_string()], "singlestore");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_s2(&[], "singlestore");
    }
}
