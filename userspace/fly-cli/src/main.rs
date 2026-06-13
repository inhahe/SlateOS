#![deny(clippy::all)]

//! fly-cli — SlateOS Fly.io (edge PaaS for Docker apps, Chicago, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fly(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fly [OPTIONS]");
        println!("Fly.io (SlateOS) — global app platform for Docker (formerly Fly Edge)");
        println!();
        println!("Options:");
        println!("  --apps                 Apps (the deployed Docker images)");
        println!("  --machines             Fly Machines (the firecracker-based VMs)");
        println!("  --regions              Regions (35+ global locations)");
        println!("  --postgres             Fly Postgres (managed Postgres clusters)");
        println!("  --gpus                 Fly GPUs (A100/H100/L40S for AI workloads)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Fly.io 2024 (SlateOS) — flyctl"); return 0; }
    println!("Fly.io 2024 (SlateOS) — Global App Platform");
    println!("  Vendor: Fly.io, Inc. (Chicago, IL — private)");
    println!("  Founders: Kurt Mackey + Thomas Ptacek + Steve Berryman, 2017");
    println!("          founded as 'Fly Edge' for serverless edge compute, pivoted to general PaaS 2020");
    println!("          Kurt Mackey: CEO, ex-Crowdmob + Compose.io");
    println!("          Thomas Ptacek: famously prolific writer + security veteran (ex-Matasano)");
    println!("          Strong cult-following in indie dev + tech-Twitter community");
    println!("  Private funding:");
    println!("         Series B Apr 2022: $70M at ~$400M valuation (a16z led)");
    println!("         total raised: ~$110M");
    println!("         a16z, Intel Capital, Dell Technologies Capital backers");
    println!("         estimated $40-80M ARR (private — growing rapidly with AI workloads)");
    println!("  Strategic position: 'run your Docker app close to users globally — Heroku for the Docker era':");
    println!("                    pitch: 'fly deploy and your app runs in 35 regions worldwide'");
    println!("                    target: developers + scale-ups who care about latency + dev experience");
    println!("                    primary competitor: Heroku (Salesforce), Render, Railway, Vercel (different model)");
    println!("                    secondary: AWS (App Runner), Cloudflare Workers, DigitalOcean App Platform");
    println!("                    Fly's wedge: anycast routing + Firecracker VMs + 35+ regions + great DX");
    println!("                    'Distributed by default' = move workload to user via Fly's anycast network");
    println!("  Pricing (per-second billing — generous free tier):");
    println!("    Free tier: 3 small VMs always-free (256MB shared-cpu-1x in any 2 regions)");
    println!("    Shared CPU: $0.0000022/sec (~$5.70/mo for shared-cpu-1x)");
    println!("    Performance CPU: $0.0000220/sec (~$57/mo for performance-1x)");
    println!("    Postgres: $1.94+/mo (single-node) to $30K+/mo (HA clusters)");
    println!("    GPUs: $1.25-$6.00/hr (A10, A100, L40S, H100)");
    println!("    Bandwidth: 100GB/mo free outbound, then $0.02-$0.12/GB by region");
    println!("    typically cheaper than equivalent AWS Fargate at small scales");
    println!("  Product portfolio:");
    println!("    1. Fly Apps (the original product):");
    println!("       - Deploy a Docker image, get a globally distributed app");
    println!("       - fly.toml config file for deploys + secrets");
    println!("       - HTTP, TCP, UDP proxy support");
    println!("       - Anycast IPv4/IPv6 from Fly's global network");
    println!("    2. Fly Machines (the new compute primitive):");
    println!("       - Firecracker microVMs (Amazon's secure microVM tech)");
    println!("       - REST API to create + run individual VMs");
    println!("       - Start in ~1-3 seconds");
    println!("       - Per-second billing");
    println!("       - 'Start, run, stop' pattern (or always-on)");
    println!("    3. Fly Postgres:");
    println!("       - Managed Postgres clusters");
    println!("       - HA replication across regions");
    println!("       - Backups + point-in-time recovery");
    println!("    4. Fly Redis (managed via Upstash):");
    println!("       - Partnership integration for low-latency Redis");
    println!("    5. Fly GPUs (2023+ — the AI inference push):");
    println!("       - A10, A100, L40S, H100 GPUs in selected regions");
    println!("       - Optimized for ML inference + LLM serving");
    println!("       - 'fly deploy' with GPU = AI workloads at the edge");
    println!("    6. Fly Tigris (S3-compatible object storage):");
    println!("       - Partnership with Tigris Data");
    println!("       - Globally distributed object storage");
    println!("    7. Fly LiteFS (SQLite over FUSE):");
    println!("       - Distributed SQLite for edge applications");
    println!("       - 'Replicate SQLite across regions'");
    println!("       - Cool architectural innovation for global apps");
    println!("    8. flyctl (the CLI):");
    println!("       - Best-in-class developer experience");
    println!("       - 'fly launch' detects framework + scaffolds deploy");
    println!("       - 'fly ssh console' for live debugging");
    println!("       - 'fly logs' for tail");
    println!("  Anycast networking (the edge bet):");
    println!("    - Fly operates its own anycast network across 35+ regions");
    println!("    - User traffic routes to nearest Fly POP");
    println!("    - Apps can deploy to any subset of regions");
    println!("    - Latency-optimized for global users");
    println!("    - Custom-built (not just AWS rehosted)");
    println!("  Firecracker VMs (the runtime):");
    println!("    - Amazon's open-source microVM technology");
    println!("    - Hardware isolation (vs containers)");
    println!("    - Sub-second start times");
    println!("    - 'KVM-like security for containers' positioning");
    println!("    - Fly uses Firecracker on its own bare-metal hardware (not AWS)");
    println!("  Integrations:");
    println!("    - Docker images (anything that builds to OCI)");
    println!("    - Most popular frameworks: Rails, Django, FastAPI, Next.js, Phoenix, Elixir, Go, Rust");
    println!("    - Postgres: native Fly Postgres + external connections");
    println!("    - Redis: Upstash partnership");
    println!("    - Object storage: Tigris, S3, R2 (external)");
    println!("    - Secrets management: built-in encrypted secret storage");
    println!("    - DNS: Fly DNS + custom domains");
    println!("    - GitOps: GitHub Actions + GitLab CI templates");
    println!("    - Sentry, Honeycomb, Datadog, OpenTelemetry for observability");
    println!("  Fly CLI usage:");
    println!("    fly auth login");
    println!("    fly launch                     # scaffold from current directory");
    println!("    fly deploy --remote-only       # build remotely + deploy");
    println!("    fly apps list");
    println!("    fly status --app my-app");
    println!("    fly logs --app my-app");
    println!("    fly ssh console --app my-app   # live debug");
    println!("    fly scale count 3 --region iad,fra,nrt");
    println!("    fly postgres create --name my-db --region iad --vm-size shared-cpu-1x");
    println!("    fly machines run -e AWS_REGION=iad nginx:latest");
    println!("    fly gpu list                    # see available GPU regions");
    println!("    fly volumes create my-data --region iad --size 10");
    println!("  Customers (tens of thousands of devs):");
    println!("    - Indie devs + startups + scale-ups");
    println!("    - Known users: Plausible, Supabase (some infra), Resend, Tinybird (early)");
    println!("    - Many YC startups choose Fly for early production");
    println!("    - Tech-Twitter community amplification (Thomas Ptacek's writing)");
    println!("    - International: significant European + global use");
    println!("    - sweet spot: developers who want Heroku-style DX with edge + container flexibility");
    println!("  Critique: outages have been more frequent than enterprise customers prefer");
    println!("           Postgres HA reliability issues publicly acknowledged (2022-2023)");
    println!("           less mature enterprise security/compliance vs AWS/GCP");
    println!("           geo-distribution sounds great but most apps don't need it");
    println!("           competing with Cloudflare Workers, Vercel, Render for similar buyers");
    println!("           pricing can balloon for high-CPU sustained workloads vs reserved instances on AWS");
    println!("           support quality variable (community-first model has limits)");
    println!("           AI workloads (GPU) require capacity planning across regions");
    println!("  Differentiator: anycast routing + Firecracker microVMs across 35+ regions + per-second billing + best-in-class flyctl CX + LiteFS (distributed SQLite for edge apps) + GPU support for AI inference + Thomas Ptacek's writing-driven brand + 'fly launch' instant DX for Docker apps + dev-cult-following — the global app platform that lets indie devs deploy distributed apps with a single command and per-second billing");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fly".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fly(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fly};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fly"), "fly");
        assert_eq!(basename(r"C:\bin\fly.exe"), "fly.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fly.exe"), "fly");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fly(&["--help".to_string()], "fly"), 0);
        assert_eq!(run_fly(&["-h".to_string()], "fly"), 0);
        let _ = run_fly(&["--version".to_string()], "fly");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fly(&[], "fly");
    }
}
