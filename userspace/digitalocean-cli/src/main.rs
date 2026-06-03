#![deny(clippy::all)]

//! digitalocean-cli — OurOS DigitalOcean (developer-first cloud, NYC, NYSE:DOCN)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_do(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: digitalocean [OPTIONS]");
        println!("DigitalOcean (OurOS) — developer cloud (Droplets + App Platform + Managed services, NYSE:DOCN)");
        println!();
        println!("Options:");
        println!("  --droplets             Droplets (the iconic VMs)");
        println!("  --app-platform         App Platform (managed PaaS)");
        println!("  --kubernetes           DigitalOcean Kubernetes (DOKS)");
        println!("  --databases            Managed Databases (Postgres, MySQL, Redis, MongoDB, Kafka)");
        println!("  --spaces               Spaces (S3-compatible object storage)");
        println!("  --paperspace           Paperspace (acquired 2023, GPU + AI infra)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("DigitalOcean 2024 (OurOS) — doctl 1.x"); return 0; }
    println!("DigitalOcean 2024 (OurOS) — Developer Cloud Platform");
    println!("  Vendor: DigitalOcean Holdings, Inc. (New York, NY — NYSE:DOCN since 2021)");
    println!("  Founders: Ben Uretsky + Moisey Uretsky + Mitch Wainer + Jeff Carr + Alec Hartman, 2011");
    println!("          founded in NYC — early cloud-for-developers focus");
    println!("          'Droplet' = iconic 1-click VM (since 2013)");
    println!("          'SSD-only cloud' marketing during the early HDD era differentiated brand");
    println!("          Yancey Spruill: CEO 2019-2024, replaced by Paddy Srinivasan (former CTO of NetSuite)");
    println!("  Public market (NYSE:DOCN):");
    println!("         IPO Mar 2021 at $47/share — raised $750M");
    println!("         peak ~$130 late 2021");
    println!("         settled $30-50 range 2023-2024");
    println!("         FY2024 revenue: ~$770M (+13% YoY)");
    println!("         Market cap: $3-4B range");
    println!("         Operating margins improving (+20% adjusted EBITDA)");
    println!("         Activist investor Engaged Capital 2023 — pushed for AI growth");
    println!("  Strategic position: 'cloud for builders + SMB — simpler + cheaper than AWS':");
    println!("                    pitch: 'the developer cloud that doesn't punish you for not being a Fortune 500'");
    println!("                    target: indie devs + SMB + scale-ups");
    println!("                    primary competitor: AWS, GCP, Azure (for SMB segment)");
    println!("                    secondary: Linode (Akamai), Vultr, Hetzner, OVHcloud, Render");
    println!("                    DigitalOcean's wedge: predictable pricing + best DX + tutorials + ~600+ docs");
    println!("                    'AWS for the rest of us' = simplified UI + SKUs + pricing");
    println!("                    AI push: Paperspace acquisition $111M (Mar 2023) for GPU compute");
    println!("  Pricing (notably transparent + predictable):");
    println!("    Basic Droplet: $4-$96/mo (1-8 vCPU, 1-32GB RAM, SSD storage)");
    println!("    Premium Droplet: $6-$120/mo (more CPU/RAM/perf per dollar)");
    println!("    GPU Droplet: $0.61-$10/hr (A100, H100)");
    println!("    Managed Postgres: $15-$2000+/mo (single node to clusters)");
    println!("    Kubernetes (DOKS): $0 control plane, pay per worker node");
    println!("    Spaces: $5/mo (250GB + 1TB transfer)");
    println!("    App Platform: $5/mo per service (basic tier)");
    println!("    typically 30-50% cheaper than AWS for equivalent small-scale workloads");
    println!("  Product portfolio:");
    println!("    1. Droplets (the iconic compute):");
    println!("       - Standard, Premium, CPU-Optimized, Memory-Optimized, Storage-Optimized");
    println!("       - 14+ global data centers (NYC, SFO, AMS, FRA, BLR, SGP, SYD, TOR, LON)");
    println!("       - 1-click apps (WordPress, LAMP, Docker, Discourse, etc.)");
    println!("    2. App Platform (managed PaaS):");
    println!("       - Heroku-style git push deploys");
    println!("       - Auto-scaling + load balancing");
    println!("       - Build from GitHub/GitLab/Docker registry");
    println!("    3. DigitalOcean Kubernetes (DOKS):");
    println!("       - Managed K8s with free control plane");
    println!("       - 1-click clusters (similar UX to Droplets)");
    println!("       - Integration with Spaces + LBs + Volumes");
    println!("    4. Managed Databases:");
    println!("       - Postgres, MySQL, Redis, MongoDB, Kafka, OpenSearch");
    println!("       - HA replication + automated backups");
    println!("       - Connection pooling for Postgres");
    println!("    5. Spaces (S3-compatible object storage):");
    println!("       - 250GB + 1TB transfer base plan");
    println!("       - CDN built-in (Spaces CDN)");
    println!("       - S3 API for compatibility");
    println!("    6. Load Balancers + Floating IPs + Reserved IPs");
    println!("    7. Volumes (block storage):");
    println!("       - Up to 16TB per volume");
    println!("       - Snapshot + resize");
    println!("    8. Functions (serverless — was Cloud Functions):");
    println!("       - Apache OpenWhisk-based");
    println!("       - Node, Python, Go, PHP, Ruby runtimes");
    println!("    9. Paperspace (acquired 2023 $111M — the AI bet):");
    println!("       - GPU compute (A100, H100, V100, etc.)");
    println!("       - ML training + inference workloads");
    println!("       - Gradient ML platform");
    println!("       - Strategic for DO's GenAI play");
    println!("    10. Cloudways (managed hosting, acquired 2022 $350M):");
    println!("       - Managed WordPress + WooCommerce + Magento hosting");
    println!("       - Layer above raw Droplets/AWS/GCP infra");
    println!("       - Brings SMB hosting customers");
    println!("  Developer community (the moat):");
    println!("    - DigitalOcean tutorials: 6,000+ articles on Linux/Docker/K8s/etc.");
    println!("    - Among the highest-traffic developer documentation sites");
    println!("    - 'How To Install X on Ubuntu' = often the top SEO result");
    println!("    - Hatch program for early-stage startups (free credits)");
    println!("    - Community Q&A + open source contributions");
    println!("  Paperspace integration (AI strategy):");
    println!("    - Mar 2023: acquired Paperspace for $111M");
    println!("    - GPU compute + ML platform (Gradient)");
    println!("    - 'GPU Droplets' on DO marketplace by 2024");
    println!("    - Competes with Lambda Labs + Together AI + RunPod for GPU compute");
    println!("    - 1-click LLM deployment (e.g., 1-click Mistral, Llama)");
    println!("  Integrations:");
    println!("    - GitHub, GitLab, Docker Hub for App Platform builds");
    println!("    - Terraform + Pulumi + Crossplane providers");
    println!("    - 1-click apps: WordPress, Discourse, GitLab, MariaDB, MongoDB, etc.");
    println!("    - Marketplace: 350+ pre-configured Droplet images");
    println!("    - Monitoring: built-in DO Monitoring + Datadog + Grafana integrations");
    println!("    - Object storage: S3-compatible Spaces");
    println!("    - DNS: built-in DNS with API");
    println!("    - SDKs: Go (godo), Python, Ruby, Node, PHP, JavaScript");
    println!("  DigitalOcean CLI usage:");
    println!("    doctl auth init                                         # configure API token");
    println!("    doctl compute droplet create my-server --size s-1vcpu-1gb --image ubuntu-22-04-x64 --region nyc3");
    println!("    doctl compute droplet list");
    println!("    doctl compute droplet delete my-server");
    println!("    doctl kubernetes cluster create my-k8s --region nyc3 --size s-2vcpu-4gb --count 3");
    println!("    doctl apps create --spec app.yaml");
    println!("    doctl databases create my-db --engine pg --region nyc3 --size db-s-1vcpu-1gb");
    println!("    doctl spaces create my-bucket --region nyc3");
    println!("    doctl serverless deploy my-functions");
    println!("    doctl monitoring alert create --type cpu --threshold 80 --droplet 12345");
    println!("  Customers (650,000+ paying):");
    println!("    - SMB + indie devs + scale-ups (the iconic DO base)");
    println!("    - WordPress hosters + agencies (via Cloudways)");
    println!("    - International: 50%+ revenue from outside US");
    println!("    - GitLab (historically — moved off), Snyk, Brave Browser, Linode (acquired)");
    println!("    - 158K+ developer customers");
    println!("  Critique: enterprise feature gap vs AWS (no IAM Roles parity, no advanced VPC)");
    println!("           growth slowing as SMB cloud market matures");
    println!("           AI bet via Paperspace too early to assess success");
    println!("           App Platform behind Heroku/Render/Vercel in DX polish");
    println!("           Cloudways acquisition $350M was richly priced for managed hosting margins");
    println!("           multi-cloud customers spend more on AWS+GCP than DO");
    println!("           competition from Hetzner + OVHcloud in price-sensitive segments");
    println!("           Akamai's Linode acquisition compresses competitive set");
    println!("  Differentiator: predictable transparent pricing (Droplet SKUs since 2013) + 6,000+ tutorials = best developer documentation site + 650K+ paying customers + 1-click apps + Paperspace GPU acquisition for AI + Cloudways managed-hosting acquisition + ~$770M revenue + NYC-based developer-first brand — the cloud platform that SMBs + indie devs + scale-ups choose when AWS feels like overkill and they want simple pricing and great docs");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "digitalocean".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_do(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_do};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/digitalocean"), "digitalocean");
        assert_eq!(basename(r"C:\bin\digitalocean.exe"), "digitalocean.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("digitalocean.exe"), "digitalocean");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_do(&["--help".to_string()], "digitalocean"), 0);
        assert_eq!(run_do(&["-h".to_string()], "digitalocean"), 0);
        assert_eq!(run_do(&["--version".to_string()], "digitalocean"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_do(&[], "digitalocean"), 0);
    }
}
