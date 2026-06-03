#![deny(clippy::all)]

//! vespa-cli — OurOS Vespa (Yahoo-origin big-data search + ranking + vector platform, Trondheim NO)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vespa(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vespa [OPTIONS]");
        println!("Vespa (OurOS) — big-data search + ranking + vectors + LLM platform (Yahoo open-source)");
        println!();
        println!("Options:");
        println!("  --search               Full-text + structured search (Yahoo origin)");
        println!("  --ranking              First-stage + second-stage ranking with ML");
        println!("  --vector               Dense + sparse vector search");
        println!("  --cloud                Vespa Cloud (managed by Vespa.ai)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Vespa 2024 (OurOS) — vespa-cli 8.x"); return 0; }
    println!("Vespa 2024 (OurOS) — Big-Data Serving Engine (Search + Ranking + Vectors + LLM)");
    println!("  Vendor: Vespa.ai (Trondheim, Norway — private, Yahoo spin-off Aug 2023)");
    println!("  Origins:");
    println!("         FAST Search & Transfer founded 1997 (Norway) — acquired by Microsoft 2008 ($1.2B)");
    println!("         Yahoo Search engineers built Vespa from scratch (2003+) for Yahoo search/ranking");
    println!("         Vespa open-sourced by Yahoo 2017 (Apache 2.0)");
    println!("         Powers Yahoo Search, Yahoo Mail, Yahoo Finance, Flickr — billions of queries/day");
    println!("         Aug 2023: spun out as Vespa.ai (separate company, Yahoo investor)");
    println!("  Founders: Jon Bratseth (CEO), 2023 spin-off");
    println!("          Jon Bratseth: Vespa architect at Yahoo for 17+ years, principal architect");
    println!("          Vespa team: ~50 engineers, ~80% based in Trondheim, Norway");
    println!("          'Built and battle-tested at Yahoo for 20+ years before going independent'");
    println!("  Funding:");
    println!("         Total raised: $31M");
    println!("         Series A Sep 2023: $31M Blossom Capital lead");
    println!("         Yahoo retained minority stake in spin-off");
    println!("         Revenue private — Vespa Cloud growing 2024");
    println!("  Strategic position: 'big-data serving — search + ranking + vectors + ML inference, sub-100ms':");
    println!("                    pitch: 'big-data search + ranking + vector + ML in one engine — proven at Yahoo scale'");
    println!("                    target: large-scale search + recommendation + RAG + ad-serving workloads");
    println!("                    primary competitor: Elasticsearch (search), Pinecone (vector), Solr, OpenSearch");
    println!("                    secondary: Milvus + Weaviate + Qdrant (pure vector)");
    println!("                    Vespa's wedge: combined search + ranking + vector + ML in one engine + battle-tested");
    println!("                    'Most mature unified search + ranking + vector platform'");
    println!("                    Bing Search (Microsoft) reportedly uses Vespa-inspired tech");
    println!("  Pricing (OSS + Cloud + Enterprise):");
    println!("    Vespa OSS: free, Apache 2.0");
    println!("    Vespa Cloud Trial: free");
    println!("    Vespa Cloud Production: ~$1/hr per node + traffic");
    println!("    Vespa Enterprise: custom (on-prem + dedicated support)");
    println!("    notably more enterprise-priced than vector-only OSS DBs");
    println!("    justified by: complete search + ranking platform replacing 3-4 systems");
    println!("  Architecture (the proven Yahoo-scale platform):");
    println!("    - Written in C++ (perf-critical engine), Java (control + APIs)");
    println!("    - Content nodes: store documents + indexes + execute queries");
    println!("    - Container nodes: route + rank + transform responses");
    println!("    - Config server cluster: cluster-wide configuration");
    println!("    - Document type schemas (structured + unstructured)");
    println!("    - First-stage ranking: matching + recall (similarity, BM25)");
    println!("    - Second-stage ranking: ML ranking models (LightGBM, ONNX, TensorFlow)");
    println!("    - Stateless query/result containers (linearly scalable)");
    println!("    - Designed for: serving (low latency) not analytics (high throughput)");
    println!("  Product portfolio:");
    println!("    1. Vespa Engine (OSS core):");
    println!("       - Apache 2.0 open source");
    println!("       - 5K+ GitHub stars (lower than newer vector DBs due to being long-established + less hype)");
    println!("       - Battle-tested at Yahoo (billions of queries/day for 20+ years)");
    println!("    2. Vespa Cloud (managed):");
    println!("       - Multi-cloud (AWS, GCP, Azure)");
    println!("       - Auto-scaling + zero ops");
    println!("       - 99.99% SLA enterprise tier");
    println!("    3. Search (the original use case):");
    println!("       - Full-text search (BM25, BM25F)");
    println!("       - Structured search (field-level constraints)");
    println!("       - Boolean + range + geo queries");
    println!("       - Stemming + tokenization (multi-language)");
    println!("    4. Vector search:");
    println!("       - HNSW + nearest-neighbor approximate");
    println!("       - Sparse + dense vector hybrid");
    println!("       - ColBERT-style late interaction (multi-vector)");
    println!("       - Tensor support (multi-dim arrays for ML)");
    println!("       - In-memory + memory-mapped indexes");
    println!("    5. Ranking (the differentiator):");
    println!("       - First-stage: cheap recall (BM25, ANN)");
    println!("       - Second-stage: ML re-ranking (LightGBM, XGBoost, ONNX models)");
    println!("       - Phased ranking expressions (executes only what's needed)");
    println!("       - In-engine ML inference (no separate ML service)");
    println!("    6. ML model serving:");
    println!("       - ONNX runtime built-in");
    println!("       - LightGBM + XGBoost models native");
    println!("       - TensorFlow models via ONNX export");
    println!("       - Run ML at query time, in the engine, per document");
    println!("    7. Tensor type system:");
    println!("       - First-class N-dim tensor support");
    println!("       - Tensor expressions in ranking");
    println!("       - Multi-vector models (ColBERT, late-interaction)");
    println!("       - Mathematically elegant");
    println!("    8. Streams + lineage:");
    println!("       - Update + delete with strong consistency");
    println!("       - 1000s of updates/sec/node");
    println!("       - Lineage tracking for ranking changes");
    println!("    9. Schema language (.sd files):");
    println!("       - Define fields, indexing, ranking profiles");
    println!("       - YAML-like declarative configuration");
    println!("       - Per-field tokenization + indexing options");
    println!("    10. Query language (YQL):");
    println!("       - Yahoo Query Language (YQL)");
    println!("       - SQL-inspired but search-aware");
    println!("       - Boolean + ranking + grouping combined");
    println!("  The Yahoo heritage (the battle-test):");
    println!("    - Vespa powers Yahoo Search (40+ countries)");
    println!("    - Powers Yahoo Mail search (billions of emails)");
    println!("    - Powers Yahoo Finance search");
    println!("    - Powers Flickr photo search");
    println!("    - Verizon Media used Vespa for ad-serving");
    println!("    - 20+ years of production at billions of queries/day");
    println!("    - 'No vector DB has anywhere near this production track record'");
    println!("  The unified-engine angle:");
    println!("    - Traditional stack: Elasticsearch (search) + Pinecone (vector) + ML service (ranking) + Postgres (filter)");
    println!("    - Vespa: one engine does all four — schema, indexing, ranking, ML inference");
    println!("    - Trade-off: more learning curve, but consolidates infra");
    println!("    - 'One database for search + vector + ranking + filter'");
    println!("  Integrations:");
    println!("    - vespa CLI (Go-based)");
    println!("    - SDKs: Python (pyvespa), Java, Go, .NET, REST/gRPC");
    println!("    - LangChain + LlamaIndex (Vespa as retriever)");
    println!("    - ONNX runtime built-in (TensorFlow + PyTorch + sklearn models)");
    println!("    - Kubernetes operator for self-hosted");
    println!("    - dbt integration emerging");
    println!("    - Streaming ingestion via Kafka/Pulsar/Kinesis (community)");
    println!("    - Hugging Face model integration");
    println!("  Vespa CLI usage:");
    println!("    vespa target local                                       # local Docker target");
    println!("    vespa deploy --wait 600 src/main/application              # deploy app config");
    println!("    vespa status                                              # cluster health");
    println!("    vespa document put id:my-coll:my-doc::1 docs.json");
    println!("    vespa document get id:my-coll:my-doc::1");
    println!("    vespa query 'select * from my-coll where userQuery() and category=\"news\"' --param=query='AI databases'");
    println!("    vespa visit                                               # iterate over all docs");
    println!("    vespa feed docs.jsonl                                    # bulk feed");
    println!("    vespa target cloud --application=my-tenant.my-app.default");
    println!("    vespa auth login                                          # Vespa Cloud auth");
    println!("    # Schema (.sd file):");
    println!("    # schema my_coll {{ document my_coll {{ field title type string {{ indexing: index | summary }} }} }}");
    println!("  Customers (large-scale search + recommendation + RAG):");
    println!("    - Yahoo (parent + still major user)");
    println!("    - Spotify (recommendation + search)");
    println!("    - Vimeo (video search)");
    println!("    - Ahrefs (SEO backlink search)");
    println!("    - OkCupid (recommendation)");
    println!("    - Various large e-commerce + adtech");
    println!("    - 'Used by Yahoo at billions of queries/day' = strongest enterprise reference");
    println!("  Critique: steeper learning curve than Pinecone/Chroma");
    println!("           YQL + schema language = upfront investment");
    println!("           less Python-first DX vs Chroma/LanceDB");
    println!("           branding less hot than newer vector DBs (Pinecone/Qdrant)");
    println!("           Vespa Cloud GTM smaller than competitors (only 1.5 years independent)");
    println!("           feature breadth = sometimes overwhelming for simple use cases");
    println!("           Java + C++ codebase = contribution barrier for newcomers");
    println!("           dev community smaller than newer hype-cycle products");
    println!("  Differentiator: Yahoo-origin (2003+) + battle-tested at billions of queries/day for 20+ years + powers Yahoo Search + Yahoo Mail + Yahoo Finance + Flickr + Spotify + Vimeo + Ahrefs + unified search + vector + ranking + ML inference in one engine + in-engine ML model serving (ONNX + LightGBM + XGBoost) + phased ranking (first-stage recall + second-stage ML re-rank) + first-class tensor type system + ColBERT multi-vector late interaction + 1000s of updates/sec/node + Jon Bratseth founder (Vespa architect at Yahoo 17+ years) + Apache 2.0 + Vespa Cloud managed + $31M Series A 2023 spin-off — the most production-proven and unified search + ranking + vector + ML platform, perfected at Yahoo over two decades and now independent");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vespa".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vespa(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vespa};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vespa"), "vespa");
        assert_eq!(basename(r"C:\bin\vespa.exe"), "vespa.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vespa.exe"), "vespa");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_vespa(&["--help".to_string()], "vespa"), 0);
        assert_eq!(run_vespa(&["-h".to_string()], "vespa"), 0);
        assert_eq!(run_vespa(&["--version".to_string()], "vespa"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_vespa(&[], "vespa"), 0);
    }
}
