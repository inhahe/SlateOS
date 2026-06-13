#![deny(clippy::all)]

//! marqo-cli — SlateOS Marqo (open-source multi-modal vector search engine, Sydney AU)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_marqo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: marqo [OPTIONS]");
        println!("Marqo (Slate OS) — open-source multi-modal vector search engine (Sydney AU)");
        println!();
        println!("Options:");
        println!("  --multimodal           Multi-modal search (text + image + audio + video)");
        println!("  --cloud                Marqo Cloud (managed SaaS)");
        println!("  --embed                Built-in embedding generation (one-step ingestion)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Marqo 2024 (Slate OS) — marqo CLI 2.x"); return 0; }
    println!("Marqo 2024 (Slate OS) — Open-Source Multi-Modal Vector Search Engine");
    println!("  Vendor: Marqo AI, Pty Ltd (Sydney, Australia — private)");
    println!("  Founders: Tom Hamer + Jesse Clark, 2021");
    println!("          Tom Hamer: CEO, ex-Amazon (AWS), ex-Atlassian");
    println!("          Jesse Clark: CTO, PhD in computational physics, ex-Amazon SageMaker");
    println!("          Met at Amazon working on vector search infrastructure");
    println!("          Identified gap: 'multi-modal search is too hard to set up'");
    println!("          Founded in Sydney — Australian deep-tech AI scene");
    println!("          Open-source first (Apache 2.0)");
    println!("  Funding:");
    println!("         Total raised: ~$5.4M");
    println!("         Seed Jun 2022: $5.4M (Blackbird Ventures lead, Black Sheep Capital, individuals)");
    println!("         Smaller funding than US competitors — Australian capital-efficient");
    println!("         Series A unannounced — bootstrapping Marqo Cloud growth");
    println!("  Strategic position: 'multi-modal search engine with built-in embedding generation':");
    println!("                    pitch: 'one engine for text + image + audio + video search, no separate embedder needed'");
    println!("                    target: e-commerce search, content moderation, image/video retrieval, RAG");
    println!("                    primary competitor: Weaviate (multi-modal), Vespa (search + vector), Pinecone");
    println!("                    secondary: Chroma, Qdrant, OpenSearch + embedding pipeline");
    println!("                    Marqo's wedge: built-in inference (no need for separate embedding service)");
    println!("                    'Tensor-native search — multi-vector by default'");
    println!("                    Ex-Amazon SageMaker founders → strong ML credibility");
    println!("  Pricing (OSS + Cloud):");
    println!("    Marqo OSS: free (Apache 2.0)");
    println!("    Marqo Cloud Free: free dev tier");
    println!("    Marqo Cloud Starter: ~$80/mo small index");
    println!("    Marqo Cloud Performance: GPU-backed indexes for image/video");
    println!("    Marqo Cloud Enterprise: dedicated infra, custom");
    println!("    notably GPU pricing tier — image/video embed at scale needs GPUs");
    println!("  Architecture (the integrated stack):");
    println!("    - Written in Python (orchestration) + integrates OpenSearch/Vespa-like vector engine");
    println!("    - Embedding generation: HuggingFace models in-process (no separate service)");
    println!("    - GPU support: PyTorch backend for CLIP, SBERT, custom models");
    println!("    - Tensor storage: per-document multi-vector (e.g. patches of an image)");
    println!("    - HNSW indexes for ANN");
    println!("    - REST API + Python client");
    println!("    - Single Docker container for OSS deployment");
    println!("  Product portfolio:");
    println!("    1. Marqo Engine (OSS):");
    println!("       - Apache 2.0 open source");
    println!("       - 4K+ GitHub stars");
    println!("       - Single container deploy: docker run marqoai/marqo");
    println!("       - Bundles vector search + inference in one image");
    println!("    2. Marqo Cloud (managed):");
    println!("       - SaaS multi-region (AWS-backed)");
    println!("       - GPU-backed indexes for image/video");
    println!("       - Auto-scaling, zero ops");
    println!("    3. Multi-modal search (the differentiator):");
    println!("       - Text + image + audio + video in one index");
    println!("       - CLIP (OpenAI) integrated for text/image cross-modal");
    println!("       - Vision Transformer (ViT) models supported");
    println!("       - Search by image returning text, or vice versa");
    println!("    4. Built-in embedding inference:");
    println!("       - HuggingFace model loading on startup");
    println!("       - SBERT, MPNet, BGE, e5, all-MiniLM popular models");
    println!("       - Custom model upload (BYOM)");
    println!("       - No separate embedding service needed");
    println!("       - Marqo handles encoding at index + query time");
    println!("    5. Tensor search (multi-vector per doc):");
    println!("       - Split a document into chunks, each gets a vector");
    println!("       - Search returns the best-matching chunk + parent doc");
    println!("       - 'Chunk-level retrieval, doc-level results'");
    println!("       - Critical for long documents + image patches");
    println!("    6. Hybrid search:");
    println!("       - Lexical (BM25-equivalent) + vector hybrid");
    println!("       - Reciprocal Rank Fusion (RRF) for blending");
    println!("       - Pre-filter + post-filter both supported");
    println!("    7. Document management:");
    println!("       - Auto-chunking with configurable strategies");
    println!("       - Field-level indexing (tensor + lexical + filter)");
    println!("       - Add/update/delete documents via REST");
    println!("    8. E-commerce focus:");
    println!("       - Pre-tuned models for product search");
    println!("       - Image + title + description multi-modal product search");
    println!("       - Customer: Redbubble, Australia Post, AWS Marketplace partners");
    println!("    9. Marqtune (model fine-tuning, 2024):");
    println!("       - Fine-tune embedding models on your data");
    println!("       - Generative training pairs (synthetic data)");
    println!("       - Train custom embeddings without ML team");
    println!("    10. Generative search (RAG-ready):");
    println!("       - Direct integration with LLM-based answering");
    println!("       - Source attribution from retrieved chunks");
    println!("       - LangChain + LlamaIndex retriever");
    println!("  The 'one engine for embedding + search' angle:");
    println!("    - Traditional stack: embedding service (separate) + vector DB (separate)");
    println!("    - Marqo: 'just send the document, we'll embed + index'");
    println!("    - Lower latency (no network hop between embed + search)");
    println!("    - Simpler ops (one container)");
    println!("    - Trade-off: scale embedding + search together (not independently)");
    println!("  The multi-modal pitch:");
    println!("    - Most vector DBs: 'bring your own embeddings, single-vector per doc'");
    println!("    - Marqo: 'multi-vector per doc, multi-modal native, embeddings included'");
    println!("    - Use case: search an e-commerce catalog by photo");
    println!("    - Use case: video moment retrieval (find the second X happens)");
    println!("    - Use case: cross-modal image-text-audio search");
    println!("  Integrations:");
    println!("    - marqo CLI (Python)");
    println!("    - SDKs: Python (marqo), JS/TS, Java (community)");
    println!("    - LangChain + LlamaIndex first-class");
    println!("    - HuggingFace Hub model integration");
    println!("    - OpenAI CLIP, SBERT, BGE models pre-bundled");
    println!("    - PyTorch backend for inference");
    println!("    - REST API for any language");
    println!("    - Docker + Kubernetes operator");
    println!("  Marqo CLI usage:");
    println!("    docker run -p 8882:8882 marqoai/marqo:latest          # OSS quick-start");
    println!("    # Via Python client:");
    println!("    # import marqo; mq = marqo.Client(url='http://localhost:8882')");
    println!("    # mq.create_index('my-coll', model='ViT-L/14')");
    println!("    # mq.index('my-coll').add_documents([{{'title': 'foo', 'image': 'http://...'}}])");
    println!("    # mq.index('my-coll').search('red shoes')");
    println!("    marqo create-index my-coll --model=ViT-L/14");
    println!("    marqo add-docs my-coll --file=docs.jsonl");
    println!("    marqo search my-coll --query='red running shoes' --limit=10");
    println!("    marqo search my-coll --image=https://example.com/shoe.jpg --limit=10");
    println!("    marqo delete-index my-coll");
    println!("    marqo cloud login                                       # Marqo Cloud auth");
    println!("    marqo cloud create-index my-cloud-coll --tier=performance --gpu");
    println!("  Customers (e-commerce + multi-modal):");
    println!("    - Redbubble (Australian print-on-demand marketplace — visual product search)");
    println!("    - Australia Post (search across multi-modal data)");
    println!("    - Various e-commerce startups (image-based product discovery)");
    println!("    - Content moderation platforms (multi-modal NSFW detection)");
    println!("    - Various RAG + retrieval startups");
    println!("    - 4K+ GitHub stars, growing");
    println!("    - PyPI marqo downloads steady growth");
    println!("  Critique: small funding ($5.4M vs Pinecone $138M)");
    println!("           Python-based engine = perf vs Rust DBs (Qdrant, LanceDB)");
    println!("           OSS deploy bundles PyTorch + models = heavy Docker image (10GB+)");
    println!("           Embedding + search co-located = coupled scaling");
    println!("           multi-modal still niche vs pure-text RAG");
    println!("           competing with Weaviate (more mature multi-modal story)");
    println!("           competing with Vespa (more mature search engine)");
    println!("           Australian timezone = harder for US enterprise sales coverage");
    println!("           small engineering team (~15-20 people)");
    println!("  Differentiator: built-in embedding generation in the engine (no separate inference service needed) + multi-modal native (text + image + audio + video in one index, CLIP-based) + tensor search (multi-vector per doc, chunk-level retrieval with doc-level results) + Marqtune custom fine-tuning + GPU-backed cloud tier for image/video at scale + e-commerce vertical focus (Redbubble, Australia Post) + ex-Amazon SageMaker founders + Sydney AU deep-tech + single Docker container OSS deploy + Apache 2.0 + LangChain/LlamaIndex first-class + auto-chunking + hybrid lexical + vector — the multi-modal-first vector search engine that bundles embedding inference, eliminating the need for a separate embedding pipeline");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "marqo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_marqo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_marqo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/marqo"), "marqo");
        assert_eq!(basename(r"C:\bin\marqo.exe"), "marqo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("marqo.exe"), "marqo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_marqo(&["--help".to_string()], "marqo"), 0);
        assert_eq!(run_marqo(&["-h".to_string()], "marqo"), 0);
        let _ = run_marqo(&["--version".to_string()], "marqo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_marqo(&[], "marqo");
    }
}
