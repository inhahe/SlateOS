#![deny(clippy::all)]
//! cdn77-cli — OurOS CDN77 Czech CDN personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — CDN77 content delivery + video (personality)");
    println!();
    println!("USAGE: {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about        Prague 2011, DataCamp / Datasys s.r.o");
    println!("    video        OTT video + live streaming focus");
    println!("    network      70+ PoP global Anycast network");
    println!("    pricing      Pay-as-you-go and high-volume tiers");
    println!("    edge         CDN77 Edge Computing (newer)");
    println!("    sister       Sister brand: CDN77 R&D, RPC.com, BugBee, etc.");
    println!("    help / version");
}

fn print_version() {
    println!("cdn77-cli 0.1.0 — OurOS personality binary");
    println!("DataCamp Limited / CDN77 — Prague, Czech Republic + London, UK");
}

fn cmd_about() {
    println!("CDN77 — Performance-driven content delivery.");
    println!();
    println!("Founded:  2011 in Prague, Czech Republic");
    println!("Parent:   DataCamp Limited (UK holdco) / Datasys s.r.o (Czech ops)");
    println!("          'CDN77' chosen partly for the Czech country code (CZ)");
    println!("          and partly for the 77 / lucky-number / palindrome aesthetic");
    println!();
    println!("Bootstrapped, profitable, privately held. Built without venture capital.");
    println!("Estimated 100-200 employees in 2024.");
    println!();
    println!("Original positioning:");
    println!("  General-purpose mid-market CDN. Like KeyCDN and BunnyCDN — but");
    println!("  CDN77 leaned harder into the OTT video + live streaming use case");
    println!("  early on (2014-2015), which became its differentiating niche.");
    println!();
    println!("Notable enterprise customers (publicly disclosed):");
    println!("  IGN, FAZE Clan media properties, OUI/SNCF (French rail),");
    println!("  Avast (Czech security giant), Veeam, AVG, multiple Czech telcos.");
    println!("  Strong base in CEE + DACH region, plus growing EMEA enterprise.");
    println!();
    println!("Differentiator:");
    println!("  Among the few CDN77 / mid-tier CDNs that offers proper origin");
    println!("  shield + multi-tier caching + live streaming optimization out");
    println!("  of the box, at substantially lower TCO than Akamai/Cloudfront.");
}

fn cmd_video() {
    println!("CDN77 video delivery — the differentiator");
    println!();
    println!("Video specifically is what CDN77 leans into. Their pitch deck has");
    println!("been 'best mid-market CDN for OTT' since ~2015.");
    println!();
    println!("Live streaming features:");
    println!("  • Live origin acceleration (sub-second segment delivery)");
    println!("  • Low-latency HLS / DASH (LL-HLS, LL-DASH support)");
    println!("  • DRM-passthrough (Widevine + FairPlay + PlayReady)");
    println!("  • Token-authenticated playback URLs (signed HMAC + expiry)");
    println!("  • Per-IP / per-session concurrency limits");
    println!("  • Geo-blocking (country / region / specific IP block)");
    println!("  • Hotlink protection (referrer checks)");
    println!();
    println!("VOD features:");
    println!("  • Origin-side video file optimization recommendations");
    println!("  • Adaptive bitrate manifest acceleration");
    println!("  • Range-request friendly caching for video seeking");
    println!("  • Bandwidth shaping (consistent delivery to avoid stuttering)");
    println!();
    println!("Origin Shield / Mid-tier:");
    println!("  Regional cache tier that absorbs origin requests. For live video");
    println!("  where 10,000 concurrent viewers in a region need the same segment,");
    println!("  one fetch from origin populates the regional cache, then edge");
    println!("  serves the rest. Origin-protection at scale.");
    println!();
    println!("RTMP -> HLS / DASH transmuxing:");
    println!("  Not transcoding (no compute), but repackaging RTMP push streams");
    println!("  into HLS / DASH segments for CDN distribution. Sub-second");
    println!("  latency target.");
    println!();
    println!("Notable use cases:");
    println!("  Esports streaming, regional sports broadcasters, news live blogs,");
    println!("  influencer creator subscription platforms, e-learning live cohorts.");
}

fn cmd_network() {
    println!("CDN77 network architecture");
    println!();
    println!("Footprint (mid-2024):");
    println!("  70+ PoPs globally — Europe, North America, APAC, LATAM, MENA.");
    println!("  Significantly denser than KeyCDN/BunnyCDN at the mid-tier scale.");
    println!();
    println!("Routing:");
    println!("  Anycast TCP — clients connect to nearest PoP by BGP propagation.");
    println!("  Self-built Anycast IPv4 + IPv6 ranges.");
    println!();
    println!("Tiered caching:");
    println!("  Edge -> Regional shield -> Origin");
    println!("  Configurable per-zone — origin shield can be turned off for low-");
    println!("  TTL dynamic content or on for high-fanout static assets.");
    println!();
    println!("Peering + transit:");
    println!("  Direct settlement-free peering at major IXPs (DE-CIX, AMS-IX,");
    println!("  LINX, NL-IX, MSK-IX legacy, Equinix Ashburn, NYIIX, etc.)");
    println!("  Transit from multiple Tier-1 providers for blended best paths.");
    println!();
    println!("Capacity (published claims):");
    println!("  ~100 Tbps aggregate network capacity. Realistic for the PoP");
    println!("  count and the typical CDN over-provisioning factor.");
    println!();
    println!("Compute density per PoP:");
    println!("  CDN77 leans toward fewer, larger PoPs vs Cloudflare's many-small.");
    println!("  Trade-off: deeper cache per location, less edge-locality for");
    println!("  ultra-latency-sensitive apps.");
    println!();
    println!("HTTP versioning:");
    println!("  HTTP/2 + HTTP/3 (QUIC) on by default for new zones.");
    println!("  TLS 1.3 + 0-RTT supported.");
}

fn cmd_pricing() {
    println!("CDN77 pricing");
    println!();
    println!("Pricing model:");
    println!("  • Pay-as-you-go for self-serve (small accounts)");
    println!("  • Volume commits for mid-market and enterprise");
    println!();
    println!("Self-serve (PAYG) typical rates (per GB):");
    println!("  Europe + North America:  ~$0.045 - 0.05 / GB");
    println!("  Asia + Oceania:           ~$0.10 / GB");
    println!("  South America + Africa:   ~$0.12 - 0.15 / GB");
    println!();
    println!("Commit pricing (negotiated, indicative):");
    println!("  At 100 TB/month commit:  ~$0.015 - 0.025 / GB blended");
    println!("  At 1 PB/month commit:    ~$0.005 - 0.012 / GB blended");
    println!("  At 10 PB/month commit:   single-digit / mil / GB blended");
    println!();
    println!("This puts CDN77 firmly in the 'mid-market enterprise' price band:");
    println!();
    println!("  Bunny.net      $0.01 / GB (cheapest PAYG)");
    println!("  CDN77 PAYG     $0.045 / GB");
    println!("  KeyCDN         $0.04 / GB");
    println!("  CloudFront     $0.085 / GB (US/EU on-demand)");
    println!("  Cloudflare biz custom (with bandwidth alliance offsets)");
    println!("  Fastly         $0.12 / GB (US/EU on-demand)");
    println!("  Akamai         negotiated, enterprise only");
    println!();
    println!("CDN77's commit pricing for >100 TB customers is genuinely");
    println!("competitive with Akamai and substantially below Fastly's list.");
    println!("This is the sweet spot of the business.");
}

fn cmd_edge() {
    println!("CDN77 Edge Computing");
    println!();
    println!("CDN77's edge-compute offering arrived later than Cloudflare Workers");
    println!("(2018), Fastly Compute@Edge (2020), or Akamai EdgeWorkers (2020).");
    println!();
    println!("Edge Compute features:");
    println!("  • V8 isolate-based JS runtime, similar architecture to peers");
    println!("  • Request/response transformation");
    println!("  • A/B testing logic at the edge");
    println!("  • Custom auth (signed-URL checks, JWT validation)");
    println!("  • Header rewriting, redirects, country-based response variants");
    println!("  • Limited KV-store for state");
    println!();
    println!("Programming model is JavaScript / TypeScript. CDN77 publishes");
    println!("an SDK + local emulator for testing functions before deploy.");
    println!();
    println!("Honest positioning:");
    println!("  CDN77 is not trying to compete with Cloudflare Workers for");
    println!("  developer-platform mindshare. The edge compute serves existing");
    println!("  CDN customers who want light request manipulation without");
    println!("  going to a separate compute platform. It's a feature of the CDN,");
    println!("  not a serverless platform unto itself.");
    println!();
    println!("Use cases that fit:");
    println!("  • Token rewriting for video URL signing");
    println!("  • Origin selector logic (multi-region origin routing)");
    println!("  • Lightweight personalization (currency, language redirect)");
    println!("  • Anti-scraping header checks");
    println!();
    println!("Use cases that DON'T fit:");
    println!("  Anything stateful at scale — CDN77 KV is not a serious database.");
    println!("  Anything compute-heavy — V8 isolate CPU limits apply.");
    println!("  Anything requiring large NPM ecosystems — modules are limited.");
}

fn cmd_sister() {
    println!("CDN77 sister brands and broader DataCamp / Datasys orbit");
    println!();
    println!("DataCamp Limited (UK holdco):");
    println!("  Parent legal entity. Holds the CDN77 brand and adjacent IP.");
    println!("  (Note: NOT the same as DataCamp Inc., the online-learning company.");
    println!("  Pure name collision; entirely separate businesses.)");
    println!();
    println!("Datasys s.r.o (Czech operating company):");
    println!("  Prague-based engineering and ops home for the actual CDN77 service.");
    println!();
    println!("Related ventures across the years (varying degrees of formal connection):");
    println!();
    println!("  CDN77 R&D:");
    println!("    Internal research arm. Open-sources occasional Go / Rust tooling.");
    println!();
    println!("  Plynet, Tezbox (legacy, names may have changed):");
    println!("    Early adjacent services — payment infra and crypto wallet tech");
    println!("    that existed in the broader Czech tech holding orbit.");
    println!();
    println!("  AVG, Avast connections:");
    println!("    Czech security industry is small. CDN77 leadership has historical");
    println!("    overlap with the Avast/AVG ecosystem alumni. Czech tech is tight.");
    println!();
    println!("Key point:");
    println!("  CDN77 is a Czech-founded, Czech-engineered, UK-incorporated CDN");
    println!("  that became a credible global mid-tier delivery platform without");
    println!("  raising venture capital. It's a model worth studying: focused");
    println!("  vertical (video), targeted geography (CEE + EU + global enterprise),");
    println!("  disciplined growth, ~13 years of compound execution. Quiet success.");
}

fn run_cdn77(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "video" => cmd_video(),
        "network" => cmd_network(),
        "pricing" => cmd_pricing(),
        "edge" => cmd_edge(),
        "sister" => cmd_sister(),
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
        .unwrap_or_else(|| "cdn77-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_cdn77(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cdn77-cli"), "cdn77-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cdn77-cli.exe"), "cdn77-cli");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_cdn77(&[], "cdn77-cli"), 0);
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_cdn77(&["bogus".into()], "cdn77-cli"), 2);
    }
}
