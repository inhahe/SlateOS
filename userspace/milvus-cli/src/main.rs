#![deny(clippy::all)]

//! milvus-cli — OurOS Milvus (open-source cloud-native vector DB, Zilliz commercial, Shanghai/Redwood City)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_milvus(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: milvus [OPTIONS]");
        println!("Milvus (OurOS) — open-source cloud-native vector DB (LF AI), Zilliz commercial");
        println!();
        println!("Options:");
        println!("  --lite                 Milvus Lite (embedded in Python)");
        println!("  --standalone           Milvus Standalone (single binary)");
        println!("  --distributed          Milvus Distributed (K8s, billion+ scale)");
        println!("  --zilliz-cloud         Zilliz Cloud (managed Milvus, AWS/Azure/GCP)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Milvus 2024 (OurOS) — milvus_cli 2.x"); return 0; }
    println!("Milvus 2024 (OurOS) — Open-Source Cloud-Native Vector Database (LF AI)");
    println!("  Vendor: Zilliz, Inc. (Redwood City + Shanghai — private, commercial backer)");
    println!("          Project: Milvus (LF AI Foundation graduated project, since 2020)");
    println!("  Founders: Charles Xie (CEO of Zilliz), Milvus founded 2019 inside Zilliz");
    println!("          Charles Xie: ex-Oracle America (database engineer for 11 years)");
    println!("          Zilliz incubated Milvus → contributed to LF AI Foundation Jan 2020");
    println!("          Milvus graduated top-level LF AI project June 2021");
    println!("          One of the very first vector DBs (pre-dates Pinecone)");
    println!("  Funding (Zilliz):");
    println!("         Total raised: ~$113M");
    println!("         Series B Aug 2022: $60M");
    println!("         Investors: Hillhouse Capital, Pavilion Capital, Yunqi Partners, Prosperity7");
    println!("         Revenue private — Zilliz Cloud growing on RAG wave");
    println!("  Strategic position: 'cloud-native vector DB at billion-scale + LF AI open source':");
    println!("                    pitch: 'production-grade open-source vector DB for billion-scale workloads + multi-modal'");
    println!("                    target: enterprises with very large vector datasets + multi-cloud needs");
    println!("                    primary competitor: Pinecone, Weaviate, Qdrant, Chroma");
    println!("                    secondary: pgvector, Elasticsearch, MongoDB Atlas vector search");
    println!("                    Milvus's wedge: scalability + LF AI governance + GPU support + index diversity");
    println!("                    challenge: complexity vs Chroma/Qdrant simplicity for small/medium use");
    println!("                    counter: Milvus Lite (2024) for embedded simplicity, Distributed for scale");
    println!("  Pricing (Milvus OSS + Zilliz Cloud):");
    println!("    Milvus OSS: free (Apache 2.0)");
    println!("    Zilliz Cloud Free: 2GB cluster (always free)");
    println!("    Zilliz Cloud Standard: from $99/mo (1 CU dedicated)");
    println!("    Zilliz Cloud Enterprise: dedicated + audit + SSO + 99.99% SLA");
    println!("    Zilliz BYOC: deploy in customer VPC");
    println!("    typically competitive with Pinecone Serverless on cost");
    println!("  Architecture (cloud-native + storage/compute separation):");
    println!("    - Written primarily in Go + C++ (perf-critical index code C++)");
    println!("    - Storage on object store (S3, MinIO, GCS, Azure Blob)");
    println!("    - Compute: query nodes (read), data nodes (write), index nodes (build), coord nodes (control)");
    println!("    - Meta on etcd, message broker on Pulsar/Kafka/RocksMQ");
    println!("    - K8s-native distributed deployment");
    println!("    - 10+ index types (HNSW, IVF_FLAT, IVF_PQ, DiskANN, SCANN, GPU_IVF_FLAT, etc.)");
    println!("    - GPU acceleration via RAFT (NVIDIA library)");
    println!("    - Multi-tenancy via partitions + databases");
    println!("  Product portfolio:");
    println!("    1. Milvus (LF AI open-source):");
    println!("       - Apache 2.0, distributed cloud-native");
    println!("       - 28K+ GitHub stars (highest of any vector DB)");
    println!("       - 50M+ Docker pulls cumulative");
    println!("       - Graduated LF AI project");
    println!("    2. Milvus Lite (2024+):");
    println!("       - pip install pymilvus, embedded SQLite-like mode");
    println!("       - Answer to Chroma's embedded simplicity");
    println!("       - Same API as full Milvus");
    println!("    3. Milvus Standalone:");
    println!("       - Single-binary Docker deployment");
    println!("       - Good for: small-medium production, dev");
    println!("    4. Milvus Distributed:");
    println!("       - K8s-native, scales to billions of vectors");
    println!("       - Independent scaling per component (query/data/index/coord)");
    println!("       - Used by: Walmart, Roblox, Salesforce, ebay");
    println!("    5. Zilliz Cloud (managed):");
    println!("       - Multi-cloud (AWS, GCP, Azure)");
    println!("       - Auto-scaling, zero ops");
    println!("       - Multi-region replication");
    println!("       - SOC 2 Type II, HIPAA, GDPR");
    println!("    6. Index types (industry-leading diversity):");
    println!("       - FLAT (exact)");
    println!("       - IVF_FLAT, IVF_SQ8, IVF_PQ (inverted file)");
    println!("       - HNSW (graph)");
    println!("       - DiskANN (Microsoft Research)");
    println!("       - SCANN (Google Research)");
    println!("       - AUTOINDEX (Zilliz Cloud automatic)");
    println!("       - GPU_IVF_FLAT, GPU_IVF_PQ (GPU-accelerated)");
    println!("       - BIN_FLAT, BIN_IVF_FLAT (binary vectors)");
    println!("    7. GPU acceleration:");
    println!("       - NVIDIA RAFT library integration");
    println!("       - 10-100x speedup for index build + search");
    println!("       - Critical for billion-scale");
    println!("    8. Hybrid search:");
    println!("       - BM25 + dense vectors (2024+)");
    println!("       - Sparse vectors (SPLADE, BGE-M3)");
    println!("       - Multi-vector queries");
    println!("    9. Multi-tenancy:");
    println!("       - Databases (per-tenant)");
    println!("       - Partitions (within collection)");
    println!("    10. Time-Travel + versioning:");
    println!("       - Snapshot/restore");
    println!("       - Bulk import + export to S3/GCS");
    println!("    11. Milvus Operator (K8s):");
    println!("       - Helm chart + Kubernetes operator");
    println!("       - Production-grade K8s deployments");
    println!("    12. Vector visualization (Attu):");
    println!("       - Open-source GUI for Milvus");
    println!("       - Collection/index management + query UI");
    println!("  The LF AI governance angle:");
    println!("    - Milvus is LF AI top-level project (graduated June 2021)");
    println!("    - Multi-stakeholder governance (not pure Zilliz)");
    println!("    - Contributors include: Zilliz, NVIDIA, Microsoft, Roblox, others");
    println!("    - Enterprise comfort: 'won't disappear if Zilliz fails'");
    println!("    - Apache 2.0 license, vendor-neutral");
    println!("  The billion-scale claim:");
    println!("    - Milvus Distributed regularly handles billions of vectors");
    println!("    - Customers: ebay (1B+ vectors), Roblox (multi-B), Walmart (catalog)");
    println!("    - Sharding + replication architecture scales linearly");
    println!("    - GPU indexing critical at this scale");
    println!("    - 'No other OSS vector DB has more billion-scale production deployments'");
    println!("  Integrations:");
    println!("    - milvus_cli (Python CLI)");
    println!("    - SDKs: Python (pymilvus), JS/TS, Java, Go, Rust, C#, Node");
    println!("    - LangChain + LlamaIndex + Haystack");
    println!("    - OpenAI + Cohere + Anthropic + HF embedders");
    println!("    - Spark connector (Milvus-Spark)");
    println!("    - Kafka + Pulsar streaming sources");
    println!("    - Apache Beam connector");
    println!("    - Airflow + Dagster + Prefect operators");
    println!("    - Attu (open-source GUI)");
    println!("    - Birdwatcher (Milvus debugging tool)");
    println!("  Milvus CLI usage:");
    println!("    docker run -d --name milvus -p 19530:19530 -p 9091:9091 milvusdb/milvus:latest standalone");
    println!("    # Via Python:");
    println!("    # from pymilvus import MilvusClient; client = MilvusClient('http://localhost:19530')");
    println!("    milvus_cli connect -uri http://localhost:19530");
    println!("    milvus_cli create collection my-coll -p schema.yaml");
    println!("    milvus_cli create index my-coll my_vec_field -t HNSW -m COSINE -p 'M:16, efConstruction:200'");
    println!("    milvus_cli load -c my-coll");
    println!("    milvus_cli search -c my-coll -d 'my_vec_field' -v '[0.1,0.2,...]' -k 10");
    println!("    milvus_cli show collections");
    println!("    milvus_cli backup create my-backup -c my-coll");
    println!("    milvus_cli release -c my-coll");
    println!("    # Zilliz Cloud:");
    println!("    zilliz-cli auth login");
    println!("    zilliz-cli cluster create my-cluster --cu-size=1 --region=aws-us-west-2");
    println!("  Customers (open-source + Zilliz Cloud):");
    println!("    - Walmart, Roblox, ebay (1B+ vector deployments)");
    println!("    - Salesforce, Tencent, Bilibili");
    println!("    - NVIDIA, Bosch, Reno (industry research)");
    println!("    - 5,000+ enterprise organizations using Milvus (cumulative)");
    println!("    - Use cases: image/video search, recommendation, drug discovery, fraud detection");
    println!("    - 28K+ GitHub stars, 50M+ Docker pulls cumulative");
    println!("  Critique: complexity intimidating (multiple node types, etcd, pulsar)");
    println!("           Milvus Lite is recent (2024) — Chroma had simplicity earlier");
    println!("           Zilliz Cloud GTM smaller than Pinecone's");
    println!("           operating distributed Milvus requires K8s expertise");
    println!("           Postgres + pgvector + sqlite-vec erode small-scale use case");
    println!("           index choice (10+ options) requires expertise to pick");
    println!("           Asian engineering origin (Shanghai) less brand recognition in US enterprise (improving)");
    println!("           feature-rich = sometimes overwhelming for simple use cases");
    println!("  Differentiator: LF AI Foundation top-level project (governance-neutral, graduated 2021) + 28K+ GitHub stars (highest of any vector DB) + 50M+ Docker pulls + cloud-native architecture (storage/compute separation on S3/MinIO/GCS) + 10+ index types (HNSW + IVF + DiskANN + SCANN + GPU variants) + NVIDIA RAFT GPU acceleration + Walmart/Roblox/ebay billion-scale customers + Charles Xie founder (ex-Oracle DB engineer 11 years) + Milvus Lite (2024 embedded simplicity) + Standalone (single binary) + Distributed (K8s billion-scale) + Zilliz Cloud managed (multi-cloud) + Attu visualization GUI + Apache 2.0 + $113M raised — the most scalable open-source vector database with the broadest index type diversity, governed by LF AI Foundation, used at billion-scale by Walmart and Roblox");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "milvus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_milvus(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_milvus};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/milvus"), "milvus");
        assert_eq!(basename(r"C:\bin\milvus.exe"), "milvus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("milvus.exe"), "milvus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_milvus(&["--help".to_string()], "milvus"), 0);
        assert_eq!(run_milvus(&["-h".to_string()], "milvus"), 0);
        assert_eq!(run_milvus(&["--version".to_string()], "milvus"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_milvus(&[], "milvus"), 0);
    }
}
