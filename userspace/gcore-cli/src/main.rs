#![deny(clippy::all)]
//! gcore-cli — OurOS Gcore (Luxembourg edge + ML inference) personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Gcore edge cloud + AI inference (personality)");
    println!();
    println!("USAGE: {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about         Andre Reitenbach, Luxembourg 2014");
    println!("    products      CDN + Cloud + AI + Streaming + Security stack");
    println!("    ai            Gcore AI Inference (sub-50ms LLM inference at edge)");
    println!("    streaming     Streaming platform (heritage from G-Core Labs / WG)");
    println!("    gaming        Wargaming.net heritage and gaming infrastructure");
    println!("    network       180+ PoP global Anycast network");
    println!("    cloud         Edge cloud (VMs, Kubernetes, bare metal)");
    println!("    help / version");
}

fn print_version() {
    println!("gcore-cli 0.1.0 — OurOS personality binary");
    println!("Gcore — Luxembourg (headquarters); R&D distributed across Europe");
}

fn cmd_about() {
    println!("Gcore — The edge AI platform.");
    println!();
    println!("Founded:  2014");
    println!("HQ:       Luxembourg");
    println!("Founder:  Andre Reitenbach (still CEO)");
    println!();
    println!("Origin:");
    println!("  Originally 'G-Core Labs', built as the infrastructure backbone");
    println!("  for Wargaming.net (the publisher of World of Tanks). The company");
    println!("  grew up running CDN + live ops + matchmaking for very large");
    println!("  online games before pivoting to a commercial edge cloud business.");
    println!();
    println!("Rebrand:");
    println!("  G-Core Labs -> Gcore around 2022. Cleaner brand for B2B sales");
    println!("  outside the gaming-tech orbit.");
    println!();
    println!("Funding:");
    println!("  Largely bootstrapped + reinvested operating profit historically.");
    println!("  Jun 2023: USD 60M Series A from Wargaming + others");
    println!("  Reported expansion of additional rounds since (terms private).");
    println!();
    println!("Headcount:");
    println!("  ~600-700 employees as of 2024, distributed across Europe.");
    println!();
    println!("Positioning:");
    println!("  Full-stack edge cloud — CDN + edge compute + streaming + cloud");
    println!("  hosting + AI inference, sold as a unified platform. One of the");
    println!("  few independent European-headquartered edge clouds at scale.");
}

fn cmd_products() {
    println!("Gcore product portfolio");
    println!();
    println!("Edge Delivery:");
    println!("  • CDN — global Anycast network, 180+ PoPs");
    println!("  • Streaming Platform — VOD + Live, transcoding, DRM");
    println!("  • DNS — authoritative DNS + GeoDNS");
    println!();
    println!("Edge Cloud:");
    println!("  • Virtual Machines — Linux + Windows, multi-region");
    println!("  • Bare Metal Servers — dedicated hardware on demand");
    println!("  • Managed Kubernetes — multi-region clusters");
    println!("  • Object Storage — S3-compatible");
    println!("  • Block + File Storage — high-IOPS volumes");
    println!("  • Load Balancers — L4/L7, global anycast options");
    println!();
    println!("Edge AI (the strategic bet):");
    println!("  • AI Inference at Edge (Everywhere Inference)");
    println!("  • Managed GPU clusters (H100, A100, MI300X)");
    println!("  • Bare-metal AI workstations");
    println!("  • LLM API endpoint (hosted open-source models)");
    println!();
    println!("Security:");
    println!("  • DDoS Protection — L3/L4/L7, anycast scrubbing");
    println!("  • WAF — OWASP rules + custom rules engine");
    println!("  • Bot Protection");
    println!();
    println!("FastEdge (edge functions):");
    println!("  WebAssembly + JS runtime at edge, similar to Cloudflare Workers");
    println!("  but heavier on Wasm support for non-JS languages.");
    println!();
    println!("Strategy: bundle all of these for enterprise contracts. Sell against");
    println!("multi-cloud + AWS + Cloudflare on price, region density, and the");
    println!("integrated AI inference story (which is genuinely differentiated).");
}

fn cmd_ai() {
    println!("Gcore AI Inference — the strategic pivot");
    println!();
    println!("What it is:");
    println!("  GPU-backed inference for ML models, deployed across Gcore's edge");
    println!("  PoPs and bare-metal regions. Sub-50ms response time for many");
    println!("  LLM and vision workloads thanks to geographic proximity.");
    println!();
    println!("Gcore Everywhere Inference:");
    println!("  Run YOUR model (custom fine-tune or HuggingFace OSS) on Gcore's");
    println!("  managed GPU infrastructure with smart routing — requests land");
    println!("  on the nearest PoP with available GPU capacity.");
    println!();
    println!("Catalog of hosted open-source models (LLM API style):");
    println!("  Llama 3 (8B, 70B), Mistral, Mixtral, Phi-3, Qwen,");
    println!("  Stable Diffusion family, FLUX (image gen), Whisper (speech)");
    println!();
    println!("Pricing model:");
    println!("  Per-token (for LLM API) or per-GPU-hour (for managed clusters).");
    println!("  Typically 30-50% cheaper than equivalent on AWS/GCP for the same");
    println!("  GPU class, with the trade-off of less ecosystem (no Bedrock,");
    println!("  no Vertex AI, etc.).");
    println!();
    println!("Why this matters:");
    println!("  Inference latency is becoming a product-defining constraint —");
    println!("  agentic apps, voice assistants, real-time content moderation all");
    println!("  need sub-100ms model calls. Gcore's edge density advantage for");
    println!("  inference is its strongest differentiation against AWS/Azure GCP.");
    println!();
    println!("Hardware partnerships:");
    println!("  NVIDIA H100 + H200 clusters in multiple regions.");
    println!("  AMD MI300X clusters announced 2024 (multi-vendor GPU strategy).");
    println!("  Intel Gaudi cluster trials.");
}

fn cmd_streaming() {
    println!("Gcore Streaming Platform");
    println!();
    println!("Heritage:");
    println!("  Streaming was always core to Gcore's identity — built originally");
    println!("  to handle Wargaming's gameplay video + esports broadcasts +");
    println!("  in-game asset delivery.");
    println!();
    println!("Streaming Platform features:");
    println!("  • Live ingest (RTMP, SRT push)");
    println!("  • Cloud transcoding (HLS + DASH + LL-HLS)");
    println!("  • Per-title encoding (analyze content, optimize bitrate ladder)");
    println!("  • Live origin shield + multi-region failover");
    println!("  • White-label player web component");
    println!("  • DRM passthrough (Widevine, FairPlay, PlayReady)");
    println!("  • Token-authenticated playback URLs (signed expiry)");
    println!("  • Geo-blocking, concurrent-stream limits, hotlink protection");
    println!("  • Live recording -> auto-VOD pipeline");
    println!("  • Analytics: concurrent viewers, region, device, drop-off");
    println!();
    println!("Use cases:");
    println!("  • Live sports broadcasters (regional rights compliance)");
    println!("  • Esports event streaming (low-latency, high concurrency)");
    println!("  • Corporate town halls + B2B webinars (private streaming)");
    println!("  • OTT subscription services (DRM + concurrency control)");
    println!("  • Education + e-learning live cohorts");
    println!();
    println!("Comparable to:");
    println!("  Mux (more developer-focused but pricier),");
    println!("  Wowza Streaming Cloud (older, on-prem-friendly),");
    println!("  Bunny Stream (cheaper but lighter on enterprise features),");
    println!("  AWS Elemental MediaLive + MediaPackage (more flexible, more complex).");
}

fn cmd_gaming() {
    println!("Gcore's gaming infrastructure heritage");
    println!();
    println!("Wargaming.net relationship:");
    println!("  Gcore's predecessor company (G-Core Labs) was originally the");
    println!("  in-house infrastructure organization spun out of Wargaming.net,");
    println!("  publisher of World of Tanks (one of the largest free-to-play");
    println!("  MMO games globally, peak ~100M registered users).");
    println!();
    println!("  Wargaming HQ history is a separate story — moved from Belarus");
    println!("  to Cyprus pre-2022, then further restructured post-Feb 2022.");
    println!("  Gcore became fully independent of any war-zone jurisdiction.");
    println!();
    println!("Gaming-specific infrastructure capabilities (still differentiating):");
    println!("  • Game build distribution (multi-TB game patches to millions");
    println!("    of clients concurrently — classic CDN flash-crowd workload)");
    println!("  • Game server hosting (low-latency bare-metal in 25+ regions");
    println!("    for matchmaking against geographic latency requirements)");
    println!("  • DDoS mitigation tuned for gaming (rule sets that account for");
    println!("    UDP-heavy traffic, latency-critical sessions, persistent");
    println!("    real-time connections — different from web HTTP traffic)");
    println!("  • Anti-cheat infra partnerships");
    println!();
    println!("Gaming customers (publicly disclosed):");
    println!("  Wargaming (founding customer, ongoing), G5 Entertainment,");
    println!("  Sandbox Interactive (Albion Online), many indie + mid-tier");
    println!("  game studios across Europe and Asia.");
    println!();
    println!("The gaming workload type is genuinely different from generic web");
    println!("CDN, and the operational maturity of running World-of-Tanks-class");
    println!("traffic for a decade is hard to replicate. This is Gcore's moat");
    println!("relative to newer entrants.");
}

fn cmd_network() {
    println!("Gcore network architecture");
    println!();
    println!("Footprint:");
    println!("  180+ PoPs globally (as of mid-2024). Among the densest");
    println!("  non-hyperscaler edge networks in the industry, comparable to");
    println!("  or larger than Fastly in PoP count.");
    println!();
    println!("Strong regions:");
    println!("  • Europe — very dense across Germany, France, Netherlands, UK,");
    println!("    Scandinavia, plus eastern European cities");
    println!("  • Russia + CIS — historic Wargaming legacy, still operating");
    println!("    (subject to sanctions compliance)");
    println!("  • Middle East — Saudi Arabia, UAE, Israel PoPs");
    println!("  • Asia — Japan, Singapore, Hong Kong, India, Malaysia, Thailand");
    println!("  • Africa — South Africa, Nigeria (Lagos)");
    println!("  • LATAM — Brazil, Argentina, Chile, Mexico");
    println!("  • North America — multiple US cities + Canada");
    println!();
    println!("Network capacity (published claim):");
    println!("  ~200 Tbps aggregate. Modern enterprise CDN tier.");
    println!();
    println!("Routing:");
    println!("  Anycast IPv4 + IPv6 prefixes, BGP-propagated.");
    println!("  HTTP/2 + HTTP/3 (QUIC) at edge.");
    println!("  TLS 1.3 with 0-RTT.");
    println!();
    println!("Tiered caching:");
    println!("  Configurable origin shield, regional cache tier, edge cache.");
    println!();
    println!("Peering + transit:");
    println!("  Direct peering at major IXPs (DE-CIX, AMS-IX, LINX, EPIX);");
    println!("  multiple Tier-1 transit providers; cloud-direct interconnects");
    println!("  to AWS / Azure / GCP for hybrid origin scenarios.");
}

fn cmd_cloud() {
    println!("Gcore Edge Cloud — IaaS at the edge");
    println!();
    println!("This is where Gcore goes beyond CDN-with-edge-functions into");
    println!("full IaaS-at-the-edge territory.");
    println!();
    println!("Compute:");
    println!("  • Virtual Machines — KVM-based, multiple Linux distros + Windows");
    println!("  • Bare Metal — dedicated servers, custom configurations,");
    println!("                 GPU-accelerated options for AI workloads");
    println!("  • Managed Kubernetes — multi-region clusters with built-in");
    println!("                          load balancers + storage CSI");
    println!("  • Function-as-a-Service (FastEdge) — Wasm + JS runtime");
    println!();
    println!("Storage:");
    println!("  • Object Storage — S3-compatible, multi-region replication");
    println!("  • Block Storage — high-IOPS volumes attachable to VMs");
    println!("  • File Storage — NFS-mountable volumes");
    println!("  • Backup as a Service");
    println!();
    println!("Networking:");
    println!("  • Software-defined private networks (VPC equivalent)");
    println!("  • Cross-region private links");
    println!("  • Cloud-direct interconnects to AWS / Azure / GCP / Equinix");
    println!("  • Load balancers, NATs, VPN gateways");
    println!();
    println!("Pricing:");
    println!("  Per-hour for VMs (with monthly commit discount available).");
    println!("  Storage and egress separately metered.");
    println!("  Generally priced 20-40% below equivalent AWS/Azure/GCP on US/EU");
    println!("  regions, with steeper discounts in less competitive geographies.");
    println!();
    println!("Sweet spot:");
    println!("  European or non-US enterprise that wants multi-region cloud");
    println!("  without lock-in to a US hyperscaler AND wants the edge CDN +");
    println!("  AI inference + cloud compute on one bill, one console, one SLA.");
}

fn run_gcore(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "products" => cmd_products(),
        "ai" => cmd_ai(),
        "streaming" => cmd_streaming(),
        "gaming" => cmd_gaming(),
        "network" => cmd_network(),
        "cloud" => cmd_cloud(),
        "help" | "--help" | "-h" => print_help(prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'");
            eprintln!("Try '{prog} help' for the list of subcommands.");
            return 2;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "gcore-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_gcore(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gcore-cli"), "gcore-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gcore-cli.exe"), "gcore-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_gcore(&[], "gcore-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_gcore(&["bogus".into()], "gcore-cli"), 2);
    }
}
