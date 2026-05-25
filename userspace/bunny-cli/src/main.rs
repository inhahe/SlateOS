#![deny(clippy::all)]
//! bunny-cli — OurOS Bunny.net / BunnyCDN personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Bunny.net (BunnyCDN) edge platform (personality)");
    println!();
    println!("USAGE: {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about        Dejan Grofelnik Pelzel, Slovenia 2015, bootstrapped");
    println!("    pricing      Famously cheap: $0.01-0.06 per GB tiered");
    println!("    products     CDN, Storage, Stream, Optimizer, Edge Scripting");
    println!("    pull         Pull zones vs storage zones");
    println!("    edge         Bunny Edge Scripting (V8 isolates + JS)");
    println!("    stream       Bunny Stream video platform");
    println!("    fonts        Bunny Fonts — GDPR-friendly Google Fonts replacement");
    println!("    help / version");
}

fn print_version() {
    println!("bunny-cli 0.1.0 — OurOS personality binary");
    println!("BunnyWay d.o.o. — Maribor, Slovenia (Bunny.net)");
}

fn cmd_about() {
    println!("Bunny.net — Content delivery, reinvented.");
    println!();
    println!("Founded:  2015 in Maribor, Slovenia, by Dejan Grofelnik Pelzel");
    println!("          The 'BunnyCDN' brand became 'Bunny.net' around 2020-2021");
    println!("          as the platform expanded beyond pure CDN.");
    println!();
    println!("Bootstrapped:");
    println!("  No outside funding. No VC. Owner-operated and profitable.");
    println!("  Unusual in the CDN industry — every major competitor is either");
    println!("  publicly traded (CFLR, FSLY, AKAM) or private-equity backed.");
    println!();
    println!("Headcount: small (estimated under 100 employees as of 2024)");
    println!();
    println!("Network footprint:");
    println!("  120+ PoPs globally — heavy presence in Europe, growing in APAC");
    println!("  and LATAM. Largely Anycast TCP. Self-built routing optimization");
    println!("  ('Bunny Net Tiered Caching' — origin -> regional -> edge).");
    println!();
    println!("Positioning:");
    println!("  'CDN at SMB-friendly prices with developer-friendly UX.'");
    println!("  Often cited by indie devs and small SaaS on HackerNews as a");
    println!("  3-10x cheaper alternative to Cloudflare/Fastly/CloudFront for");
    println!("  egress-heavy workloads (video, large images, downloads).");
}

fn cmd_pricing() {
    println!("Bunny.net pricing — the indie-favourite cost structure");
    println!();
    println!("CDN egress (per GB), Standard Tier (the default):");
    println!("  Europe + North America:  $0.01 / GB");
    println!("  Asia + Oceania:          $0.03 / GB");
    println!("  South America + Africa:  $0.045 / GB");
    println!();
    println!("CDN egress, High Volume Tier (after 1 PB/month):");
    println!("  Even cheaper — contact sales");
    println!();
    println!("Storage zone (Bunny Storage):");
    println!("  $0.01-0.05 / GB / month (depends on region count)");
    println!("  Replication across multiple regions is built in.");
    println!();
    println!("HTTP requests:");
    println!("  Generally FREE on standard pull/storage zones — only egress is metered.");
    println!();
    println!("Bunny Stream (video):");
    println!("  Storage: $0.005 / GB / month");
    println!("  Streaming: $0.005 / GB delivered");
    println!("  Transcoding: included for first N minutes per month, then per-min");
    println!();
    println!("Minimum monthly bill: $1.00 (yes, one dollar)");
    println!();
    println!("Compare to AWS CloudFront:");
    println!("  CloudFront US/EU egress: ~$0.085 / GB (8.5x Bunny)");
    println!("  CloudFront APAC: ~$0.12 / GB (4x Bunny)");
    println!("  CloudFront South America: ~$0.110 / GB (~2.4x Bunny)");
    println!();
    println!("Trade-off: Bunny's SLAs and enterprise support are leaner.");
    println!("Best fit: cost-conscious teams without enterprise compliance asks.");
}

fn cmd_products() {
    println!("Bunny.net product portfolio");
    println!();
    println!("Bunny CDN:");
    println!("  The original product. Pull zone (origin shield) or storage zone.");
    println!("  Standard Tier (cheap, fewer PoPs) vs High Volume Tier (more PoPs).");
    println!("  Free SSL via Let's Encrypt or BYO cert.");
    println!();
    println!("Bunny Storage:");
    println!("  S3-compatible object storage. Single-region or geo-replicated.");
    println!("  Often combined with Bunny CDN — Storage as origin, CDN as edge.");
    println!();
    println!("Bunny Stream:");
    println!("  Video platform — upload, transcode (HLS + DASH), deliver via CDN.");
    println!("  Built-in player (web component), DRM optional, analytics included.");
    println!();
    println!("Bunny Optimizer:");
    println!("  Image + JS/CSS transformation at the edge. WebP, AVIF, resize,");
    println!("  watermark, minify, prefetch hints.");
    println!();
    println!("Bunny DNS:");
    println!("  Authoritative DNS with geo-steering, latency-based routing,");
    println!("  failover, GeoDNS, and EDNS Client Subnet support.");
    println!();
    println!("Bunny Edge Scripting (Magic Containers, V8 isolates):");
    println!("  JavaScript edge functions a la Cloudflare Workers / Fastly Compute.");
    println!("  Run on the same Anycast network as the CDN.");
    println!();
    println!("Bunny Shield:");
    println!("  DDoS protection + bot mitigation. Less mature than Cloudflare WAF.");
    println!();
    println!("Bunny Fonts:");
    println!("  See 'bunny fonts' subcommand.");
    println!();
    println!("Bunny AI (newest, 2023-2024):");
    println!("  LLM inference at the edge. Embedding generation, image gen, etc.");
}

fn cmd_pull() {
    println!("Bunny.net pull zones vs storage zones");
    println!();
    println!("Pull zone:");
    println!("  Configure with an 'origin URL' — Bunny fetches from your existing");
    println!("  origin server on cache miss, caches at edge, serves to client.");
    println!();
    println!("  Example: origin = https://images.example.com");
    println!("           bunny  = https://cdn.b-cdn.net/products/123.jpg");
    println!("           cdn.b-cdn.net is a Bunny-issued subdomain; configure");
    println!("           your own CNAME like images.example.com -> CNAME -> b-cdn");
    println!();
    println!("Storage zone:");
    println!("  An object store you upload to directly. Files live ON Bunny's");
    println!("  infrastructure — no upstream origin. Replicated across N regions.");
    println!();
    println!("  Combined pattern (the common one):");
    println!("    1. Create a storage zone — your canonical asset bucket");
    println!("    2. Create a pull zone with the storage zone as origin");
    println!("    3. CDN edge serves the cached content fast; storage is the");
    println!("       authoritative source");
    println!();
    println!("Pull-zone-only is good when:");
    println!("  • You already have S3 / your own origin and want CDN in front");
    println!();
    println!("Storage + pull-zone is good when:");
    println!("  • You want everything in one platform (single bill, single ACL)");
    println!("  • You don't already have or want S3");
    println!("  • You want geo-replicated origin without DIY replication");
    println!();
    println!("Storage API:");
    println!("  Bunny ships a REST API + S3-compatible endpoint.");
    println!("  Auth: AccessKey header (per-zone secret).");
}

fn cmd_edge() {
    println!("Bunny Edge Scripting (Magic Containers / Edge Compute)");
    println!();
    println!("What it is:");
    println!("  Run JavaScript at Bunny's edge PoPs — request manipulation,");
    println!("  routing, A/B testing, auth checks, response rewriting.");
    println!("  V8 isolates (similar architecture to Cloudflare Workers).");
    println!();
    println!("Programming model (simplified):");
    println!();
    println!("  export default {{");
    println!("    async fetch(request, env, ctx) {{");
    println!("      if (request.headers.get('x-bot') === 'true') {{");
    println!("        return new Response('blocked', {{ status: 403 }});");
    println!("      }}");
    println!("      const upstream = await fetch(request);");
    println!("      const html = await upstream.text();");
    println!("      const rewritten = html.replaceAll('OLD', 'NEW');");
    println!("      return new Response(rewritten, upstream);");
    println!("    }},");
    println!("  }};");
    println!();
    println!("Magic Containers (newer, 2024):");
    println!("  Run full containerized apps (Node, Python, etc.) at the edge");
    println!("  in regional locations. Stateful, longer-lived than V8 isolates.");
    println!("  Bridge between edge-scripting and full edge-compute workloads.");
    println!();
    println!("Pricing:");
    println!("  Per-request fee, well below Cloudflare Workers' published rate.");
    println!("  No CPU-millisecond billing on the standard tier (as of mid-2024).");
    println!();
    println!("Limits:");
    println!("  CPU time per request, memory, subrequest count — typical V8");
    println!("  isolate constraints. Bunny publishes current limits in docs.");
}

fn cmd_stream() {
    println!("Bunny Stream — video platform");
    println!();
    println!("What it does:");
    println!("  Upload a video -> Bunny transcodes (HLS + DASH adaptive bitrate)");
    println!("  -> serves through Bunny CDN -> embed via a web component player.");
    println!();
    println!("Workflow:");
    println!("  1. Create a video library (essentially an org-scoped bucket)");
    println!("  2. Upload via TUS-protocol resumable upload OR direct PUT");
    println!("  3. Bunny transcodes automatically into 6+ rungs (240p->4K depending");
    println!("     on source resolution)");
    println!("  4. Embed: <iframe src=\"https://iframe.mediadelivery.net/embed/{{lib}}/{{vid}}\">");
    println!("     Or use the web component <bunny-player video-id=\"...\">");
    println!();
    println!("Features:");
    println!("  • Automatic thumbnails + sprite sheets for scrubbing previews");
    println!("  • Closed captions (manual upload or automatic via Whisper API)");
    println!("  • Chapter markers, watermarking, custom branding on player");
    println!("  • Token-authenticated playback URLs (time-limited)");
    println!("  • DRM (Widevine + FairPlay + PlayReady) on enterprise plans");
    println!("  • Heatmap analytics — see where viewers drop off");
    println!();
    println!("Pricing (indicative):");
    println!("  ~$0.005 / GB delivered + ~$0.005 / GB stored / month");
    println!("  No per-minute encoding fee on standard transcodes");
    println!();
    println!("Compare:");
    println!("  Mux video: significantly more expensive but enterprise-grade SLAs");
    println!("  AWS MediaConvert + CloudFront: more flexible, much pricier all-in");
    println!("  Cloudflare Stream: similar concept, comparable price");
}

fn cmd_fonts() {
    println!("Bunny Fonts — GDPR-friendly Google Fonts replacement");
    println!();
    println!("Background:");
    println!("  In Jan 2022, a German court ruled that embedding Google Fonts");
    println!("  via fonts.googleapis.com transferred user IP addresses to Google,");
    println!("  violating GDPR without explicit consent. Massive legal threat");
    println!("  letter campaigns followed across EU SMBs.");
    println!();
    println!("Bunny Fonts (fonts.bunny.net):");
    println!("  A drop-in replacement service hosting the same open-source font");
    println!("  files (Roboto, Inter, Open Sans, etc.) but on Bunny's GDPR-");
    println!("  compliant European infrastructure with no Google connection.");
    println!();
    println!("Migration is literally:");
    println!("  - <link href=\"https://fonts.googleapis.com/css2?family=Inter\" />");
    println!("  + <link href=\"https://fonts.bunny.net/css2?family=Inter\" />");
    println!();
    println!("Same CSS syntax, same font names, same WOFF2 files (the open-source");
    println!("fonts are MIT/SIL licensed — anyone can serve them).");
    println!();
    println!("Cost: free for the standard service, supported by Bunny's main CDN");
    println!("revenues. Effectively a marketing wedge: 'try Bunny for fonts, then");
    println!("for CDN.'");
    println!();
    println!("Privacy:");
    println!("  • No tracking cookies");
    println!("  • No IP logging beyond minimum operational logs");
    println!("  • EU/UK PoPs serve EU/UK requests by default");
    println!();
    println!("Wide adoption among EU agencies and WordPress hosts after the 2022");
    println!("court ruling. One of the cleaner privacy-driven CDN wins.");
}

fn run_bunny(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "pricing" => cmd_pricing(),
        "products" => cmd_products(),
        "pull" => cmd_pull(),
        "edge" => cmd_edge(),
        "stream" => cmd_stream(),
        "fonts" => cmd_fonts(),
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
        .unwrap_or_else(|| "bunny-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_bunny(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bunny-cli"), "bunny-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bunny-cli.exe"), "bunny-cli");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_bunny(&[], "bunny-cli"), 0);
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_bunny(&["bogus".into()], "bunny-cli"), 2);
    }
}
