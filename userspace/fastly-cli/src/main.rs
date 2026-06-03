#![deny(clippy::all)]

//! fastly-cli — OurOS Fastly (edge cloud + Compute@Edge Wasm, San Francisco, NYSE:FSLY)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fastly(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fastly [OPTIONS]");
        println!("Fastly (OurOS) — edge cloud platform (CDN + Compute@Edge Wasm + security, NYSE:FSLY)");
        println!();
        println!("Options:");
        println!("  --compute              Compute@Edge (WebAssembly serverless at edge)");
        println!("  --cdn                  CDN (programmable, instant purge)");
        println!("  --waf                  Next-Gen WAF (Signal Sciences acquisition)");
        println!("  --image-optimizer      Image Optimizer (Glitch acquisition)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Fastly 2024 (OurOS) — fastly CLI 10.x"); return 0; }
    println!("Fastly 2024 (OurOS) — Edge Cloud Platform");
    println!("  Vendor: Fastly, Inc. (San Francisco, CA — NYSE:FSLY since 2019)");
    println!("  Founders: Artur Bergman (CEO/CTO until 2020), 2011");
    println!("          Artur Bergman: ex-Wikia CTO, Perl hacker, beloved/eccentric founder figure");
    println!("          'Make the Internet faster' — original Varnish-based CDN bet");
    println!("          Joshua Bixby: CEO 2020-2024, replaced by Todd Nightingale (former Cisco Meraki)");
    println!("  Public market (NYSE:FSLY):");
    println!("         IPO May 2019 at $16/share — raised $180M");
    println!("         peak ~$120 late 2020 (COVID work-from-home edge boom)");
    println!("         settled $4-12 range 2023-2024 (deep drawdown)");
    println!("         FY2024 revenue: ~$543M (+8% YoY — modest growth)");
    println!("         Market cap: $1-2B range");
    println!("         June 2021: 1-hour global outage took down Reddit/Amazon/UK gov + caused stock pop ironically");
    println!("  Strategic position: 'edge cloud + Wasm compute — programmable edge for developers':");
    println!("                    pitch: 'a developer-first edge cloud — instant purge, Compute@Edge Wasm, real-time logs'");
    println!("                    target: media + ecommerce + developer-heavy teams that need programmable CDN");
    println!("                    primary competitor: Akamai (incumbent), Cloudflare (modern), AWS CloudFront");
    println!("                    secondary: Bunny.net, KeyCDN, StackPath");
    println!("                    Fastly's wedge: real-time config push (~150ms), instant purge, VCL programmability, Wasm compute");
    println!("                    longtime developer + ops favorite for high-control edge use cases");
    println!("                    challenge: Cloudflare's bigger network + aggressive free tier squeezes the market");
    println!("  Pricing (usage-based, no free tier for production):");
    println!("    Free trial: $50 credit (developer accounts)");
    println!("    CDN: $0.12/GB North America/Europe traffic, $0.0075 per 10K requests");
    println!("    Compute@Edge: $0.50 per 1M requests + compute time billing");
    println!("    Next-Gen WAF (Signal Sciences): custom + per-request");
    println!("    typically more expensive than Cloudflare but historically more programmable");
    println!("    targets media/ecommerce companies who can afford premium for control");
    println!("  Product portfolio:");
    println!("    1. CDN (the original, Varnish-based):");
    println!("       - Built on customized Varnish Cache");
    println!("       - VCL (Varnish Config Language) for edge logic");
    println!("       - Instant purge (~150ms globally — best-in-class)");
    println!("       - Real-time logs streaming");
    println!("       - 80+ POPs globally");
    println!("    2. Compute@Edge (the Wasm bet):");
    println!("       - WebAssembly-based serverless compute at edge");
    println!("       - Languages: Rust, JavaScript, AssemblyScript, Go (TinyGo)");
    println!("       - Lucet runtime (Fastly open-source Wasm compiler)");
    println!("       - ~35us cold start (faster than Lambda, comparable to Workers)");
    println!("       - Strong developer focus + dev tools");
    println!("    3. Next-Gen WAF (Signal Sciences, $775M acquisition Sep 2020):");
    println!("       - Application protection (RASP-style, learns app behavior)");
    println!("       - Replaced legacy ModSecurity-style WAFs");
    println!("       - Strong developer/SRE adoption");
    println!("    4. DDoS Protection:");
    println!("       - Built into platform (no separate SKU)");
    println!("       - Network-level mitigation at POPs");
    println!("    5. Bot Management:");
    println!("       - Bot detection + mitigation");
    println!("       - Account takeover protection");
    println!("    6. Image Optimizer (Glitch, $80M acq for Glitch + IO):");
    println!("       - Real-time image transformation");
    println!("       - WebP/AVIF auto-conversion");
    println!("       - Compete with: Cloudinary, imgix");
    println!("    7. Object Storage (recently added):");
    println!("       - S3-compatible object storage at edge");
    println!("       - Aimed at Compute@Edge developers");
    println!("    8. Live Streaming + Media Optimization:");
    println!("       - Strong media customer base (live sports, news)");
    println!("       - Origin shielding, multi-CDN strategies");
    println!("       - Customers like ESPN, BBC, NYTimes");
    println!("    9. Edge KV Store (preview):");
    println!("       - Distributed KV store for Compute@Edge");
    println!("       - Compete with: Cloudflare KV, Vercel KV");
    println!("  Compute@Edge architecture (the Wasm bet):");
    println!("    - WebAssembly + Lucet compiler (Fastly open-source)");
    println!("    - Compiles ahead-of-time to native code = no JIT overhead");
    println!("    - ~35us cold start (vs ~5ms Cloudflare Workers, vs 100-500ms Lambda)");
    println!("    - Compute@Edge language SDK in Rust, JS, Go, AssemblyScript");
    println!("    - Differs from Cloudflare's V8-isolate approach: more language flexibility, slightly slower bigger memory");
    println!("    - Open-source Lucet contributed to Bytecode Alliance");
    println!("  The June 8 2021 outage:");
    println!("    - A single customer config change triggered a bug → 1-hour global Fastly outage");
    println!("    - Reddit, Amazon, GitHub, NYTimes, UK gov sites went down");
    println!("    - Stock paradoxically *rose* afterward — proved Fastly was infrastructure for half the internet");
    println!("    - Post-mortem widely praised for transparency");
    println!("  Signal Sciences acquisition (Sep 2020 $775M):");
    println!("    - Signal Sciences had built next-gen WAF (no rule tuning, behavioral analysis)");
    println!("    - Fastly's WAF answer to Cloudflare/Akamai");
    println!("    - Integrated as 'Next-Gen WAF' but somewhat siloed");
    println!("    - Andrew Peterson (Signal Sciences co-founder) departed 2022");
    println!("  Integrations:");
    println!("    - Fastly CLI for Compute@Edge + config");
    println!("    - Terraform + Pulumi providers");
    println!("    - GitHub Actions for Compute@Edge deploys");
    println!("    - Real-time log streaming: S3, GCS, Splunk, Datadog, NewRelic, Logentries");
    println!("    - Open standards: HTTP/3, QUIC, Brotli");
    println!("    - SDKs: Rust, JS, Go, AssemblyScript for Compute@Edge");
    println!("    - VCL marketplace + community snippets");
    println!("  Fastly CLI usage:");
    println!("    fastly auth-token create                                # auth");
    println!("    fastly service create --name=my-cdn --type=vcl");
    println!("    fastly compute init                                      # scaffold Compute@Edge project");
    println!("    fastly compute build                                     # compile Wasm");
    println!("    fastly compute deploy                                    # deploy to edge");
    println!("    fastly purge --service=SERVICE_ID --all                  # instant purge");
    println!("    fastly logging s3 create --service=ID --name=my-logs --bucket=my-bucket");
    println!("    fastly stats realtime --service=ID                       # real-time analytics");
    println!("  Customers (media + ecommerce + dev-heavy):");
    println!("    - Major: Spotify, Stripe, GitHub, Shopify, NYTimes, Reddit (until 2021 outage), Etsy, Pinterest");
    println!("    - Media: BBC, ESPN, Vox, BuzzFeed");
    println!("    - Ecommerce: Wayfair, Ticketmaster (some)");
    println!("    - Sweet spot: high-traffic media + dev-savvy ecommerce");
    println!("    - 3,400+ customers (Q4 2024)");
    println!("  Critique: Cloudflare's free tier + bigger network compressing Fastly's market");
    println!("           Compute@Edge slower to traction than Cloudflare Workers");
    println!("           growth deceleration: 38% (2020) → 22% (2022) → 8% (2024)");
    println!("           Signal Sciences integration ongoing — siloed from CDN console");
    println!("           CEO turnover (Bergman → Bixby → Nightingale) signals strategy churn");
    println!("           June 2021 outage hurt brand among CDN risk-averse buyers");
    println!("           premium pricing vs free Cloudflare = uphill battle for new customers");
    println!("           net retention rate declining ($170M+ → $115M+ NRR over 3 years)");
    println!("  Differentiator: ~35us Wasm cold start (Lucet) + 150ms global instant purge + VCL programmability (most powerful edge config language) + Signal Sciences Next-Gen WAF (RASP-style) + real-time logging + media-heavy customer base (Spotify, NYTimes, GitHub, Stripe) + open-source Lucet/Bytecode Alliance contributions + ~$543M revenue — the developer-first edge cloud that prioritizes programmability and Wasm-language flexibility over Cloudflare's V8-only Workers, used by media and ecommerce companies who need ultimate edge control");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fastly".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fastly(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fastly};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fastly"), "fastly");
        assert_eq!(basename(r"C:\bin\fastly.exe"), "fastly.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fastly.exe"), "fastly");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_fastly(&["--help".to_string()], "fastly"), 0);
        assert_eq!(run_fastly(&["-h".to_string()], "fastly"), 0);
        assert_eq!(run_fastly(&["--version".to_string()], "fastly"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_fastly(&[], "fastly"), 0);
    }
}
