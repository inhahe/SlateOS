#![deny(clippy::all)]

//! weaviate-cli — Slate OS Weaviate (open-source vector + AI-native database, Amsterdam, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wv(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: weaviate [OPTIONS]");
        println!("Weaviate (Slate OS) — open-source vector + AI-native database, modules-based architecture");
        println!();
        println!("Options:");
        println!("  --modules              Modules (text2vec-openai, generative-openai, qna-transformers, etc.)");
        println!("  --hybrid               Hybrid search (BM25 + dense vectors built-in)");
        println!("  --multi-tenancy        Multi-tenancy (isolated tenants in same instance)");
        println!("  --cloud                Weaviate Cloud (managed multi-cloud)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Weaviate 2024 (Slate OS) — weaviate CLI 1.x"); return 0; }
    println!("Weaviate 2024 (Slate OS) — Open-Source AI-Native Vector Database");
    println!("  Vendor: Weaviate B.V. (Amsterdam, Netherlands — private)");
    println!("  Founders: Bob van Luijt (CEO) + Etienne Dilocker (CTO), 2019");
    println!("          Bob van Luijt: ex-Apple designer + entrepreneur, conceived semantic search bet");
    println!("          Etienne Dilocker: engineer, built early Weaviate prototype");
    println!("          Originally called 'SeMI Technologies' (Semantic Machine Intelligence)");
    println!("          Renamed Weaviate matches the open-source database");
    println!("          Pre-dates ChatGPT — built thesis on semantic search before LLMs went mainstream");
    println!("  Funding:");
    println!("         Total raised: ~$68M");
    println!("         Series B Mar 2023: $50M at $200M+ valuation (post-ChatGPT)");
    println!("         Investors: Index Ventures, NEA, Battery Ventures, Cortical Ventures");
    println!("         Revenue private but growing fast — open source download numbers strong");
    println!("  Strategic position: 'open-source vector DB + AI-native + modules ecosystem':");
    println!("                    pitch: 'open-source, modular, do everything in the DB — vectors + hybrid + generative'");
    println!("                    target: AI/ML teams who want OSS option + flexibility");
    println!("                    primary competitor: Pinecone (closed-source managed), Qdrant, Milvus, Chroma");
    println!("                    secondary: pgvector, Elasticsearch, MongoDB Atlas");
    println!("                    Weaviate's wedge: open-source + modules architecture + GraphQL API + early hybrid search");
    println!("                    AI-native: 'do generation + retrieval + reasoning in the DB'");
    println!("                    Apache 2.0 license = customer-friendly");
    println!("  Pricing (open-source + cloud + enterprise):");
    println!("    Weaviate Open Source: free, Apache 2.0");
    println!("    Weaviate Cloud Standard: $25/mo entry tier (managed)");
    println!("    Weaviate Cloud Pro: usage-based + 99.9% SLA");
    println!("    Weaviate Cloud Enterprise: dedicated + private cluster + SLAs");
    println!("    Sandbox: free shared instance for testing");
    println!("    typically cheaper than Pinecone for large-vector workloads on Cloud");
    println!("  Architecture (modules + AI-native):");
    println!("    - Written in Go (single binary, easy to deploy)");
    println!("    - HNSW index (default), with PQ (Product Quantization) compression");
    println!("    - Module system: pluggable embedders + generators + Q&A + classifiers");
    println!("    - Modules: text2vec-openai, text2vec-cohere, generative-openai, qna-transformers, ref2vec-centroid, etc.");
    println!("    - Multi-shard distributed (sharding by class)");
    println!("    - GraphQL + gRPC + REST APIs");
    println!("    - Multi-tenancy: per-tenant isolation with shared resource pool");
    println!("    - BM25 + dense vector hybrid search");
    println!("  Product portfolio:");
    println!("    1. Weaviate Core (open-source):");
    println!("       - Vector + hybrid + keyword search");
    println!("       - Object storage (vectors + metadata + raw data)");
    println!("       - Schema definition with classes");
    println!("       - 11K+ GitHub stars, 50K+ Docker pulls/month");
    println!("    2. Weaviate Cloud (managed):");
    println!("       - Multi-cloud (AWS, GCP, Azure)");
    println!("       - Sandbox + Standard + Pro + Enterprise tiers");
    println!("       - Auto-scaling + backups");
    println!("    3. Modules ecosystem:");
    println!("       - Vectorizer modules: text2vec-openai, text2vec-cohere, text2vec-huggingface, text2vec-transformers (self-hosted), text2vec-jina");
    println!("       - Generator modules: generative-openai, generative-anthropic, generative-cohere, generative-palm");
    println!("       - Reader/QnA modules: qna-transformers, qna-openai");
    println!("       - Multi-modal: img2vec-neural, multi2vec-clip, multi2vec-bind");
    println!("       - Reranker modules: reranker-cohere, reranker-transformers");
    println!("       - Spell-check: spellcheck-transformers");
    println!("    4. Multi-modal:");
    println!("       - Text + image + audio + video vectors");
    println!("       - CLIP integration");
    println!("       - ImageBind integration (Meta's multi-modal model)");
    println!("    5. Generative search:");
    println!("       - Query → retrieve → generate (all in one API call)");
    println!("       - Calls LLM under hood with retrieved context");
    println!("       - 'RAG built into the DB' pattern");
    println!("    6. Hybrid search:");
    println!("       - BM25 + vector with alpha-weighted fusion");
    println!("       - One of first vector DBs with native hybrid");
    println!("    7. Multi-tenancy:");
    println!("       - Per-tenant isolation");
    println!("       - Critical for SaaS apps with per-customer indexes");
    println!("       - Cost-efficient (shared infrastructure)");
    println!("    8. PQ + BQ compression:");
    println!("       - Product Quantization (PQ): 4-32x memory reduction");
    println!("       - Binary Quantization (BQ): more aggressive 32x");
    println!("       - Trade-off: speed/memory vs recall");
    println!("    9. Replication + sharding:");
    println!("       - Multi-replica HA");
    println!("       - Class-based sharding");
    println!("       - Raft consensus for metadata");
    println!("    10. Cross-references + Refs (the GraphQL DNA):");
    println!("       - Objects can reference other objects");
    println!("       - Walk graph relations in queries");
    println!("       - Knowledge graph + vector hybrid use cases");
    println!("  The modules architecture (the differentiator):");
    println!("    - Most vector DBs: 'send us vectors, we'll store + search'");
    println!("    - Weaviate: 'send us text/images, we'll vectorize + store + search + generate'");
    println!("    - Modules attach embedding/generation models to schema classes");
    println!("    - One API: 'ask question, get answer with sources'");
    println!("    - Reduces application code (no separate embedding step)");
    println!("    - Trade-off: more vendor lock-in to Weaviate model");
    println!("  The AI-native pitch:");
    println!("    - 'AI-native' = LLM + retrieval + reasoning integrated");
    println!("    - Generative search: query → retrieve → LLM → response (one API)");
    println!("    - Reranker support: cross-encoders for second-stage relevance");
    println!("    - Spell-check + classification in DB");
    println!("    - Vision: DB is the agent's working memory");
    println!("  Integrations:");
    println!("    - weaviate CLI (Python wrapper around REST/gRPC)");
    println!("    - SDKs: Python (weaviate-python-client v4), JS/TS, Go, Java");
    println!("    - LangChain + LlamaIndex + Haystack (deepset)");
    println!("    - OpenAI + Cohere + Anthropic + Hugging Face embedders");
    println!("    - Airflow + Dagster connectors");
    println!("    - Snowflake + Databricks integrations");
    println!("    - Kubernetes operator (weaviate-helm)");
    println!("  Weaviate CLI usage:");
    println!("    docker run -p 8080:8080 -p 50051:50051 cr.weaviate.io/semitechnologies/weaviate:latest");
    println!("    # Or via Python:");
    println!("    # weaviate.connect_to_local() / weaviate.connect_to_wcs(...)");
    println!("    weaviate-cli ping http://localhost:8080");
    println!("    weaviate-cli schema-create --class-name=Article --properties=title:text,content:text --vectorizer=text2vec-openai");
    println!("    weaviate-cli data-import --class=Article --file=articles.json");
    println!("    weaviate-cli query --class=Article --near-text='AI databases' --limit=5");
    println!("    weaviate-cli backup-create --backend=s3 --include=Article");
    println!("    weaviate-cli tenants-create --class=Article --names=tenant_a,tenant_b");
    println!("    # GraphQL example:");
    println!("    # {{ Get {{ Article(nearText: {{concepts: [\"vector search\"]}} limit: 3) {{ title }} }} }}");
    println!("  Customers (open source + managed):");
    println!("    - Open source: many startups + research orgs (download-heavy)");
    println!("    - Cloud: Stack Overflow (semantic search), Vodafone, Instabase, Unstructured.io");
    println!("    - 50K+ Docker pulls/month, 11K+ GitHub stars");
    println!("    - Use cases: semantic search, RAG, recommendation, content moderation");
    println!("  Critique: smaller go-to-market than Pinecone");
    println!("           modules architecture sometimes confusing to newcomers");
    println!("           GraphQL API + gRPC API + REST = three ways = complexity");
    println!("           Postgres + pgvector competition for 'good enough' use cases");
    println!("           Qdrant + Milvus also OSS — crowded OSS vector DB space");
    println!("           generative search modules call out to OpenAI/etc — latency adds up");
    println!("           v4 Python SDK breaking changes 2023 caused migration friction");
    println!("           closed-source competitors (Pinecone) have more enterprise traction");
    println!("  Differentiator: open-source (Apache 2.0) + modules architecture (vectorize + generate + Q&A in DB) + GraphQL + gRPC + REST APIs + native hybrid search (BM25 + dense from early on) + multi-tenancy + multi-modal (CLIP/ImageBind) + generative search (RAG in one API call) + PQ/BQ compression + Go single-binary deployment + Bob van Luijt + Etienne Dilocker founders 2019 (pre-ChatGPT) + 11K+ GitHub stars + 50K+ Docker pulls/month + $68M raised + Stack Overflow customer — the AI-native open-source vector database with the deepest module ecosystem (vectorizers + generators + rerankers + Q&A all pluggable) and the strongest 'DB-does-it-all' philosophy");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "weaviate".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wv(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wv};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/weaviate"), "weaviate");
        assert_eq!(basename(r"C:\bin\weaviate.exe"), "weaviate.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("weaviate.exe"), "weaviate");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wv(&["--help".to_string()], "weaviate"), 0);
        assert_eq!(run_wv(&["-h".to_string()], "weaviate"), 0);
        let _ = run_wv(&["--version".to_string()], "weaviate");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wv(&[], "weaviate");
    }
}
