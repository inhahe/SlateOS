#![deny(clippy::all)]

//! cloudflare-cli — OurOS Cloudflare (edge + security + Workers + R2 + AI, San Francisco, NYSE:NET)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cloudflare [OPTIONS]");
        println!("Cloudflare (OurOS) — connectivity cloud (CDN + security + Workers + AI, NYSE:NET)");
        println!();
        println!("Options:");
        println!("  --workers              Workers (serverless edge compute, V8 isolates)");
        println!("  --r2                   R2 (S3-compatible object storage, no egress fees)");
        println!("  --d1                   D1 (SQLite database at the edge)");
        println!("  --kv                   Workers KV (key-value at the edge)");
        println!("  --pages                Pages (Jamstack hosting)");
        println!("  --workers-ai           Workers AI (LLM inference at the edge)");
        println!("  --zero-trust           Zero Trust (Cloudflare One — SASE)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cloudflare 2024 (OurOS) — wrangler 3.x"); return 0; }
    println!("Cloudflare 2024 (OurOS) — Connectivity Cloud");
    println!("  Vendor: Cloudflare, Inc. (San Francisco, CA — NYSE:NET since 2019)");
    println!("  Founders: Matthew Prince (CEO) + Lee Holloway + Michelle Zatlyn, 2009");
    println!("          founded as Project Honey Pot (spam tracking) → pivoted to Cloudflare (DDoS+CDN)");
    println!("          Matthew Prince: long-time CEO + co-founder, regular industry voice");
    println!("          Michelle Zatlyn: co-founder + President");
    println!("          Lee Holloway: co-founder + former CTO (left due to illness)");
    println!("          'Help build a better Internet' = corporate mission");
    println!("  Public market (NYSE:NET):");
    println!("         IPO Sept 2019 at $15/share — raised $525M");
    println!("         peak ~$220 in late 2021");
    println!("         settled $80-120 range 2023-2024");
    println!("         FY2024 revenue: ~$1.67B (+30% YoY)");
    println!("         Market cap: $30-40B range");
    println!("         Strongest growth profile of any pure-play security/infra company");
    println!("  Strategic position: 'connectivity cloud — security + performance + compute + AI at the edge':");
    println!("                    pitch: 'one unified platform to make everything you connect to faster, safer, smarter'");
    println!("                    target: everyone — from indie dev to Fortune 100 + government");
    println!("                    primary competitor: AWS CloudFront, Akamai, Fastly (CDN); Zscaler, Palo Alto (Zero Trust)");
    println!("                    secondary: Cloudflare Workers vs Vercel/Netlify/Lambda@Edge");
    println!("                    Cloudflare's wedge: massive global anycast network (300+ cities) + every-layer-stack platform");
    println!("                    network effect: more users on Cloudflare = better DDoS data + threat intel");
    println!("                    'Internet's immune system' positioning resonates with security buyers");
    println!("  Pricing (notably aggressive free tier + transparent paid tiers):");
    println!("    Free: 50,000 Workers requests/day, 100K DNS queries, unlimited DDoS protection");
    println!("    Pro: $20/mo per domain (faster propagation, image optimization, etc.)");
    println!("    Business: $200/mo per domain (Argo + Railgun + advanced features)");
    println!("    Enterprise: custom (typically $5K-$5M+/yr)");
    println!("    Workers: $5/mo for 10M requests, then $0.50/M");
    println!("    R2: $0.015/GB-month storage, $0 egress (vs $0.09/GB S3 egress)");
    println!("    typically aggressive pricing to undercut AWS data egress in particular");
    println!("  Product portfolio (the 'connectivity cloud'):");
    println!("    1. CDN + DDoS Protection (the original products):");
    println!("       - 300+ city anycast network");
    println!("       - Unlimited DDoS mitigation (free tier!)");
    println!("       - Global edge caching + TLS");
    println!("       - Argo Smart Routing (paid, performance)");
    println!("    2. Workers (serverless edge compute):");
    println!("       - V8 isolate-based (NOT containers — faster cold start)");
    println!("       - Run JavaScript/TypeScript/WebAssembly/Python at 300+ edge locations");
    println!("       - ~5ms cold start, sub-millisecond warm");
    println!("       - Service Workers API + Worker-specific APIs");
    println!("    3. Workers Storage:");
    println!("       - KV (key-value): eventually consistent, edge-replicated");
    println!("       - Durable Objects: strongly consistent, location-aware single-instance");
    println!("       - R2: S3-compatible object storage (NO egress fees — disruptive)");
    println!("       - D1: SQLite database at the edge");
    println!("       - Queues: managed message queues");
    println!("       - Vectorize: vector database for RAG/AI use cases");
    println!("       - Hyperdrive: Postgres connection pooling/caching at edge");
    println!("    4. Pages (Jamstack hosting):");
    println!("       - Git push to deploy static + serverless");
    println!("       - Pages Functions = Workers integration");
    println!("       - Compete with: Vercel, Netlify, GitHub Pages");
    println!("    5. Cloudflare One (Zero Trust + SASE):");
    println!("       - ZTNA (Zero Trust Network Access)");
    println!("       - Secure Web Gateway");
    println!("       - DLP (Data Loss Prevention)");
    println!("       - Browser Isolation");
    println!("       - Compete with: Zscaler, Netskope, Palo Alto Prisma Access");
    println!("    6. Workers AI (2023+ — AI inference at edge):");
    println!("       - Llama, Mistral, Whisper, Stable Diffusion served from 200+ Cloudflare PoPs");
    println!("       - $0.011 per 1K Llama-3-8B input tokens (highly competitive)");
    println!("       - 'Run any open-source model at the edge'");
    println!("       - AI Gateway: caching + analytics + rate limiting for AI APIs");
    println!("       - Vectorize integration for RAG");
    println!("    7. Magic Transit + Magic WAN:");
    println!("       - L3 DDoS protection + WAN-as-a-service");
    println!("       - BGP/IP transit replacement");
    println!("       - Enterprise networking layer");
    println!("    8. Magic Firewall:");
    println!("       - Cloud-delivered network firewall");
    println!("    9. Email Security (acquired Area 1 Security 2022 $162M):");
    println!("       - Anti-phishing email security");
    println!("       - Compete with: Proofpoint, Mimecast, Microsoft Defender for Office");
    println!("    10. SSL/TLS + DNS (the foundational free tier):");
    println!("       - Free Universal SSL");
    println!("       - 1.1.1.1 DNS resolver (the fastest public DNS, with privacy)");
    println!("       - Authoritative DNS for managed domains");
    println!("  V8 isolates architecture (the Workers bet):");
    println!("    - V8 isolates (Chrome's JS engine sandboxes) instead of containers");
    println!("    - ~5ms cold start vs 100-500ms for Lambda");
    println!("    - Hundreds of isolates per process = high density");
    println!("    - Trade-off: only JS/TS/WASM/Python (no arbitrary binaries)");
    println!("    - Tighter sandboxing than containers");
    println!("    - Wrangler (CLI) provides best-in-class DX");
    println!("  Anycast network (the moat):");
    println!("    - 300+ cities, 100+ countries");
    println!("    - 320+ Tbps network capacity (2024)");
    println!("    - Largest DDoS attacks mitigated (>200 Gbps daily)");
    println!("    - One IP, served from nearest location automatically");
    println!("    - Massive threat intel data — sees % of all Internet traffic");
    println!("  R2 (the disruptive storage product):");
    println!("    - S3-compatible API");
    println!("    - $0 egress fees (huge disruption to AWS S3 model)");
    println!("    - $0.015/GB-month storage (cheaper than S3 Standard)");
    println!("    - Strong adoption from data-heavy customers tired of AWS egress");
    println!("  Integrations:");
    println!("    - Wrangler CLI for Workers/Pages/R2/D1");
    println!("    - Terraform + Pulumi providers");
    println!("    - GitHub/GitLab/Bitbucket for Pages deploys");
    println!("    - Open standards: BGP, DNS, S3 API, OpenTelemetry");
    println!("    - 1,000+ apps in Cloudflare Apps marketplace");
    println!("    - Compatible with most web frameworks (Next.js, Remix, Astro, Hono, etc.)");
    println!("    - SDKs: JS, Go, Python, Rust, PHP, Java, .NET, Ruby");
    println!("  Cloudflare CLI usage:");
    println!("    wrangler login                                         # auth");
    println!("    wrangler init my-worker --template typescript          # scaffold");
    println!("    wrangler dev                                            # local dev");
    println!("    wrangler deploy                                         # deploy to edge");
    println!("    wrangler tail my-worker                                 # live logs");
    println!("    wrangler kv:key put MY_KEY my_value");
    println!("    wrangler r2 bucket create my-bucket");
    println!("    wrangler r2 object put my-bucket/file.txt --file file.txt");
    println!("    wrangler d1 create my-db");
    println!("    wrangler d1 execute my-db --command 'SELECT * FROM users'");
    println!("    wrangler pages deploy ./dist");
    println!("    wrangler queues create my-queue");
    println!("  Customers:");
    println!("    - 30%+ of Fortune 1000");
    println!("    - 4M+ active Cloudflare accounts");
    println!("    - 27M+ Internet properties protected");
    println!("    - Major: Discord, Stripe, Shopify (some), Doordash, IBM (parts), Salesforce");
    println!("    - U.S. federal: DoD, intelligence community, FBI");
    println!("    - Sweet spot: anything from small website to Fortune 100");
    println!("    - international: equally strong globally (network advantages)");
    println!("  Critique: Workers' JS-only sandbox limits some use cases (no arbitrary binaries)");
    println!("           AWS Lambda + CloudFront bundling for AWS-native shops");
    println!("           D1 + R2 + Workers AI all relatively young vs AWS equivalents");
    println!("           Cloudflare One (Zero Trust) less mature than Zscaler/Netskope at enterprise scale");
    println!("           customer support quality variable for free + Pro tiers");
    println!("           DNS resolver (1.1.1.1) generally free = no direct revenue from it");
    println!("           data residency/sovereignty challenges for EU/regulated customers");
    println!("           Email Security (Area 1) integration ongoing");
    println!("  Differentiator: 300+ city anycast network (largest CDN footprint) + V8 isolate Workers (5ms cold start vs 100-500ms Lambda) + R2 (zero-egress object storage — disruptive to AWS S3) + Workers AI (open-source LLM inference at edge) + 1.1.1.1 DNS resolver + Cloudflare One Zero Trust + 30%+ Fortune 1000 customer base + $1.67B revenue with 30%+ growth + 'connectivity cloud' multi-product platform — the every-layer infrastructure provider that nearly every Internet user touches every day even if they don't know it");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cloudflare".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cf(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cloudflare"), "cloudflare");
        assert_eq!(basename(r"C:\bin\cloudflare.exe"), "cloudflare.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cloudflare.exe"), "cloudflare");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cf(&["--help".to_string()], "cloudflare"), 0);
        assert_eq!(run_cf(&["-h".to_string()], "cloudflare"), 0);
        let _ = run_cf(&["--version".to_string()], "cloudflare");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cf(&[], "cloudflare");
    }
}
