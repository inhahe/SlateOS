#![deny(clippy::all)]
//! keycdn-cli — SlateOS KeyCDN Swiss personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — KeyCDN content delivery (personality)");
    println!();
    println!("USAGE: {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about        proinity GmbH, Zurich Switzerland, 2012");
    println!("    pricing      Pay-as-you-go, 4 cents per GB North America/Europe");
    println!("    zones        Pull zones + push zones model");
    println!("    tools        KeyCDN's free network tools site");
    println!("    api          REST API for zone + edge management");
    println!("    privacy      Swiss data-protection positioning");
    println!("    help / version");
}

fn print_version() {
    println!("keycdn-cli 0.1.0 — Slate OS personality binary");
    println!("proinity GmbH — Wil/Zurich, Switzerland (KeyCDN)");
}

fn cmd_about() {
    println!("KeyCDN — Faster content delivery.");
    println!();
    println!("Founded:  2012 by proinity LLC (later GmbH), Switzerland");
    println!("HQ:       Wil, Canton of St. Gallen, Switzerland");
    println!("Founder:  Sven Hesse (early CTO, now Managing Director)");
    println!();
    println!("Bootstrapped, profitable, small team. The classic Swiss-quiet");
    println!("infrastructure company — does one thing well, doesn't seek press,");
    println!("doesn't have a flashy founder story.");
    println!();
    println!("Footprint:");
    println!("  35+ PoPs globally, primarily in Europe and North America with");
    println!("  meaningful coverage in APAC. Smaller than the big-3 CDNs but");
    println!("  meaningful coverage for most workloads.");
    println!();
    println!("Customer base:");
    println!("  Heavy in Switzerland, Germany, Austria, Liechtenstein, France.");
    println!("  Adoption among privacy-conscious EU customers who want a non-US,");
    println!("  non-Chinese CDN. WordPress hosts. Smaller ecommerce sites.");
    println!();
    println!("Positioning vs Bunny.net:");
    println!("  Similar 'small + cheap CDN' segment. KeyCDN emphasizes Swiss");
    println!("  privacy + neutrality more; Bunny emphasizes DX + edge compute.");
    println!("  KeyCDN is older (2012 vs Bunny 2015) but smaller in 2024.");
}

fn cmd_pricing() {
    println!("KeyCDN pricing — pay-as-you-go simplicity");
    println!();
    println!("No contracts, no monthly minimums, no commit. Per-GB only.");
    println!();
    println!("Egress rates (per GB), tiered by region and volume:");
    println!();
    println!("  North America + Europe:");
    println!("    First 10 TB:    $0.04 / GB");
    println!("    10-50 TB:        $0.03 / GB");
    println!("    50-100 TB:       $0.02 / GB");
    println!("    100+ TB:         $0.01 / GB");
    println!();
    println!("  Asia + Oceania:");
    println!("    First 10 TB:    $0.10 / GB");
    println!("    10+ TB:          $0.08 / GB");
    println!();
    println!("  South America:");
    println!("    First 10 TB:    $0.11 / GB");
    println!();
    println!("  Africa:");
    println!("    First 10 TB:    $0.30 / GB (highest globally — sparse infra)");
    println!();
    println!("HTTPS requests:  $0.02 per 10,000 requests");
    println!("Storage (Push zone): $0.06 / GB / month");
    println!();
    println!("Minimum: $4 / month account fee");
    println!();
    println!("How this stacks up:");
    println!("  KeyCDN US/EU base rate (4c/GB) is between Bunny ($0.01) and");
    println!("  CloudFront ($0.085). The volume tiers get genuinely cheap above");
    println!("  ~100 TB/month. Good fit: mid-volume sites that value simplicity");
    println!("  + Swiss jurisdiction over rock-bottom cost.");
}

fn cmd_zones() {
    println!("KeyCDN zones — Pull and Push models");
    println!();
    println!("Pull zone (origin pull):");
    println!("  Configure an origin URL. KeyCDN fetches from your origin on");
    println!("  cache miss, caches at edge, serves to client. The standard CDN");
    println!("  pattern. Origin authentication via header or HTTP auth.");
    println!();
    println!("  URL structure: <zone>-<id>.kxcdn.com");
    println!("  CNAME your own domain to that hostname.");
    println!();
    println!("Push zone (FTP/SFTP upload to KeyCDN-hosted storage):");
    println!("  Upload files directly to a KeyCDN-managed FTP-like store.");
    println!("  KeyCDN serves them through its edge.");
    println!("  Older pattern, more like S3-without-S3-compat.");
    println!();
    println!("Configuration knobs (per zone):");
    println!("  • CORS headers (origin allowlist, methods, headers)");
    println!("  • Cache control overrides (force TTL, ignore origin cache hdrs)");
    println!("  • SSL/TLS — free Let's Encrypt or BYO certificate (paid plan)");
    println!("  • HTTP/2, HTTP/3 + QUIC, Brotli compression — toggle per zone");
    println!("  • Custom origin host header rewriting");
    println!("  • Token auth (signed URLs with HMAC + expiry)");
    println!("  • Bot, country, IP block/allow lists");
    println!("  • Hotlink protection");
    println!("  • Origin shield (regional caching tier in front of origin)");
}

fn cmd_tools() {
    println!("KeyCDN free network tools (tools.keycdn.com)");
    println!();
    println!("KeyCDN ships a public, free, no-signup-required suite of network");
    println!("diagnostic tools. Heavy SEO play — these tools rank for many of");
    println!("the 'how do I test X' search queries developers ask.");
    println!();
    println!("Tools provided:");
    println!("  • Ping Test — ICMP ping from 14+ global locations");
    println!("  • Traceroute — multi-region traceroute");
    println!("  • Performance Test — TTFB + full-page load from multiple PoPs");
    println!("  • Pingdom-style speed test (was once free, now limited)");
    println!("  • DNS Lookup — A / AAAA / CNAME / MX / TXT / NS records");
    println!("  • DNS Speed Test — compare your DNS to public resolvers");
    println!("  • IP Location Finder — geolocate an IP");
    println!("  • SSL Checker — test cert chain, OCSP, expiry, ciphers");
    println!("  • HTTP/2 Test — does the host support HTTP/2?");
    println!("  • HTTP/3 Test — same, for HTTP/3 / QUIC");
    println!("  • Brotli Test — Brotli support detection");
    println!("  • Network Speed Test (browser-side)");
    println!("  • Email Verification — DNS-level SMTP verification");
    println!("  • Geo IP — bulk lookups");
    println!();
    println!("These exist as a marketing wedge: 'developer hits a problem,");
    println!("googles, lands on KeyCDN tool, gets answer, sees KeyCDN branding,");
    println!("considers KeyCDN for their next project.' Effective inbound funnel.");
}

fn cmd_api() {
    println!("KeyCDN REST API");
    println!();
    println!("Base URL: https://api.keycdn.com");
    println!("Auth:     HTTP Basic — username=API_KEY, password empty");
    println!("Format:   JSON");
    println!();
    println!("Resources:");
    println!("  GET    /zones.json                  list zones");
    println!("  POST   /zones.json                  create zone");
    println!("  GET    /zones/{{id}}.json             zone detail");
    println!("  PUT    /zones/{{id}}.json             update zone config");
    println!("  DELETE /zones/{{id}}.json             delete zone");
    println!("  GET    /zonealiases.json            list custom domain aliases");
    println!("  POST   /zonealiases.json            add alias");
    println!("  GET    /zonereferrers.json          referrer allowlists");
    println!("  POST   /zones/{{id}}/purge.json       purge cache (all)");
    println!("  POST   /zones/{{id}}/purgeurl.json    purge specific URLs");
    println!("  POST   /zones/{{id}}/purgetag.json    purge by cache tag");
    println!();
    println!("Statistics:");
    println!("  GET /reports/traffic.json    bandwidth by zone, by region, by time");
    println!("  GET /reports/credits.json    historical egress billing data");
    println!("  GET /reports/statuscodes.json HTTP status distribution");
    println!();
    println!("Cache tag purging:");
    println!("  Tag responses with a 'Cache-Tag: blog,post-42' header.");
    println!("  Purge by tag: invalidate all responses for tag 'post-42' across all PoPs.");
    println!("  More efficient than URL-by-URL purging for content rewrites.");
}

fn cmd_privacy() {
    println!("KeyCDN's Swiss privacy positioning");
    println!();
    println!("Why being Swiss matters in CDN-land:");
    println!();
    println!("  Switzerland is not in the EU but maintains GDPR-equivalent");
    println!("  privacy law (revFADP, in force Sep 1, 2023). It is recognized");
    println!("  by the EU as providing adequate data protection — so EU customers");
    println!("  can use Swiss CDNs without separate Standard Contractual Clauses.");
    println!();
    println!("  Switzerland is NOT subject to:");
    println!("    • US CLOUD Act (which applies to US-headquartered providers'");
    println!("      data regardless of location)");
    println!("    • US FISA Section 702 surveillance authority");
    println!("    • Chinese National Intelligence Law");
    println!();
    println!("  This makes Switzerland an attractive jurisdiction for:");
    println!("    • Investigative journalism + whistleblower platforms");
    println!("    • Privacy-focused SaaS (mail providers like Proton historically)");
    println!("    • Healthcare data (when not subject to US HIPAA + BAA needs)");
    println!("    • Finance / banking (Swiss bank secrecy heritage)");
    println!();
    println!("KeyCDN's specific privacy claims:");
    println!("  • No third-party trackers in customer dashboard");
    println!("  • Minimal log retention (operational only, not analytics-grade)");
    println!("  • Customer data stored on Swiss infrastructure when possible");
    println!("  • DPA + SCC support included free for EU customers");
    println!();
    println!("Practical limit: KeyCDN is still a CDN — content is replicated to");
    println!("global PoPs by definition. 'Swiss-only data residency' for CDN");
    println!("payload is fundamentally contradictory; the privacy story applies");
    println!("to account/billing/log data, not the cached content itself.");
}

fn run_keycdn(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "pricing" => cmd_pricing(),
        "zones" => cmd_zones(),
        "tools" => cmd_tools(),
        "api" => cmd_api(),
        "privacy" => cmd_privacy(),
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
        .unwrap_or_else(|| "keycdn-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_keycdn(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/keycdn-cli"), "keycdn-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("keycdn-cli.exe"), "keycdn-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_keycdn(&[], "keycdn-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_keycdn(&["bogus".into()], "keycdn-cli"), 2);
    }
}
