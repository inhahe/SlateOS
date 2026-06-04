#![deny(clippy::all)]
//! azion-cli — OurOS Azion (Brazilian edge platform) personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Azion edge computing platform (personality)");
    println!();
    println!("USAGE: {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about         Rafael Umann + cofounders 2011, Sao Paulo");
    println!("    latam         LATAM-first network footprint");
    println!("    edge          Edge Functions, Edge Application, Edge SQL");
    println!("    products      Edge platform product portfolio");
    println!("    open          Open-source bet (jsrs.js, edge starters)");
    println!("    customers     Brazilian and LATAM enterprise base");
    println!("    help / version");
}

fn print_version() {
    println!("azion-cli 0.1.0 — OurOS personality binary");
    println!("Azion Technologies — Sao Paulo, Brazil");
}

fn cmd_about() {
    println!("Azion — Build at the edge.");
    println!();
    println!("Founded:  2011 in Sao Paulo, Brazil");
    println!("Founders: Rafael Umann (CEO) and co-founders");
    println!();
    println!("Backers (publicly disclosed rounds):");
    println!("  2011-2014: bootstrap + Brazilian local angels");
    println!("  2015:      Series A from Vivo Ventures (Telefonica Brazil)");
    println!("  2018:      Series B undisclosed");
    println!("  2022:      Strategic round from Crescera Capital + others");
    println!("             (terms undisclosed; reported ~9-figure BRL)");
    println!();
    println!("Positioning:");
    println!("  The 'LATAM-native edge platform' — Brazil-headquartered, with");
    println!("  dense network in LATAM and growing PoPs in North America +");
    println!("  Europe + APAC. Edge computing pivot from CDN since ~2018.");
    println!();
    println!("Headcount: ~300-400 employees as of 2024.");
    println!();
    println!("Why this matters:");
    println!("  LATAM internet infrastructure is uniquely challenging. Long");
    println!("  international subsea cable hops, asymmetric peering with North");
    println!("  America, and complex per-country tax + privacy regimes (Brazilian");
    println!("  LGPD, Mexican LFPDPPP, Colombian Law 1581, etc.).");
    println!();
    println!("  Azion built around these realities. North American CDNs treat");
    println!("  LATAM as a tail market; Azion treats it as the primary one.");
}

fn cmd_latam() {
    println!("Azion's LATAM-first network and product");
    println!();
    println!("Network footprint:");
    println!("  Strongest density in LATAM:");
    println!("    • Brazil: SAO (Sao Paulo, multiple), RIO, BSB (Brasilia),");
    println!("              FOR (Fortaleza), POA (Porto Alegre), BHZ, REC, MAO");
    println!("    • Argentina: EZE (Buenos Aires)");
    println!("    • Chile: SCL (Santiago)");
    println!("    • Colombia: BOG (Bogota)");
    println!("    • Peru: LIM (Lima)");
    println!("    • Mexico: MEX (Mexico City), QRO (Queretaro)");
    println!();
    println!("  Plus PoPs in major North American, European, and APAC cities");
    println!("  for global delivery and origin-shielding.");
    println!();
    println!("LATAM specifics:");
    println!("  • In-country presence matters for Brazilian LGPD compliance");
    println!("    (transferring personal data abroad is restricted)");
    println!("  • Brazil has unusual internet routing (~20% of traffic stays");
    println!("    domestic; the rest hops via Miami)");
    println!("  • Azion peers directly at IX.br (PTT Sao Paulo, world's largest");
    println!("    IXP by member count)");
    println!("  • PIX (Brazilian instant payments) integration for billing");
    println!("    domestic Brazilian customers");
    println!();
    println!("Compare to North American CDNs in LATAM:");
    println!("  Cloudflare has SAO + Brazilian PoPs but routes via the US for");
    println!("  origin shield. Akamai has presence but limited regional density.");
    println!("  CloudFront has SAO, BUE, SCL, but limited country coverage.");
    println!();
    println!("Azion's value for LATAM customers: traffic stays in-region, which");
    println!("matters for latency AND for data-residency compliance.");
}

fn cmd_edge() {
    println!("Azion Edge Functions and Edge Application");
    println!();
    println!("Edge Functions:");
    println!("  Serverless JavaScript at edge PoPs. V8 isolate runtime.");
    println!("  Standard Service Worker fetch-event API (request -> response).");
    println!();
    println!("  addEventListener('fetch', event => {{");
    println!("    event.respondWith(handle(event.request));");
    println!("  }});");
    println!();
    println!("  async function handle(req) {{");
    println!("    const url = new URL(req.url);");
    println!("    if (url.pathname.startsWith('/api/')) {{");
    println!("      return new Response('hello from edge');");
    println!("    }}");
    println!("    return fetch(req); // pass through to origin");
    println!("  }}");
    println!();
    println!("Edge Application:");
    println!("  Higher-level abstraction. An Edge Application bundles:");
    println!("  • A primary origin (or multiple, for routing)");
    println!("  • Rule-engine pipeline (request match -> actions)");
    println!("  • Optional Edge Functions hooked into the pipeline");
    println!("  • Cache settings, headers, WAF policies");
    println!("  • A domain (CNAME target you point your DNS at)");
    println!();
    println!("Edge SQL (newer, 2023-2024):");
    println!("  SQLite-flavored database replicated to edge PoPs. Eventual");
    println!("  consistency for reads; primary writes to a central region.");
    println!("  Similar concept to Cloudflare D1 or Turso. Edge-local query");
    println!("  for personalization or feature-flag lookup without origin round-trip.");
    println!();
    println!("Edge KV:");
    println!("  Key-value store, similar to Cloudflare KV. Eventually consistent");
    println!("  globally; reads are edge-local. Bounded value size.");
}

fn cmd_products() {
    println!("Azion product portfolio");
    println!();
    println!("Delivery + acceleration:");
    println!("  • Edge Cache — global Anycast CDN");
    println!("  • Image Processor — on-the-fly resize, format conversion, watermark");
    println!("  • Live Ingest — RTMP -> HLS/DASH transmuxing for live streaming");
    println!("  • Application Acceleration — TCP optimizations, prefetch");
    println!();
    println!("Compute:");
    println!("  • Edge Functions — JS isolates");
    println!("  • Edge SQL — distributed SQLite at the edge");
    println!("  • Edge KV — key-value store");
    println!("  • Edge Storage — S3-compatible object store, regional");
    println!();
    println!("Security:");
    println!("  • Web Application Firewall (WAF) — OWASP rules + custom");
    println!("  • Bot Manager — bot detection + challenge");
    println!("  • DDoS Protection — L3/L4/L7 mitigation");
    println!("  • Network Lists — country / IP block-and-allow");
    println!("  • Edge Firewall — request filtering at the network layer");
    println!();
    println!("Identity:");
    println!("  • Edge Auth — JWT validation, OAuth token introspection at edge");
    println!();
    println!("Observability:");
    println!("  • Real-Time Metrics dashboard");
    println!("  • Real-Time Events log streaming");
    println!("  • Activity History");
    println!("  • Data Stream — push logs to S3 / Datadog / Splunk / Elastic / etc.");
    println!();
    println!("All of these are billed independently; you mix-and-match per app.");
    println!("Pricing is usage-based with monthly commit tiers available.");
}

fn cmd_open() {
    println!("Azion's open-source bet");
    println!();
    println!("Azion has invested in open-source visibility more aggressively than");
    println!("most non-US edge platforms — a developer-relations strategy aimed at");
    println!("expanding mindshare beyond LATAM.");
    println!();
    println!("Notable open-source projects:");
    println!();
    println!("  jsrs.js / Azion Edge Runtime:");
    println!("    Compatibility layer + SDK for running Web Workers-style code");
    println!("    on Azion Edge Functions. Mirrors Service Worker APIs.");
    println!();
    println!("  Vulcan:");
    println!("    Build tooling that compiles Next.js, Nuxt, Astro, Vue, React,");
    println!("    Vite, and Hexo projects for deployment to Azion Edge.");
    println!("    Conceptually similar to Cloudflare Wrangler or Vercel's adapter.");
    println!();
    println!("  Edge starters / templates:");
    println!("    Open-source repos for popular framework + Azion combinations.");
    println!("    'Deploy this Next.js app to Azion Edge' style.");
    println!();
    println!("  Bundlers and CLI tooling:");
    println!("    Azion CLI (cross-platform Go binary) for project scaffold,");
    println!("    deploy, log streaming, account management.");
    println!();
    println!("Strategy:");
    println!("  By open-sourcing the build adapters, Azion lowers the barrier");
    println!("  for developers to try Azion vs Vercel / Cloudflare / Netlify.");
    println!("  Even if Azion doesn't win the developer outright, the OSS work");
    println!("  improves Azion's mindshare among DevRel-attentive developers");
    println!("  in LATAM (where Azion is the natural local choice).");
}

fn cmd_customers() {
    println!("Azion customer base");
    println!();
    println!("Brazilian enterprise (publicly disclosed):");
    println!("  • Bradesco (banking)");
    println!("  • Itau (banking)");
    println!("  • Magazine Luiza (retail)");
    println!("  • Mercado Livre (some flows)");
    println!("  • B3 (Brazilian stock exchange)");
    println!("  • Globo (media conglomerate, partial)");
    println!("  • SBT, Band, RecordTV (broadcasters)");
    println!("  • iFood (food delivery)");
    println!("  • Movile Group");
    println!("  • Brazilian government agencies");
    println!();
    println!("LATAM (broader):");
    println!("  • Falabella (Chile, ecommerce)");
    println!("  • Movistar (Telefonica subsidiaries in multiple countries)");
    println!("  • Major Brazilian banks: Banco do Brasil, Caixa, Santander Brasil");
    println!("  • Brazilian fintechs: Nubank (some flows), C6, PicPay");
    println!();
    println!("Pattern:");
    println!("  Heavy Brazilian banking + retail + fintech concentration.");
    println!("  These are large compliance-regulated workloads that benefit");
    println!("  from in-country edge presence under Brazilian LGPD.");
    println!();
    println!("North American + European customer base:");
    println!("  Smaller; Azion competes against Cloudflare/Fastly there and");
    println!("  doesn't have the same regional advantage. Generally wins when");
    println!("  customers have meaningful LATAM end-user traffic and want a");
    println!("  unified platform across regions.");
}

fn run_azion(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "latam" => cmd_latam(),
        "edge" => cmd_edge(),
        "products" => cmd_products(),
        "open" => cmd_open(),
        "customers" => cmd_customers(),
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
        .unwrap_or_else(|| "azion-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_azion(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/azion-cli"), "azion-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("azion-cli.exe"), "azion-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_azion(&[], "azion-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_azion(&["bogus".into()], "azion-cli"), 2);
    }
}
