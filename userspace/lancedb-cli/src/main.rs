#![deny(clippy::all)]

//! lancedb-cli — Slate OS LanceDB (multi-modal Rust vector DB on Lance columnar format, SF, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lance(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lancedb [OPTIONS]");
        println!("LanceDB (Slate OS) — multi-modal Rust vector DB on Lance columnar format");
        println!();
        println!("Options:");
        println!("  --lance                Lance columnar format (the underlying file format)");
        println!("  --serverless           Multi-modal serverless DB (zero infra)");
        println!("  --cloud                LanceDB Cloud (managed, GA 2024)");
        println!("  --enterprise           LanceDB Enterprise (on-prem + private cloud)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("LanceDB 2024 (Slate OS) — lancedb CLI 0.x"); return 0; }
    println!("LanceDB 2024 (Slate OS) — Multi-Modal Vector Database on Lance Columnar Format");
    println!("  Vendor: LanceDB, Inc. (San Francisco, CA — private)");
    println!("  Founders: Chang She + Lei Xu, 2022");
    println!("          Chang She: co-creator of pandas (yes, *that* pandas), ex-Tubi, ex-Aviary");
    println!("          Lei Xu: ex-Cruise (autonomous driving), ex-NetApp, ex-Couchbase");
    println!("          Founded out of Cruise pain point: storing + querying multi-modal AI data");
    println!("          Built Lance file format first → LanceDB on top");
    println!("          Open-source first (Apache 2.0)");
    println!("  Funding:");
    println!("         Total raised: ~$11M");
    println!("         Seed Aug 2023: $8M (Y Combinator + Essence VC + others)");
    println!("         Series A unannounced 2024");
    println!("         Smaller funding than competitors — lean engineering team");
    println!("  Strategic position: 'multi-modal vector DB on open Lance format — embedded or serverless':");
    println!("                    pitch: 'multi-modal AI data storage + vector search on open columnar format'");
    println!("                    target: multi-modal AI workloads (text + image + video + audio embeddings)");
    println!("                    primary competitor: Chroma (simplicity), Qdrant (Rust + perf), Pinecone, Weaviate");
    println!("                    secondary: pgvector, sqlite-vec");
    println!("                    LanceDB's wedge: Lance columnar format + true multi-modal data + zero-copy + embedded mode");
    println!("                    'Postgres for AI' aspirational framing");
    println!("                    Chang She's pandas pedigree → high engineering credibility");
    println!("  Pricing (OSS + Cloud + Enterprise):");
    println!("    LanceDB OSS: free (Apache 2.0)");
    println!("    LanceDB Cloud Free: free dev tier");
    println!("    LanceDB Cloud: consumption-based (compute + storage)");
    println!("    LanceDB Enterprise: on-prem or BYOC license");
    println!("    typically competitive with Chroma Cloud + Pinecone Serverless");
    println!("  Architecture (the Lance format + DB):");
    println!("    - Written in Rust (single static binary)");
    println!("    - Lance file format: columnar, optimized for vector search + multi-modal data");
    println!("    - Lance vs Parquet: faster random access for vector retrieval");
    println!("    - Storage on local disk + S3 + Azure Blob + GCS (cloud-native)");
    println!("    - IVF_PQ + HNSW indexes");
    println!("    - DuckDB integration for SQL queries");
    println!("    - Pandas/Polars DataFrame integration (zero-copy via Arrow)");
    println!("    - Embedded (no server) + remote modes share same API");
    println!("  Product portfolio:");
    println!("    1. Lance file format (the foundation):");
    println!("       - Columnar format, Apache Arrow-compatible");
    println!("       - Optimized for random access (vector retrieval, ML training)");
    println!("       - Faster than Parquet for ML use cases");
    println!("       - Apache 2.0 open source");
    println!("       - Used independently of LanceDB (ML training pipelines, data lakes)");
    println!("    2. LanceDB (embedded, OSS):");
    println!("       - pip install lancedb → working in 1 line");
    println!("       - Files on local disk or cloud object store");
    println!("       - Single-process, library-not-server mode");
    println!("       - 7K+ GitHub stars, growing fast");
    println!("    3. LanceDB Cloud (managed, GA 2024):");
    println!("       - Serverless multi-cloud (AWS first)");
    println!("       - Pay per query + storage");
    println!("       - Same API as embedded");
    println!("    4. LanceDB Enterprise:");
    println!("       - On-prem + private cloud deployments");
    println!("       - Multi-region replication, HA");
    println!("       - SOC 2 compliance");
    println!("    5. Multi-modal data storage:");
    println!("       - Vectors + binary blobs (images, audio, video) + metadata in one table");
    println!("       - Zero-copy reads via Arrow");
    println!("       - 'One table, all your AI data'");
    println!("    6. Full-text search + hybrid:");
    println!("       - Native BM25 full-text (Tantivy under the hood)");
    println!("       - Reciprocal Rank Fusion (RRF) for hybrid");
    println!("       - No external search engine needed");
    println!("    7. Versioning + time-travel:");
    println!("       - Lance format has built-in versions (like Git)");
    println!("       - Query data as of a previous state");
    println!("       - Reverse changes, branch + merge");
    println!("       - Critical for training data lineage");
    println!("    8. DuckDB SQL integration:");
    println!("       - Query LanceDB tables via DuckDB SQL");
    println!("       - Combine vector + SQL in one query");
    println!("    9. Embedding functions (LangChain-like):");
    println!("       - OpenAI, Cohere, sentence-transformers, instructor");
    println!("       - Auto-embed on add (similar to Chroma)");
    println!("       - Embedding caching to avoid re-computation");
    println!("    10. Reranking:");
    println!("       - Cross-encoder reranking");
    println!("       - Cohere Rerank, custom rerankers");
    println!("  The Lance format edge:");
    println!("    - Parquet optimized for: full scans (analytics)");
    println!("    - Lance optimized for: random-access (vector retrieval, ML iteration)");
    println!("    - Lance: positional indexes, no row group boundaries");
    println!("    - 2-3x faster random access than Parquet");
    println!("    - Same compression efficiency");
    println!("    - Used by some ML training pipelines independent of LanceDB");
    println!("  The multi-modal angle:");
    println!("    - Most vector DBs: 'store vectors, point to S3 for blobs'");
    println!("    - LanceDB: 'store vectors + blobs together in one table'");
    println!("    - Lance format handles blobs efficiently (no row-group bloat)");
    println!("    - Zero-copy via Arrow → no serialization overhead");
    println!("    - Use case: video frame retrieval, image search with raw bytes");
    println!("  Chang She's pandas heritage:");
    println!("    - Co-created pandas with Wes McKinney");
    println!("    - Deep credibility in Python data community");
    println!("    - 'pandas for AI' framing (LanceDB:AI :: pandas:tabular)");
    println!("    - DX prioritizes Python developers (similar to Chroma)");
    println!("  Integrations:");
    println!("    - lancedb CLI (Python)");
    println!("    - SDKs: Python (lancedb), JS/TS, Rust, Java, .NET (community)");
    println!("    - LangChain + LlamaIndex first-class");
    println!("    - DuckDB SQL integration");
    println!("    - Pandas + Polars + Arrow Flight zero-copy");
    println!("    - OpenAI + Cohere + sentence-transformers embedders");
    println!("    - PyTorch + TensorFlow datasets (Lance format direct)");
    println!("    - Direct cloud storage: S3, GCS, Azure Blob");
    println!("  LanceDB CLI usage:");
    println!("    pip install lancedb                                      # the install");
    println!("    # Via Python:");
    println!("    # import lancedb; db = lancedb.connect('./mydb')");
    println!("    # tbl = db.create_table('docs', data=[{{...}}, ...])");
    println!("    # tbl.search([0.1, 0.2, ...]).limit(10).to_pandas()");
    println!("    lancedb create-table mydb my-coll --schema schema.json");
    println!("    lancedb upsert mydb my-coll --file data.parquet");
    println!("    lancedb search mydb my-coll --vector '[0.1,0.2,...]' --limit=10");
    println!("    lancedb create-index mydb my-coll my_vec --type=IVF_PQ --num-partitions=256 --num-sub-vectors=96");
    println!("    lancedb version-history mydb my-coll                     # time-travel");
    println!("    lancedb restore mydb my-coll --version=5                 # rollback");
    println!("    lancedb fts-index mydb my-coll --field=text              # full-text index");
    println!("    # DuckDB SQL via Python:");
    println!("    # tbl.to_arrow() → DuckDB query");
    println!("  Customers (early traction):");
    println!("    - Cruise (the original use case)");
    println!("    - Various AI startups + research labs");
    println!("    - Multi-modal AI (image/video) workloads");
    println!("    - 7K+ GitHub stars, growing fast");
    println!("    - PyPI downloads growing (smaller than Chroma but growth-y)");
    println!("    - LanceDB Cloud customer count private (recent GA)");
    println!("  Critique: smaller funding than competitors ($11M vs Pinecone $138M)");
    println!("           young company (2022) — operational maturity question");
    println!("           Lance format adoption outside LanceDB still emerging");
    println!("           multi-modal use case still niche vs pure-text RAG (mass market)");
    println!("           DX competition from Chroma (which is in every LangChain tutorial)");
    println!("           production features (replication, HA) less mature than Milvus/Qdrant");
    println!("           Apache Arrow + DuckDB dependencies = supply chain complexity");
    println!("           pure 'vector DB' framing competes against integrated approaches");
    println!("  Differentiator: Lance columnar format (2-3x faster random access than Parquet, optimized for vector retrieval + ML training) + multi-modal data storage (vectors + binary blobs in one table) + built-in versioning + time-travel (Git-like) + Tantivy full-text search + DuckDB SQL integration + Apache Arrow zero-copy + Pandas/Polars native + Chang She founder (co-creator of pandas with Wes McKinney) + Lei Xu (ex-Cruise) + written in Rust + embedded + serverless + enterprise modes + LangChain/LlamaIndex first-class + Apache 2.0 open source + Cruise original use case + $11M raised — the multi-modal AI database built on the open Lance columnar format, with pandas co-creator credibility and the unique combination of vector + blob + SQL + full-text + versioning in one engine");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lancedb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lance(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lance};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lancedb"), "lancedb");
        assert_eq!(basename(r"C:\bin\lancedb.exe"), "lancedb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lancedb.exe"), "lancedb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lance(&["--help".to_string()], "lancedb"), 0);
        assert_eq!(run_lance(&["-h".to_string()], "lancedb"), 0);
        let _ = run_lance(&["--version".to_string()], "lancedb");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lance(&[], "lancedb");
    }
}
