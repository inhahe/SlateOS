#![deny(clippy::all)]

//! qdrant-cli — OurOS Qdrant (open-source Rust vector DB, Berlin, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qdrant [OPTIONS]");
        println!("Qdrant (OurOS) — open-source Rust vector database with filterable HNSW");
        println!();
        println!("Options:");
        println!("  --filterable-hnsw      Filterable HNSW (filter + vector search combined)");
        println!("  --scalar-quantization  SQ8/SQ4 scalar quantization (4x memory reduction)");
        println!("  --binary-quantization  BQ (32x memory reduction)");
        println!("  --cloud                Qdrant Cloud (managed multi-cloud)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Qdrant 2024 (OurOS) — qdrant CLI 1.x"); return 0; }
    println!("Qdrant 2024 (OurOS) — Open-Source Rust Vector Database");
    println!("  Vendor: Qdrant Solutions GmbH (Berlin, Germany — private)");
    println!("  Founders: Andrey Vasnetsov (CTO) + Andre Zayarni (CEO), 2021");
    println!("          Andrey Vasnetsov: Rust + ML engineer, built Qdrant prototype");
    println!("          Andre Zayarni: ex-CTO of several startups, business side");
    println!("          Built in Rust from day one — performance + safety focus");
    println!("          'Qdrant' (pronounced 'Quadrant') — math reference");
    println!("          Open-source from day one (Apache 2.0)");
    println!("  Funding:");
    println!("         Total raised: ~$37M");
    println!("         Series A Jan 2024: $28M at $100M+ valuation");
    println!("         Investors: Spark Capital, Unusual Ventures, 42CAP, Pareto Holdings");
    println!("         Revenue private but growing — Cloud product 2023+");
    println!("  Strategic position: 'fastest Rust-based vector DB — production-grade open source + cloud':");
    println!("                    pitch: 'highest performance vector DB, written in Rust, open source, with filterable HNSW'");
    println!("                    target: AI/ML teams wanting performance + OSS option");
    println!("                    primary competitor: Pinecone (closed managed), Weaviate, Milvus, Chroma");
    println!("                    secondary: pgvector, Elasticsearch");
    println!("                    Qdrant's wedge: Rust performance + filterable HNSW (filter inside HNSW traversal) + OSS");
    println!("                    benchmark-leading on most public vector DB benchmarks 2023-2024");
    println!("                    'production-grade since day one' — multi-node + replication + WAL early");
    println!("  Pricing (open-source + cloud + enterprise):");
    println!("    Qdrant Open Source: free, Apache 2.0, self-hosted");
    println!("    Qdrant Cloud Free: 1GB cluster, 1 node (always free)");
    println!("    Qdrant Cloud Standard: $0.115/hr (1GB) up to $35+/hr (large)");
    println!("    Qdrant Cloud Premium: dedicated single-tenant + private network");
    println!("    Qdrant Hybrid Cloud: customer-managed K8s (BYOC)");
    println!("    Qdrant Private Cloud: fully self-hosted enterprise license");
    println!("    typically 30-50% cheaper than Pinecone for equivalent workloads");
    println!("  Architecture (the Rust-built engine):");
    println!("    - Written in Rust (single static binary)");
    println!("    - HNSW index (Hierarchical Navigable Small World)");
    println!("    - Filterable HNSW: filter conditions inline during graph traversal");
    println!("    - Multi-vector per point support (named vectors)");
    println!("    - Sparse vectors (BM25-like) + dense vectors hybrid");
    println!("    - Quantization: scalar (SQ8/SQ4), binary (BQ), product (PQ)");
    println!("    - Sharding + replication (Raft consensus)");
    println!("    - WAL + persistent storage (rocksdb-based segments)");
    println!("    - gRPC + REST APIs");
    println!("  Product portfolio:");
    println!("    1. Qdrant (open-source core):");
    println!("       - Rust, single binary, Docker image");
    println!("       - Filterable HNSW (the differentiator)");
    println!("       - 19K+ GitHub stars, 5M+ Docker pulls");
    println!("    2. Qdrant Cloud (managed):");
    println!("       - Multi-cloud (AWS, GCP, Azure)");
    println!("       - Free Forever tier (1GB)");
    println!("       - Auto-scaling clusters");
    println!("    3. Filterable HNSW (the technical innovation):");
    println!("       - Most vector DBs: filter after vector search (post-filter)");
    println!("       - Qdrant: filter conditions affect graph traversal (in-filter)");
    println!("       - Result: filters work correctly even at small top-k");
    println!("       - Critical for: 'find similar AI papers by author=Bob'");
    println!("    4. Quantization (memory reduction):");
    println!("       - Scalar Quantization (SQ): 4-8x memory reduction");
    println!("       - Binary Quantization (BQ): 32x reduction");
    println!("       - Product Quantization (PQ): variable");
    println!("       - Combine with HNSW for fast approx");
    println!("    5. Hybrid Cloud (BYOC):");
    println!("       - Qdrant Hybrid: managed control plane, customer K8s for data plane");
    println!("       - Data never leaves customer VPC");
    println!("       - GA 2023, popular with enterprise security teams");
    println!("    6. Multi-vector support:");
    println!("       - One point can have multiple named vectors");
    println!("       - 'text_embedding' + 'image_embedding' on same record");
    println!("       - Query different vectors per use case");
    println!("    7. Sparse vector support:");
    println!("       - Native sparse vectors (SPLADE-style)");
    println!("       - Combined with dense for hybrid search");
    println!("    8. Snapshots + backups:");
    println!("       - Point-in-time snapshots");
    println!("       - Per-collection backups");
    println!("       - S3-compatible snapshot storage");
    println!("    9. Web UI (Qdrant Cloud + self-hosted):");
    println!("       - Cluster + collection management");
    println!("       - Vector visualization");
    println!("       - REST API explorer");
    println!("    10. FastEmbed (Qdrant's embedding library):");
    println!("       - Rust-Python library, no PyTorch/TF runtime needed");
    println!("       - Sentence-transformers compatible models");
    println!("       - ~50x faster than sentence-transformers for inference");
    println!("       - 'embed in seconds, not minutes'");
    println!("  Filterable HNSW (the key innovation):");
    println!("    - HNSW graph: navigable small world, fast nearest-neighbor search");
    println!("    - Standard HNSW: 'find top-K, then filter by metadata'");
    println!("    - Problem: top-K may not contain any matching items (e.g. filter user=Alice)");
    println!("    - Qdrant's HNSW: graph traversal considers filter, only visits matching neighbors");
    println!("    - Result: correct top-K respecting filter, no over-search needed");
    println!("    - This is a non-trivial engineering accomplishment");
    println!("  The Rust performance angle:");
    println!("    - Most vector DBs written in C++/Go/Java");
    println!("    - Qdrant in Rust: memory safety + concurrency without GC");
    println!("    - SIMD-optimized distance computations");
    println!("    - On most public benchmarks (e.g. ann-benchmarks): Qdrant in top 3");
    println!("    - Lower memory overhead than Java/Go alternatives");
    println!("  Integrations:");
    println!("    - qdrant CLI (Python wrapper)");
    println!("    - SDKs: Python (qdrant-client), JS/TS, Go, Rust, Java, .NET");
    println!("    - LangChain + LlamaIndex + Haystack (deepset)");
    println!("    - OpenAI + Cohere + Anthropic + Hugging Face embedders");
    println!("    - Airflow + Dagster + Apache Beam connectors");
    println!("    - Snowflake + Databricks notebooks");
    println!("    - Kubernetes operator (qdrant-operator)");
    println!("    - FastEmbed library for in-process embedding");
    println!("  Qdrant CLI usage:");
    println!("    docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant:latest");
    println!("    # Via Python:");
    println!("    # qdrant_client = QdrantClient(host='localhost', port=6333)");
    println!("    qdrant-cli ping http://localhost:6333");
    println!("    qdrant-cli collection create my-coll --vector-size=1536 --distance=Cosine");
    println!("    qdrant-cli collection list");
    println!("    qdrant-cli point upsert my-coll --id=1 --vector='[0.1,0.2,...]' --payload='{{\"category\":\"news\"}}'");
    println!("    qdrant-cli point search my-coll --vector='[0.1,0.2,...]' --limit=10 --filter='{{\"must\":[{{\"key\":\"category\",\"match\":{{\"value\":\"news\"}}}}]}}'");
    println!("    qdrant-cli snapshot create my-coll --location=/snapshots/");
    println!("    qdrant-cli cluster info                                  # multi-node info");
    println!("    qdrant-cli alias create my-alias --collection=my-coll");
    println!("    # FastEmbed embedding:");
    println!("    fastembed embed --model=BAAI/bge-small-en-v1.5 'Hello world'");
    println!("  Customers (OSS + managed):");
    println!("    - GitLab (vector search across docs)");
    println!("    - Bayer (pharma research)");
    println!("    - Voyager (Bain Capital portfolio)");
    println!("    - DAGsHub (ML platform)");
    println!("    - Many AI/ML startups (RAG backbone)");
    println!("    - 19K+ GitHub stars, 5M+ Docker pulls");
    println!("    - 1,000s of Cloud accounts (private metrics)");
    println!("  Critique: smaller go-to-market than Pinecone/Weaviate");
    println!("           Berlin EU-centric — less US enterprise GTM presence");
    println!("           Postgres pgvector erodes need for separate vector DB");
    println!("           crowded OSS vector DB market (Weaviate, Milvus, Chroma, LanceDB)");
    println!("           v1.0 API stability good, but breaking changes in early versions");
    println!("           ecosystem of pre-built integrations smaller than Pinecone");
    println!("           FastEmbed is great but separate concern from DB");
    println!("  Differentiator: written in Rust (single binary, no GC, SIMD-optimized) + filterable HNSW (filter during graph traversal — not just post-filter) + scalar/binary/product quantization for 4-32x memory reduction + multi-vector + sparse + dense hybrid + sharding + Raft replication + Apache 2.0 open source + Andrey Vasnetsov + Andre Zayarni founders 2021 + 19K+ GitHub stars + 5M+ Docker pulls + Qdrant Cloud + Hybrid Cloud (BYOC) + FastEmbed (50x faster embedding library) + GitLab/Bayer customers + $37M raised + Berlin engineering — the performance-leading open-source Rust vector database that solves the filter+vector search problem with filterable HNSW and ranks top-3 on public benchmarks");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qdrant".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_qd(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_qd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/qdrant"), "qdrant");
        assert_eq!(basename(r"C:\bin\qdrant.exe"), "qdrant.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("qdrant.exe"), "qdrant");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_qd(&["--help".to_string()], "qdrant"), 0);
        assert_eq!(run_qd(&["-h".to_string()], "qdrant"), 0);
        assert_eq!(run_qd(&["--version".to_string()], "qdrant"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_qd(&[], "qdrant"), 0);
    }
}
