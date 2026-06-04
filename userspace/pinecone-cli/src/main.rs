#![deny(clippy::all)]

//! pinecone-cli — OurOS Pinecone (managed vector database, NYC, private — the category creator)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pinecone [OPTIONS]");
        println!("Pinecone (OurOS) — fully managed vector database for AI applications");
        println!();
        println!("Options:");
        println!("  --serverless           Serverless indexes (pay per query + storage)");
        println!("  --pods                 Pod-based indexes (dedicated, predictable cost)");
        println!("  --hybrid               Hybrid search (sparse + dense vectors)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Pinecone 2024 (OurOS) — pc CLI 1.x"); return 0; }
    println!("Pinecone 2024 (OurOS) — Fully Managed Vector Database");
    println!("  Vendor: Pinecone Systems, Inc. (New York, NY — private)");
    println!("  Founders: Edo Liberty, 2019 (sole founder, CEO)");
    println!("          Edo Liberty: ex-Sr Director Research at AWS (Amazon SageMaker), ex-Yahoo Research");
    println!("          Yale CS PhD — research in randomized algorithms + streaming + dimensionality reduction");
    println!("          'Created the category' — Pinecone was first fully-managed vector DB SaaS (2021 GA)");
    println!("          Bet: LLM-driven RAG would explode = vectors became infrastructure");
    println!("          Vindicated by ChatGPT moment Nov 2022");
    println!("  Funding:");
    println!("         Total raised: ~$138M");
    println!("         Series B Apr 2023: $100M at $750M valuation (post-ChatGPT boom)");
    println!("         Investors: Andreessen Horowitz, Menlo Ventures, Wing Venture Capital");
    println!("         Revenue rumored ~$50-100M ARR (2024 estimate)");
    println!("  Strategic position: 'the original managed vector DB — easy + scalable + RAG-ready':");
    println!("                    pitch: 'put your embeddings in Pinecone, do vector search at scale — no ops'");
    println!("                    target: AI/ML teams + RAG applications + recommendation systems");
    println!("                    primary competitor: Weaviate, Qdrant, Chroma, Milvus/Zilliz, LanceDB");
    println!("                    secondary: pgvector, Elasticsearch, MongoDB Atlas vector search");
    println!("                    Pinecone's wedge: first-mover + brand + simplicity + Serverless tier");
    println!("                    challenge: open-source alternatives + integrated DBs (Postgres+pgvector) compress moat");
    println!("                    counter: Serverless tier (2024) drastically cuts costs for sparse-usage RAG apps");
    println!("  Pricing (serverless + pod-based options):");
    println!("    Free / Starter: 1 index, 100K vectors, 1 project (free forever)");
    println!("    Serverless Standard: $0.33/M write units + $8.25/M read units + $0.33/GB-month storage");
    println!("    Serverless Enterprise: 99.95% SLA + private VPC + audit logs");
    println!("    Pod-based: $0.0960/hr (s1.x1) up to $0.4920/hr (p2.x8) — dedicated resources");
    println!("    Pinecone Inference (re-rank/embed): $0.10/M tokens for re-ranking");
    println!("    Serverless typically 10-50x cheaper than pod-based for sparse usage");
    println!("  Architecture (the managed black box):");
    println!("    - Distributed across cells (shards)");
    println!("    - Index types: HNSW (approximate), exact (small), serverless (proprietary)");
    println!("    - Metric: cosine, dot product, Euclidean");
    println!("    - Metadata filtering (filter on top of vector search)");
    println!("    - Namespaces: logical partitioning per index");
    println!("    - Serverless: separates storage (S3-like) from compute, pay-per-use");
    println!("    - Multi-cloud (AWS, GCP, Azure)");
    println!("    - Closed-source (proprietary index implementations)");
    println!("  Product portfolio:");
    println!("    1. Serverless indexes (2024+ flagship):");
    println!("       - True serverless (no capacity planning)");
    println!("       - Pay per write + read + storage independently");
    println!("       - Scales to billions of vectors automatically");
    println!("       - Cold-start latency improved 2024");
    println!("       - Best for: RAG apps with variable traffic");
    println!("    2. Pod-based indexes (the original):");
    println!("       - Dedicated resources (s1/s2 storage-opt, p1/p2 performance-opt)");
    println!("       - Predictable cost + latency");
    println!("       - Best for: production apps with sustained QPS");
    println!("    3. Hybrid search (sparse + dense):");
    println!("       - Combine BM25-like sparse with dense vectors");
    println!("       - Better than pure dense for keyword + semantic mix");
    println!("       - SPLADE / pinecone-text library");
    println!("    4. Metadata filtering:");
    println!("       - Filter vectors by metadata before/during search");
    println!("       - Boolean, numeric, string filters");
    println!("       - Critical for multi-tenant apps");
    println!("    5. Namespaces:");
    println!("       - Logical partitioning of vectors within index");
    println!("       - Per-user, per-doc, per-tenant separation");
    println!("       - Free, included in indexes");
    println!("    6. Pinecone Inference (2024+):");
    println!("       - Embedding models hosted: multilingual-e5-large, others");
    println!("       - Reranking models: bge-reranker-v2-m3, pinecone-rerank-v0");
    println!("       - One-stop shop for RAG pipelines");
    println!("    7. Pinecone Assistant (2024):");
    println!("       - Managed RAG assistant API (chat + retrieval)");
    println!("       - Upload PDFs/text → chat with knowledge base");
    println!("       - Higher-level than raw vector ops");
    println!("    8. Reverse proxy + CDC patterns:");
    println!("       - Direct integrations with embeddings APIs");
    println!("       - Webhook + event-driven ingestion patterns");
    println!("    9. Backups + region replication:");
    println!("       - Point-in-time backups");
    println!("       - Cross-region replication (enterprise)");
    println!("    10. Index sparse vector type:");
    println!("       - Native sparse vectors (Term Frequency + index)");
    println!("       - Combined with dense for hybrid");
    println!("  The Serverless transition (2024):");
    println!("    - Original Pinecone was pod-based (capacity planning required, often expensive)");
    println!("    - Customers complained about idle index cost ($70-100+/mo minimum even unused)");
    println!("    - Open-source alternatives + pgvector grew on cost pressure");
    println!("    - Serverless launched Jan 2024 — 10-50x cheaper for sparse-usage");
    println!("    - Quieted competitive narrative substantially");
    println!("    - Architecture pivot: storage on S3-like, compute on-demand");
    println!("  The 'created the category' positioning:");
    println!("    - 2019-2021: Pinecone shipped before vector DB was a category");
    println!("    - 2022 ChatGPT moment → RAG became dominant pattern → vector DBs became infra");
    println!("    - Pinecone 'won the marketing' in 2022-2023");
    println!("    - 2023+: open-source vector DBs (Weaviate, Qdrant, Chroma, Milvus) caught up technically");
    println!("    - 2024: Postgres pgvector + integrated DBs compress traditional vector DB market");
    println!("    - Pinecone strategy: dominate the managed simple-experience tier");
    println!("  Integrations:");
    println!("    - pc CLI (Python-based)");
    println!("    - SDKs: Python (pinecone-client), JS/TS, Java, Go");
    println!("    - LangChain + LlamaIndex first-class");
    println!("    - Haystack (deepset)");
    println!("    - OpenAI + Anthropic + Cohere embeddings (any embedding model works)");
    println!("    - dbt + Airflow + Dagster connectors");
    println!("    - Snowflake + Databricks notebook integrations");
    println!("    - Direct integrations: Notion, Slack, Confluence for RAG");
    println!("  Pinecone CLI usage:");
    println!("    pinecone login                                           # API key auth");
    println!("    pinecone index create my-index --dimension=1536 --metric=cosine --cloud=aws --region=us-east-1 --type=serverless");
    println!("    pinecone index list");
    println!("    pinecone index describe my-index");
    println!("    pinecone index upsert my-index --vectors '[(\"id1\", [0.1,0.2,...,0.3], {{\"category\": \"news\"}})]'");
    println!("    pinecone index query my-index --vector '[0.1,0.2,...]' --top-k=10 --filter '{{\"category\":\"news\"}}'");
    println!("    pinecone index delete my-index --confirm");
    println!("    pinecone collection create my-backup --source-index=my-index");
    println!("    pinecone inference embed --model=multilingual-e5-large --input='Hello world'");
    println!("    pinecone inference rerank --model=bge-reranker-v2-m3 --query='AI' --documents='doc1,doc2,doc3'");
    println!("  Customers (RAG-heavy AI applications):");
    println!("    - Notion (AI features), Shopify, Gong, Cresta");
    println!("    - HubSpot, Algomo (chatbots)");
    println!("    - Many GenAI startups using as RAG backbone");
    println!("    - 5,000+ customer accounts (2024 estimate)");
    println!("    - 100K+ developer signups since 2022 ChatGPT moment");
    println!("  Critique: Postgres + pgvector erodes 'need a separate vector DB' thesis");
    println!("           open-source vector DBs (Qdrant, Weaviate, Milvus) competitive technically");
    println!("           pod-based pricing was expensive — Serverless addressed");
    println!("           closed-source: customers can't self-host or audit");
    println!("           hybrid search came after Weaviate had it well-established");
    println!("           SingleStore + Snowflake + Databricks adding native vector erodes");
    println!("           AWS OpenSearch + Azure AI Search + Pinecone overlap = customer confusion");
    println!("           Edo Liberty 'sole founder' = bus factor concern + governance");
    println!("           need to keep ahead of integrated DBs which always have data-locality advantage");
    println!("  Differentiator: created the managed vector database category (2021 GA, pre-ChatGPT) + Edo Liberty founder (ex-AWS SageMaker research) + serverless tier (2024 — 10-50x cheaper for sparse usage) + pod-based dedicated option + hybrid search (sparse + dense) + Pinecone Inference (managed embedding + rerank models) + Pinecone Assistant (managed RAG) + namespaces + metadata filtering + 5,000+ customers + Notion/Shopify/Gong/HubSpot users + $138M raised + $750M valuation + LangChain/LlamaIndex first-class integrations — the original managed vector database that won the 2022-2023 GenAI mindshare and is fighting Postgres+pgvector + open-source competition with serverless simplicity");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pinecone".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pinecone"), "pinecone");
        assert_eq!(basename(r"C:\bin\pinecone.exe"), "pinecone.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pinecone.exe"), "pinecone");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pc(&["--help".to_string()], "pinecone"), 0);
        assert_eq!(run_pc(&["-h".to_string()], "pinecone"), 0);
        let _ = run_pc(&["--version".to_string()], "pinecone");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pc(&[], "pinecone");
    }
}
