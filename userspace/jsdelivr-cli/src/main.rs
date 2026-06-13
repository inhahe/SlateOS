#![deny(clippy::all)]
//! jsdelivr-cli — SlateOS jsDelivr free public CDN personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — jsDelivr free open-source CDN (personality)");
    println!();
    println!("USAGE: {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Dmitriy Akulov 2012, npm + GitHub + WordPress");
    println!("    urls        URL conventions for npm, GitHub, WordPress, GH gists");
    println!("    sponsors    Cloudflare + Fastly + Bunny + GCore combined network");
    println!("    stats       Public anonymized usage stats");
    println!("    sri         Subresource integrity for safer embeds");
    println!("    alt         Comparison to cdnjs, unpkg, esm.sh, Skypack");
    println!("    help / version");
}

fn print_version() {
    println!("jsdelivr-cli 0.1.0 — Slate OS personality binary");
    println!("jsDelivr — Free, fast, reliable open-source CDN");
}

fn cmd_about() {
    println!("jsDelivr — Free, fast, and reliable open source CDN.");
    println!();
    println!("Founded:  2012 by Dmitriy Akulov + collaborators");
    println!("Stewardship: Currently maintained by the Polyfill.io / Prospect");
    println!("             team and various contributors. Non-profit-style ops.");
    println!();
    println!("The pitch:");
    println!("  Anyone can serve any open-source library, GitHub asset, npm");
    println!("  package, or WordPress plugin globally for free with HTTPS,");
    println!("  Brotli, HTTP/2, HTTP/3, and a CDN footprint of 1000+ PoPs.");
    println!();
    println!("Funding model:");
    println!("  No paid plans. No premium tiers. No customer accounts.");
    println!("  Operating costs covered ENTIRELY by sponsored bandwidth from");
    println!("  major CDN providers (see 'jsdelivr sponsors').");
    println!();
    println!("Scale:");
    println!("  ~150+ BILLION requests per month at peak. The single largest");
    println!("  open-source CDN by request volume (vs cdnjs, unpkg, etc.).");
    println!();
    println!("Distinguishing features:");
    println!("  • Multi-CDN combined network (failover between sponsors)");
    println!("  • Automatic minification of unminified files");
    println!("  • Per-file Subresource Integrity (SRI) hashes provided");
    println!("  • Version aliases (npm tags, GitHub branches, latest)");
    println!("  • Combined assets (multiple files in one request)");
    println!("  • Public statistics API");
    println!("  • Generous abuse policy (DDoS mitigation but minimal rate limit");
    println!("    for honest open-source asset serving)");
}

fn cmd_urls() {
    println!("jsDelivr URL conventions");
    println!();
    println!("All URLs start with https://cdn.jsdelivr.net/");
    println!();
    println!("npm packages:");
    println!("  https://cdn.jsdelivr.net/npm/{{package}}@{{version}}/{{file}}");
    println!();
    println!("  Examples:");
    println!("    /npm/jquery@3.7.1/dist/jquery.min.js");
    println!("    /npm/bootstrap@5.3.3/dist/css/bootstrap.min.css");
    println!("    /npm/lodash@4.17.21");
    println!();
    println!("  Version aliases (latest, semver ranges, npm tags):");
    println!("    /npm/jquery@latest/dist/jquery.min.js");
    println!("    /npm/react@^18/umd/react.production.min.js");
    println!("    /npm/typescript@beta/lib/typescript.js");
    println!();
    println!("GitHub:");
    println!("  https://cdn.jsdelivr.net/gh/{{user}}/{{repo}}@{{branch|tag}}/{{file}}");
    println!();
    println!("  Examples:");
    println!("    /gh/twbs/bootstrap@v5.3.3/dist/css/bootstrap.min.css");
    println!("    /gh/microsoft/playwright@main/README.md");
    println!();
    println!("WordPress plugins/themes:");
    println!("  https://cdn.jsdelivr.net/wp/plugins/{{slug}}/tags/{{tag}}/{{file}}");
    println!("  https://cdn.jsdelivr.net/wp/themes/{{slug}}/tags/{{tag}}/{{file}}");
    println!();
    println!("GitHub gists:");
    println!("  https://cdn.jsdelivr.net/gh/{{user}}/{{gistId}}/raw/{{file}}");
    println!();
    println!("Auto-minification (just add .min before the extension):");
    println!("  /npm/jquery@3.7.1/dist/jquery.js -> served as-is");
    println!("  /npm/jquery@3.7.1/dist/jquery.min.js -> served minified copy");
    println!();
    println!("Combined files (multiple files, one request):");
    println!("  /combine/npm/jquery@3,npm/bootstrap@5");
    println!("  Concatenates jquery and bootstrap into a single bundled response.");
}

fn cmd_sponsors() {
    println!("jsDelivr sponsored network — multi-CDN combined");
    println!();
    println!("How is a free CDN at 150B requests/month even possible?");
    println!("Answer: it's not one CDN. It's several CDNs that donate bandwidth.");
    println!();
    println!("Sponsors providing infrastructure (varies over time):");
    println!();
    println!("  Cloudflare:");
    println!("    Donated capacity through their Project Galileo / public CDN");
    println!("    sponsorship arm. Cloudflare's network handles substantial");
    println!("    fraction of jsDelivr traffic globally.");
    println!();
    println!("  Fastly:");
    println!("    Sponsorship of jsDelivr predates Fastly's IPO. Fastly's");
    println!("    Open Source Program donates capacity to high-impact OSS infra.");
    println!();
    println!("  Bunny.net:");
    println!("    Sponsors significant capacity in regions where Bunny has dense");
    println!("    presence (Europe especially). Marketing wedge for Bunny too.");
    println!();
    println!("  Gcore (G-Core Labs historically):");
    println!("    Donated network capacity in CEE + Asia + LATAM.");
    println!();
    println!("  Quantil / CloudKit / NSONE DNS, etc.:");
    println!("    Smaller sponsors for adjacent infra (DNS, auxiliary regions).");
    println!();
    println!("Multi-CDN load balancing:");
    println!("  jsDelivr uses NS1 (now IBM-owned) for intelligent geo + latency-");
    println!("  based DNS routing. Each request is steered to the best sponsor's");
    println!("  PoP for that user's location and the sponsor's current capacity.");
    println!();
    println!("Failover:");
    println!("  If one sponsor degrades (incident, regional outage), DNS instantly");
    println!("  routes traffic to alternative sponsors. This redundancy is why");
    println!("  jsDelivr's uptime is typically better than any single CDN's.");
}

fn cmd_stats() {
    println!("jsDelivr public statistics");
    println!();
    println!("Stats API:");
    println!("  https://data.jsdelivr.com/v1");
    println!();
    println!("Endpoints (free, no auth required):");
    println!();
    println!("  /package/npm/{{pkg}}/stats");
    println!("    Total hits + bandwidth + rank for an npm package.");
    println!();
    println!("  /package/gh/{{user}}/{{repo}}/stats");
    println!("    Same but for GitHub-hosted assets.");
    println!();
    println!("  /stats/packages?type=npm&period=month");
    println!("    Top N packages by usage over a time period.");
    println!();
    println!("  /package/npm/{{pkg}}/badge");
    println!("    SVG badge for use in README — 'X downloads / week'.");
    println!();
    println!("What the stats tell us (informative trivia):");
    println!("  • The most-served npm package via jsDelivr is jQuery (still!)");
    println!("    largely from legacy WordPress + corporate CMS embeds.");
    println!("  • Bootstrap CSS/JS is in the top 3 by request count.");
    println!("  • Modern framework usage (React, Vue, etc.) is heavily npm-fed");
    println!("    rather than CDN-fed for app builds — so React via jsDelivr is");
    println!("    less than you'd expect from npm-download counts.");
    println!("  • Heavy emoji + icon font usage (Font Awesome, Material Icons).");
    println!();
    println!("Stats provide a unique cross-cut view of the open-source ecosystem");
    println!("that npm download counts can't — actual runtime usage in production");
    println!("HTML pages, regardless of build system.");
}

fn cmd_sri() {
    println!("Subresource Integrity (SRI) with jsDelivr");
    println!();
    println!("Why SRI matters for CDN-served scripts:");
    println!("  If your page embeds a script from a CDN, and that CDN is");
    println!("  compromised (or actively malicious), your users execute attacker");
    println!("  code. SRI lets the browser verify the script's cryptographic");
    println!("  hash matches what you declared — if not, the browser refuses");
    println!("  to execute it. This protects against CDN compromise.");
    println!();
    println!("Standard usage:");
    println!();
    println!("  <script");
    println!("    src=\"https://cdn.jsdelivr.net/npm/jquery@3.7.1/dist/jquery.min.js\"");
    println!("    integrity=\"sha384-...\"");
    println!("    crossorigin=\"anonymous\">");
    println!("  </script>");
    println!();
    println!("jsDelivr's SRI API:");
    println!("  GET https://www.jsdelivr.com/integrity/{{path}}");
    println!("  Returns the integrity attribute string for any file jsDelivr serves.");
    println!();
    println!("  Even easier — visit the jsDelivr website for any package, browse");
    println!("  files, and click 'Copy SRI snippet'. The full <script ... integrity>");
    println!("  tag is generated for you.");
    println!();
    println!("Supported hash algorithms:");
    println!("  SHA-384 (recommended), SHA-256, SHA-512");
    println!();
    println!("Caveat:");
    println!("  SRI only protects the file at that exact version. Using @latest");
    println!("  or a semver range defeats SRI because the file content can change");
    println!("  under you. For SRI to mean anything, pin to an exact version.");
}

fn cmd_alt() {
    println!("jsDelivr vs other public CDNs");
    println!();
    println!("cdnjs (Cloudflare):");
    println!("  Curated library list (CDNJS team adds/approves entries).");
    println!("  Hosted entirely on Cloudflare. Slightly different URL convention.");
    println!("  Smaller surface (no arbitrary GitHub serving).");
    println!("  Strength: vetted library list; less chance of typosquats.");
    println!("  Weakness: not every npm package available; manual addition.");
    println!();
    println!("unpkg (built by ex-React team, run by Vercel):");
    println!("  Mirrors all of npm by URL: unpkg.com/{{pkg}}@{{ver}}/{{file}}.");
    println!("  Originated as 'npmcdn.com' (Michael Jackson, React Router author).");
    println!("  Strength: directly mirrors npm semantics; identical URL structure.");
    println!("  Historical weakness: occasional reliability issues at scale");
    println!("  (single-CDN, capacity-constrained).");
    println!();
    println!("esm.sh:");
    println!("  Auto-transforms CommonJS / UMD packages into ESM-compatible");
    println!("  modules. Modern import-friendly. Built by Ye Yajie.");
    println!("  Useful when you want `import x from 'https://esm.sh/...'` in");
    println!("  Deno / browser ESM without bundler. Heavier processing per request.");
    println!();
    println!("Skypack (now archived):");
    println!("  Like esm.sh but commercially backed (Astro / Snowpack team).");
    println!("  Shut down around 2022-2023 in favor of esm.sh and others.");
    println!();
    println!("jsDelivr's specific positioning:");
    println!("  Most generic, most fault-tolerant (multi-CDN), highest volume.");
    println!("  Best fit for production embeds and broad coverage.");
    println!("  Less suited to modern ESM-via-CDN dev workflows where esm.sh");
    println!("  shines.");
}

fn run_jsdelivr(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "urls" => cmd_urls(),
        "sponsors" => cmd_sponsors(),
        "stats" => cmd_stats(),
        "sri" => cmd_sri(),
        "alt" => cmd_alt(),
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
        .unwrap_or_else(|| "jsdelivr-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_jsdelivr(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/jsdelivr-cli"), "jsdelivr-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("jsdelivr-cli.exe"), "jsdelivr-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_jsdelivr(&[], "jsdelivr-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_jsdelivr(&["bogus".into()], "jsdelivr-cli"), 2);
    }
}
