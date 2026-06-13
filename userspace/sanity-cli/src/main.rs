#![deny(clippy::all)]
//! sanity-cli — Slate OS personality CLI for Sanity.io, the structured-content platform.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Sanity.io — the composable content platform that treats content as data.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Founders, Oslo origins, and the structured-content thesis");
    println!("    studio      Sanity Studio — the open-source, React-based editing environment");
    println!("    api         Content Lake, GROQ query language, and real-time APIs");
    println!("    portable    Portable Text — the rich-text format that ate Markdown");
    println!("    funding     Investors, valuation milestones, and growth trajectory");
    println!("    customers   Nike, Sonos, AT&T, Skims, Loom, Figma, and the rest");
    println!("    pricing     Free tier, Growth, Enterprise — pay for users not for content");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Content as data. Studio as code. Queries as GROQ.");
}

fn print_version() {
    println!("sanity-cli 0.1.0");
    println!("Sanity AS — Oslo, Norway. Founded 2015. Studio v3 released 2022.");
}

fn cmd_about() {
    println!("Sanity.io — structured content for the composable web");
    println!();
    println!("FOUNDED");
    println!("  2015 in Oslo, Norway, by Magnus Hillestad, Even Westvang,");
    println!("  Simen Svale Skogsrud, and Oyvind Rostad. The four had spent");
    println!("  the previous decade at Bengler, a small Oslo studio building");
    println!("  custom CMS solutions for design-conscious clients (museums,");
    println!("  publishers, public broadcasters). Sanity emerged as the");
    println!("  productized form of the internal toolkit they kept rebuilding.");
    println!();
    println!("THESIS");
    println!("  Content is data, not pages. A page is one possible projection");
    println!("  of structured content; a mobile app is another; a smart");
    println!("  speaker is a third. Authoring tools that bake in HTML or");
    println!("  presentation assumptions cannot serve omnichannel publishing.");
    println!("  Treat content as queryable, typed, real-time data — then");
    println!("  any frontend can read it.");
    println!();
    println!("HEADQUARTERS");
    println!("  Oslo, Norway (HQ), San Francisco (US office). Remote-first.");
    println!("  Approximately 200 employees as of 2024.");
}

fn cmd_studio() {
    println!("Sanity Studio — the customizable, code-driven editing environment");
    println!();
    println!("ARCHITECTURE");
    println!("  Studio is a single-page React application that you install");
    println!("  in your own repo, configure with TypeScript, and deploy to");
    println!("  Sanity's hosting (studio.sanity.build) or self-host. Schemas,");
    println!("  custom input components, document actions, plugins, and");
    println!("  workspaces are all defined as code.");
    println!();
    println!("STUDIO V3 (Dec 2022)");
    println!("  Complete rewrite. Drop-in to any Vite/Next.js project");
    println!("  (npx sanity init). Workspaces, presentation tool, embeds.");
    println!("  Studio V2 sunset: code-frozen, supported via patches only.");
    println!();
    println!("KEY FEATURES");
    println!("  - Schemas as TypeScript: defineType, defineField, defineArrayMember");
    println!("  - Real-time collaboration: see other editors' cursors live");
    println!("  - Field-level validation and conditional fields");
    println!("  - Custom input components (any React component is fair game)");
    println!("  - Document actions: publish, duplicate, custom workflows");
    println!("  - Plugins: image hotspots, color pickers, table, code input");
    println!("  - Presentation tool: visual editing with cross-document links");
    println!("  - Live preview via Visual Editing overlays on your front-end");
}

fn cmd_api() {
    println!("Content Lake — the queryable, real-time content backend");
    println!();
    println!("CONTENT LAKE");
    println!("  Sanity's hosted JSON document store. Documents are immutable");
    println!("  drafts + published pairs; every change is a transaction with");
    println!("  history. Region: GCP us-east1, eu-west1, ap-south1.");
    println!();
    println!("GROQ — Graph-Relational Object Queries");
    println!("  Sanity's open-source query language (graphql competitor).");
    println!("  *[_type == 'movie' && releaseYear > 2020] | order(title asc)");
    println!("    {{ title, 'director': director->name, castMembers[]->name }}");
    println!("  Filters, projections, joins (->), slicing [0...10], grouping.");
    println!("  Available outside Sanity as the groq npm package.");
    println!();
    println!("APIS");
    println!("  - Query API:  https://<projectId>.api.sanity.io/v2024-05-01/data/query/<dataset>");
    println!("  - Mutate API: POST /data/mutate/<dataset>  (create, patch, delete txns)");
    println!("  - Listen API: server-sent events for live updates on a query");
    println!("  - Asset API:  /assets/images/<dataset>  for image upload + transforms");
    println!("  - Doc API:    /data/doc/<dataset>/<id>   one-shot document fetch");
    println!("  - History API: /data/history  full revision log of any document");
    println!();
    println!("CDN");
    println!("  apicdn.sanity.io — cached, ~150 PoPs, ~10ms TTFB globally.");
    println!("  api.sanity.io — uncached fresh reads, slightly higher latency.");
}

fn cmd_portable() {
    println!("Portable Text — the structured rich-text format");
    println!();
    println!("WHAT IT IS");
    println!("  An open JSON specification for rich text. Instead of HTML or");
    println!("  Markdown blobs, content is an array of typed blocks. Each");
    println!("  block has children (spans of text with marks) plus optional");
    println!("  embedded objects (images, callouts, code blocks, anything).");
    println!();
    println!("EXAMPLE");
    println!("  [");
    println!("    {{ \"_type\": \"block\", \"style\": \"h2\", \"children\":");
    println!("      [{{ \"_type\": \"span\", \"text\": \"Hello\" }}] }},");
    println!("    {{ \"_type\": \"image\", \"asset\": {{ \"_ref\": \"image-abc\" }} }},");
    println!("    {{ \"_type\": \"block\", \"children\":");
    println!("      [{{ \"text\": \"See \", \"marks\": [] }},");
    println!("       {{ \"text\": \"docs\", \"marks\": [\"link1\"] }}],");
    println!("      \"markDefs\":");
    println!("      [{{ \"_key\": \"link1\", \"_type\": \"link\",");
    println!("         \"href\": \"https://example.com\" }}] }}");
    println!("  ]");
    println!();
    println!("WHY");
    println!("  Renderable as HTML, React, Vue, Svelte, native iOS/Android,");
    println!("  voice assistants, or anything else. No HTML soup to sanitize.");
    println!("  Editor can embed any schema type inline. The format is");
    println!("  portable in the deepest sense: same document, infinite outputs.");
    println!();
    println!("SPEC: portabletext.org  (open, multi-vendor adoption growing).");
}

fn cmd_funding() {
    println!("Sanity — funding history");
    println!();
    println!("  2015  Founded, bootstrapped from Bengler revenue.");
    println!("  2017  Seed: undisclosed angels, focus on building Studio.");
    println!("  2019  Series A: $9.3M, Threshold Ventures lead.");
    println!("  2021  Series B: $39M, ICONIQ Capital lead. Reported");
    println!("        valuation in the $250-400M range during ZIRP-era pricing.");
    println!("  2024  Series C: $35M, Wisdom Ventures lead, with ICONIQ and");
    println!("        Threshold participating. More disciplined valuation,");
    println!("        focus on path to operating profitability.");
    println!();
    println!("Total disclosed: ~$83M across known rounds.");
    println!("Notable: Sanity has avoided ZIRP-era extravagance — headcount");
    println!("stayed under 250, no acquisitions, no rebrand churn.");
}

fn cmd_customers() {
    println!("Selected Sanity customers");
    println!();
    println!("  Nike            — global ecommerce content for Nike.com");
    println!("  Sonos           — product marketing + support content");
    println!("  Skims           — Kim Kardashian's shapewear brand");
    println!("  AT&T            — selected campaign sites");
    println!("  Loom            — marketing site (pre-Atlassian acquisition)");
    println!("  Figma           — figma.com marketing surfaces");
    println!("  Linear          — linear.app website + changelog");
    println!("  Cloudflare      — cloudflare.com (partial)");
    println!("  Burger King     — campaign microsites and franchise portals");
    println!("  Puma            — global storefront content");
    println!("  Riot Games      — League of Legends esports portals");
    println!("  Invision        — marketing and design-system docs");
    println!();
    println!("Sweet spot: design-led brands and modern SaaS who already");
    println!("have a Next.js/Remix/Astro frontend and need a headless CMS");
    println!("their content team can actually use.");
}

fn cmd_pricing() {
    println!("Sanity pricing (as of 2024)");
    println!();
    println!("  Free        $0/mo");
    println!("              3 users, 2 datasets, 10K documents, 100K API CDN req,");
    println!("              hosted Studio, community support.");
    println!();
    println!("  Growth      $15/user/mo");
    println!("              Unlimited datasets, 10K+ documents, 1M API CDN req,");
    println!("              roles, history (30 days), SSO add-on.");
    println!();
    println!("  Enterprise  Custom");
    println!("              SSO/SAML, audit logs, custom data residency,");
    println!("              SLA, premium support, content lake import/export tools.");
    println!();
    println!("Pricing dimension: users (editors), not pageviews. Free for");
    println!("read-only viewers. Bandwidth/API requests metered separately");
    println!("but generous on the free tier.");
}

fn run_sanity(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "studio" => { cmd_studio(); 0 }
        "api" => { cmd_api(); 0 }
        "portable" => { cmd_portable(); 0 }
        "funding" => { cmd_funding(); 0 }
        "customers" => { cmd_customers(); 0 }
        "pricing" => { cmd_pricing(); 0 }
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'. Try '{prog} help'.");
            2
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "sanity".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_sanity(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/sanity"), "sanity");
        assert_eq!(basename("C:\\Tools\\sanity.exe"), "sanity.exe");
        assert_eq!(basename("sanity"), "sanity");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("sanity.exe"), "sanity");
        assert_eq!(strip_ext("sanity"), "sanity");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_sanity(&["help".to_string()], "sanity"), 0);
        let _ = run_sanity(&[], "sanity");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_sanity(&["nope".to_string()], "sanity"), 2);
    }
}
