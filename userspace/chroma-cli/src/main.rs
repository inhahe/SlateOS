#![deny(clippy::all)]

//! chroma-cli — Slate OS Chroma (open-source embedding database, San Francisco, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chroma [OPTIONS]");
        println!("Chroma (Slate OS) — open-source embedding database, AI-application-first DX");
        println!();
        println!("Options:");
        println!("  --ephemeral            In-memory client (default for prototyping)");
        println!("  --persistent           Persistent local client (SQLite + duckdb)");
        println!("  --http                 HTTP client (for self-hosted server)");
        println!("  --cloud                Chroma Cloud (managed, 2024+)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Chroma 2024 (Slate OS) — chromadb CLI 0.5.x"); return 0; }
    println!("Chroma 2024 (Slate OS) — Open-Source Embedding Database for AI Apps");
    println!("  Vendor: Chroma Inc. (San Francisco, CA — private)");
    println!("  Founders: Jeff Huber + Anton Troynikov, 2022");
    println!("          Jeff Huber: ex-Google ML, ex-product at Standard Cyborg, ex-Two Sigma");
    println!("          Anton Troynikov: ex-Standard Cyborg + ML engineer (left in 2024)");
    println!("          Founded right around ChatGPT moment (timely)");
    println!("          'AI-application-first DX' — Python-native, beautiful API");
    println!("          'The best DX of any vector DB' is consensus framing in dev community");
    println!("  Funding:");
    println!("         Total raised: ~$38M");
    println!("         Seed Apr 2023: $18M (Quiet Capital + Naval Ravikant + others)");
    println!("         Series A Sep 2023: $20M");
    println!("         Investors: Quiet Capital, Naval Ravikant, Astasia Myers, Index Ventures (B)");
    println!("         Revenue ramp dependent on Chroma Cloud (2024 GA)");
    println!("  Strategic position: 'simplest vector DB to start with — pip install chromadb, done':");
    println!("                    pitch: 'the AI-application database — beautiful Python API, embedded or hosted'");
    println!("                    target: Python AI developers + LangChain users + RAG prototypers");
    println!("                    primary competitor: Pinecone (managed), Weaviate, Qdrant, LanceDB, pgvector");
    println!("                    secondary: Postgres + pgvector, SQLite + sqlite-vec");
    println!("                    Chroma's wedge: dev experience + 'pip install chromadb' instantly works");
    println!("                    Famous in: every LangChain RAG tutorial uses Chroma as default");
    println!("                    LangChain effect: huge install base from being default RAG store");
    println!("                    'Single command to start, single command to scale' aspirational");
    println!("  Pricing (open-source + cloud):");
    println!("    Chroma Open Source: free, Apache 2.0 (embedded + self-hosted)");
    println!("    Chroma Cloud (2024+, in beta during 2024): tiered consumption pricing");
    println!("    Self-hosted: free, Docker image");
    println!("    For most prototypes: embedded mode = zero dollars");
    println!("    For production: Chroma Cloud or self-hosted on K8s");
    println!("  Architecture (the DX-focused design):");
    println!("    - Written in Python + Rust (Rust core 2024 'distributed' rewrite)");
    println!("    - Embedded mode: SQLite + DuckDB + hnswlib backend");
    println!("    - HTTP server mode: client-server with same API");
    println!("    - Collections: logical groupings of embeddings");
    println!("    - HNSW for ANN search");
    println!("    - Metadata filtering (where clauses with $eq, $gt, $lt, $in, etc.)");
    println!("    - Embedding functions: pluggable (OpenAI, Cohere, sentence-transformers, custom)");
    println!("    - 'AI-application database' positioning");
    println!("  Product portfolio:");
    println!("    1. Chroma Embedded (the DX win):");
    println!("       - pip install chromadb → import chromadb → ready");
    println!("       - In-memory or persistent (SQLite-backed)");
    println!("       - Single-process, embedded in app");
    println!("       - Perfect for: prototypes, demos, small RAG apps");
    println!("       - 13K+ GitHub stars, 15M+ PyPI downloads/month");
    println!("    2. Chroma HTTP Server (self-hosted):");
    println!("       - Docker container, single binary equivalent");
    println!("       - Same Python API, just point at server URL");
    println!("       - Production deployment option");
    println!("    3. Chroma Cloud (2024+):");
    println!("       - Fully managed, serverless");
    println!("       - In beta during 2024, GA pending");
    println!("       - Pricing similar to Pinecone Serverless");
    println!("       - Same API as embedded/self-hosted (drop-in)");
    println!("    4. Embedding Functions:");
    println!("       - OpenAI (text-embedding-3-small/large/ada-002)");
    println!("       - Cohere (embed-english-v3.0)");
    println!("       - sentence-transformers (all-MiniLM-L6-v2, etc.)");
    println!("       - Hugging Face (any HF model)");
    println!("       - Instructor + Jina + Roboflow + custom callable");
    println!("    5. Collections:");
    println!("       - Logical grouping of embeddings");
    println!("       - Per-collection metadata schema");
    println!("       - Per-collection embedding function");
    println!("    6. Metadata filtering:");
    println!("       - $eq, $ne, $gt, $gte, $lt, $lte, $in, $nin");
    println!("       - $and, $or boolean composition");
    println!("       - Filter applied during/after vector search");
    println!("    7. Document storage:");
    println!("       - Store raw document text alongside vectors");
    println!("       - Auto-embed documents on add");
    println!("       - Auto-rehydrate on query");
    println!("    8. Multi-tenancy + databases (2024+):");
    println!("       - Tenants + databases hierarchy");
    println!("       - Per-tenant isolation");
    println!("    9. Distributed Chroma (2024 rewrite):");
    println!("       - Rust-based distributed backend");
    println!("       - Scales to billions of vectors");
    println!("       - Powering Chroma Cloud");
    println!("    10. Integration ecosystem:");
    println!("       - LangChain default vector store");
    println!("       - LlamaIndex default vector store");
    println!("       - Haystack + Embedchain + others use Chroma");
    println!("  The LangChain default effect:");
    println!("    - LangChain tutorials default to Chroma for RAG examples");
    println!("    - Every 'build a RAG app' tutorial includes Chroma");
    println!("    - Resulted in massive install base early on");
    println!("    - 15M+ PyPI downloads/month (most of any vector DB)");
    println!("    - Trade-off: many users are tutorials/prototypes, not production");
    println!("    - Cloud product 2024+ aims to monetize this base");
    println!("  The 'AI-application database' framing:");
    println!("    - Most vector DBs: 'put vectors in, do vector ops'");
    println!("    - Chroma: 'embed-and-store-and-query in one API'");
    println!("    - 'collection.add(documents=[...])' — auto-embeds via function");
    println!("    - 'collection.query(query_texts=[...])' — auto-embeds query");
    println!("    - Hides vector math from app developer");
    println!("    - Trade-off: less control, more opinionated");
    println!("  Distributed Chroma rewrite (2024):");
    println!("    - 2022-2023: Chroma was Python + SQLite + DuckDB + hnswlib (embedded)");
    println!("    - 2024: Rust rewrite of core for distributed cloud");
    println!("    - Powers Chroma Cloud");
    println!("    - Same Python API surface preserved (no breaking changes)");
    println!("    - Engineering: significant — full distributed system in Rust");
    println!("  Integrations:");
    println!("    - chromadb Python package (the dominant interface)");
    println!("    - SDKs: Python, JS/TS (experimental Go, Ruby)");
    println!("    - LangChain + LlamaIndex + Haystack + Embedchain default");
    println!("    - OpenAI + Cohere + Anthropic embedders (plug-and-play)");
    println!("    - Sentence-transformers + Hugging Face for local embeddings");
    println!("    - Airflow + Dagster connectors");
    println!("    - K8s Helm chart for self-hosted");
    println!("  Chroma CLI usage:");
    println!("    pip install chromadb                                     # the install");
    println!("    # Embedded mode (in Python):");
    println!("    # import chromadb; client = chromadb.Client()");
    println!("    chromadb run                                             # start server");
    println!("    chromadb run --path /data/chroma --port 8000");
    println!("    # Via Python client:");
    println!("    # client.create_collection('my-coll', embedding_function=OpenAIEmbeddingFunction(api_key='...'))");
    println!("    # collection.add(documents=['doc1', 'doc2'], ids=['1', '2'])");
    println!("    # collection.query(query_texts=['similar to doc1'], n_results=5)");
    println!("    # collection.update / collection.delete / collection.modify");
    println!("    chroma utils status                                      # server status");
    println!("    chroma utils reset                                       # nuke all collections");
    println!("  Customers (mostly Python AI devs + startups + prototypes):");
    println!("    - Every LangChain tutorial reader");
    println!("    - GenAI startups using as RAG backbone");
    println!("    - Researchers + universities");
    println!("    - 15M+ PyPI downloads/month");
    println!("    - 13K+ GitHub stars");
    println!("    - Chroma Cloud customer count private (early beta)");
    println!("  Critique: many users are tutorials/prototypes — production cohort smaller");
    println!("           competition: every Postgres+pgvector, sqlite-vec, lancedb is 'simple too'");
    println!("           Chroma Cloud late vs Pinecone Serverless");
    println!("           DX-first means production features (sharding, replication) lagged early");
    println!("           Anton Troynikov departure 2024 = co-founder transition risk");
    println!("           ecosystem of features behind Weaviate/Qdrant");
    println!("           pure DX moat vulnerable to ecosystem improvements elsewhere");
    println!("           closed-source Cloud creates dual-product complexity");
    println!("  Differentiator: best Python DX of any vector DB ('pip install chromadb → working in 1 line') + default vector store in LangChain + LlamaIndex tutorials (massive install base) + 15M+ PyPI downloads/month + 13K+ GitHub stars + embedded + self-hosted + cloud modes share same API + auto-embedding (collection.add(documents) just works) + pluggable embedding functions (OpenAI/Cohere/HF) + Apache 2.0 open source + Jeff Huber + Anton Troynikov founders 2022 + Naval Ravikant investor + $38M raised + 2024 Rust distributed rewrite powering Chroma Cloud — the 'AI-application database' with the most beloved DX in vector DB land, riding the LangChain tutorial wave to dominate developer mindshare while building distributed production engine in Rust");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "chroma".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ch(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ch};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/chroma"), "chroma");
        assert_eq!(basename(r"C:\bin\chroma.exe"), "chroma.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("chroma.exe"), "chroma");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ch(&["--help".to_string()], "chroma"), 0);
        assert_eq!(run_ch(&["-h".to_string()], "chroma"), 0);
        let _ = run_ch(&["--version".to_string()], "chroma");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ch(&[], "chroma");
    }
}
