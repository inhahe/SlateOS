#![deny(clippy::all)]
//! contentful-cli — OurOS Contentful headless CMS personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Contentful headless CMS (personality)");
    println!();
    println!("USAGE: {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about        Sascha Konietzke + Paolo Negri 2013 Berlin");
    println!("    api          Content Delivery / Management / Preview APIs");
    println!("    spaces       Spaces, environments, content types, entries");
    println!("    studio       Compose, Launch, Studio Experiences");
    println!("    funding      Tiger Global $175M at $3B (Jul 2021) and beyond");
    println!("    customers    Notable enterprise customers");
    println!("    sdks         Official SDKs and integrations");
    println!("    help / version");
}

fn print_version() {
    println!("contentful-cli 0.1.0 — OurOS personality binary");
    println!("Contentful GmbH — Berlin, Germany");
}

fn cmd_about() {
    println!("Contentful — The composable content platform.");
    println!();
    println!("Founded:  2013 in Berlin, Germany");
    println!("Founders: Sascha Konietzke + Paolo Negri");
    println!("          (Both ex-XING engineers, attended Berlin's TechCrunch");
    println!("          ecosystem in the early 2010s.)");
    println!();
    println!("Funding journey:");
    println!("  2014: USD 2.5M seed (Point Nine Capital, Project A)");
    println!("  2014: USD 6M Series A (Balderton Capital, Benchmark)");
    println!("  2017: USD 28M Series C (General Catalyst)");
    println!("  2019: USD 33.5M Series D (Sapphire Ventures)");
    println!("  Jun 2020: USD 80M Series E (Sapphire, OMERS Ventures, et al.)");
    println!("  Jul 2021: USD 175M Series F at USD 3B valuation");
    println!("            Lead: Tiger Global. Co: Sapphire, GA, Balderton.");
    println!();
    println!("Positioning:");
    println!("  THE category-defining headless CMS for enterprise. When the");
    println!("  term 'headless CMS' is used in B2B sales contexts, Contentful");
    println!("  is the default reference point. (Sanity is the closest peer at");
    println!("  enterprise scale; Strapi is the OSS challenger.)");
    println!();
    println!("Headcount: ~700-800 employees as of 2024.");
    println!();
    println!("Acquisitions:");
    println!("  Compose.ly (Jun 2022) — content marketplace integration.");
    println!("  Ninetailed (Apr 2024) — personalization + experimentation.");
    println!("  Stackbit (Sep 2023) — visual editing + design integration.");
}

fn cmd_api() {
    println!("Contentful APIs — three tiers, different purposes");
    println!();
    println!("Content Delivery API (CDA):");
    println!("  Base URL: https://cdn.contentful.com");
    println!("  Read-only, edge-cached on Fastly.");
    println!("  Returns published content.");
    println!("  Authenticated by CDA token (read-only, scope to space + env).");
    println!("  Endpoints:");
    println!("    GET /spaces/{{space}}/environments/{{env}}/entries");
    println!("    GET /spaces/{{space}}/environments/{{env}}/entries/{{id}}");
    println!("    GET /spaces/{{space}}/environments/{{env}}/assets");
    println!();
    println!("Content Preview API (CPA):");
    println!("  Base URL: https://preview.contentful.com");
    println!("  Same endpoints as CDA, but returns DRAFT content (unpublished).");
    println!("  For staging/preview builds where editors want to see");
    println!("  unpublished changes in the live frontend.");
    println!();
    println!("Content Management API (CMA):");
    println!("  Base URL: https://api.contentful.com");
    println!("  Read AND write. Schema management. Workflow operations.");
    println!("  Authenticated by personal access token or OAuth app token.");
    println!("  Used by the Contentful web app, CLI, and integrations.");
    println!();
    println!("GraphQL Content API:");
    println!("  Base URL: https://graphql.contentful.com/content/v1/spaces/{{space}}");
    println!("  GraphQL layer over CDA. Same auth model.");
    println!("  Schema auto-generated from your content types.");
    println!();
    println!("Image Asset API:");
    println!("  URL: https://images.ctfassets.net/{{space}}/{{asset}}/{{file}}");
    println!("  Query-param transforms: w / h / fit / fm / q / r / f / focus");
    println!("  Images.ctfassets.net is backed by ImgIX in the background.");
    println!();
    println!("Rate limits:");
    println!("  CDA: 78 req/sec per token");
    println!("  CMA: 7 req/sec per token + 10/sec per organization on writes");
}

fn cmd_spaces() {
    println!("Contentful data model — Spaces / Environments / Content Types / Entries");
    println!();
    println!("Space:");
    println!("  Top-level isolation. Each Space has its own content schema,");
    println!("  its own API tokens, its own user permissions.");
    println!("  Typical pattern: one Space per brand, product, or business unit.");
    println!();
    println!("Environment:");
    println!("  A versioned snapshot of a Space's schema + content.");
    println!("  Default environments: 'master' (production).");
    println!("  Create branches: 'staging', 'preview', 'feature-launch-x'.");
    println!("  Test schema changes in a branch, merge to master when ready.");
    println!();
    println!("Content Type:");
    println!("  A schema definition. Like a database table or class.");
    println!("  Has named fields with types: Text, Symbol, Number, Date,");
    println!("  Boolean, Location, Reference (link to another entry),");
    println!("  Asset (link to image/file), Array (of any of the above),");
    println!("  Rich Text (structured document, the Contentful Rich Text format).");
    println!();
    println!("Entry:");
    println!("  An instance of a Content Type. The actual content piece.");
    println!("  E.g. content type 'BlogPost' with entry 'My First Post'.");
    println!("  Has draft + published states. Versioned (rolls back possible).");
    println!();
    println!("Asset:");
    println!("  A file (image, video, PDF, etc.) uploaded to Contentful.");
    println!("  Referenced from entries via the Asset field type.");
    println!();
    println!("Locale:");
    println!("  Per-Space localization. Each Entry/Asset can have field");
    println!("  values per configured locale (en-US, de-DE, fr-FR, etc.).");
    println!("  Fallback locale chain configurable.");
    println!();
    println!("Tags + metadata:");
    println!("  Soft taxonomies attachable to Entries and Assets.");
    println!("  Used for filtering, organization, and access control.");
}

fn cmd_studio() {
    println!("Contentful Studio — the editor experience");
    println!();
    println!("Compose:");
    println!("  Marketer-facing page-builder UI built on top of structured");
    println!("  content. Define page templates (Hero, FeatureGrid, CTA, etc.),");
    println!("  let marketers compose pages by picking and reordering modules.");
    println!("  Cuts out the 'every page needs developer help' problem of");
    println!("  pure-developer headless CMS workflows.");
    println!();
    println!("Launch:");
    println!("  Release-management UI. Group related content changes into");
    println!("  a 'release', preview them together, schedule them to publish");
    println!("  atomically at a specific time. The cure to the");
    println!("  'editor publishes 30 entries one by one and breaks the site'");
    println!("  problem.");
    println!();
    println!("Studio Experiences (post-Ninetailed acquisition):");
    println!("  Personalization + A/B testing within Contentful UI.");
    println!("  Editors create 'experiences' — content variants based on");
    println!("  audience signals (geo, user properties, etc.).");
    println!("  Runtime evaluation via the Ninetailed SDK at the frontend.");
    println!();
    println!("Studio Editor (post-Stackbit acquisition):");
    println!("  Visual editing experience that overlays the live website,");
    println!("  letting editors click on a heading on the live preview, type,");
    println!("  and have it auto-save back to Contentful as structured content.");
    println!("  WordPress-style WYSIWYG ergonomics with headless-CMS data model.");
    println!();
    println!("Tasks:");
    println!("  Lightweight workflow — assign entries to teammates with comments");
    println!("  + due dates. Not a full editorial workflow system but the");
    println!("  common-case 'please write copy for this' assignment loop.");
}

fn cmd_funding() {
    println!("Contentful funding history and valuation context");
    println!();
    println!("Detailed:");
    println!("  2013: founded");
    println!("  2014: USD 2.5M seed");
    println!("  2014: USD 6M Series A");
    println!("  2017: USD 28M Series C");
    println!("  2019: USD 33.5M Series D at ~USD 350M");
    println!("  Jun 2020: USD 80M Series E at ~USD 600M");
    println!("  Jul 2021: USD 175M Series F at USD 3B valuation");
    println!();
    println!("Series F context:");
    println!("  Peak ZIRP-era SaaS valuation environment. Tiger Global was");
    println!("  writing checks at unprecedented pace and at unprecedented prices.");
    println!("  Many of Tiger's 2021 vintage marks were subsequently written");
    println!("  down. The headless-CMS category specifically had strong tailwinds:");
    println!("  Jamstack hype peaked, headless commerce was the hot pattern,");
    println!("  Vercel was the developer-platform du jour.");
    println!();
    println!("Post-2022 environment:");
    println!("  Contentful did not publicly mark down its valuation but did");
    println!("  conduct layoffs in 2023 (~10-15% reported across two rounds).");
    println!("  Public peer comps (Squarespace, Wix, HubSpot CMS-adjacent) all");
    println!("  saw 60-80% peak-to-trough multiple compression. Contentful's");
    println!("  USD 3B valuation is widely understood to need a substantial");
    println!("  ARR-to-revenue catch-up before being justified at IPO/exit.");
    println!();
    println!("Revenue scale:");
    println!("  Estimated USD 200-300M ARR (analyst estimates, not disclosed).");
    println!("  ~4000+ paying customer accounts across all tiers.");
    println!();
    println!("IPO trajectory:");
    println!("  Long-rumored but no concrete filing. Most likely path: continued");
    println!("  growth at moderate pace, IPO when market re-opens for SaaS");
    println!("  (2025-2027 window depending on market conditions).");
}

fn cmd_customers() {
    println!("Contentful customer base (publicly disclosed)");
    println!();
    println!("Enterprise + Fortune 500:");
    println!("  • Bang & Olufsen");
    println!("  • Boots (UK retail pharmacy)");
    println!("  • Cisco");
    println!("  • Daimler / Mercedes-Benz");
    println!("  • Discovery (now Warner Bros Discovery)");
    println!("  • The Economist");
    println!("  • Equinox (luxury gyms)");
    println!("  • Heineken");
    println!("  • Lufthansa");
    println!("  • Mailchimp (some flows)");
    println!("  • NHS (UK National Health Service)");
    println!("  • Nike");
    println!("  • Notion (legal docs site)");
    println!("  • Spotify");
    println!("  • Telus");
    println!("  • Vodafone");
    println!();
    println!("Tech / startup adoption (publicly known):");
    println!("  • Algolia (own marketing site historical)");
    println!("  • Atlassian (developer docs adjacent)");
    println!("  • Bloomberg (some flows)");
    println!("  • Eventbrite");
    println!("  • InVision");
    println!("  • Stripe (some marketing properties)");
    println!();
    println!("Pattern:");
    println!("  Heavy in:");
    println!("    - Multinational consumer brands (multi-locale, multi-region")  ;
    println!("      content needs match Contentful sweet spot)");
    println!("    - Enterprise B2B (sales sites + developer docs)");
    println!("    - Direct-to-consumer ecommerce");
    println!("    - Publishing + media (some)");
    println!();
    println!("Less common:");
    println!("  • Pure-developer-led smaller startups (more likely to use");
    println!("    Sanity, Strapi, or Markdown-in-Git than Contentful)");
    println!("  • Open-source projects (Contentful is firmly closed-source SaaS)");
}

fn cmd_sdks() {
    println!("Contentful official SDKs and integrations");
    println!();
    println!("Client SDKs (Content Delivery / Preview / Management APIs):");
    println!("  • JavaScript / TypeScript (contentful + contentful-management)");
    println!("  • Python (contentful)");
    println!("  • Ruby (contentful)");
    println!("  • PHP (contentful/contentful)");
    println!("  • Java (contentful.java)");
    println!("  • .NET / C# (contentful.net)");
    println!("  • Swift (contentful.swift)");
    println!("  • Android / Java (contentful.java)");
    println!();
    println!("Framework integrations:");
    println!("  • Next.js — official starter + ISR-friendly fetch helpers");
    println!("  • Nuxt — Vue.js / Nuxt 3 module");
    println!("  • Astro — official starter + content collection adapter");
    println!("  • Gatsby — gatsby-source-contentful plugin");
    println!("  • Remix — official starter");
    println!("  • SvelteKit — community + official samples");
    println!("  • Hugo + Jekyll — JSON export pattern");
    println!();
    println!("Marketplace apps (Contentful App Framework):");
    println!("  • DAM integrations: Bynder, Cloudinary, ImgIX, Wistia, Mux");
    println!("  • Translation: Smartling, Lokalise, DeepL, Crowdin");
    println!("  • Personalization: Optimizely (post-acq), Dynamic Yield, Ninetailed");
    println!("  • Commerce: Shopify, BigCommerce, Commercetools");
    println!("  • Workflow: Slack, Teams, Asana, Jira");
    println!("  • SEO: SEMrush, Yoast");
    println!();
    println!("CLI:");
    println!("  npm install -g contentful-cli");
    println!("  contentful login");
    println!("  contentful space create / list / use");
    println!("  contentful content-type list / get / migration apply");
    println!("  contentful merge — content type migration tool");
    println!();
    println!("Webhooks:");
    println!("  Configurable per space. Fire on entry/asset create/update/publish/");
    println!("  unpublish/delete. Filter by content type + environment.");
    println!("  Standard pattern: webhook -> CI build trigger -> static site rebuild.");
}

fn run_contentful(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "api" => cmd_api(),
        "spaces" => cmd_spaces(),
        "studio" => cmd_studio(),
        "funding" => cmd_funding(),
        "customers" => cmd_customers(),
        "sdks" => cmd_sdks(),
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
        .unwrap_or_else(|| "contentful-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_contentful(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/contentful-cli"), "contentful-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("contentful-cli.exe"), "contentful-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_contentful(&[], "contentful-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_contentful(&["bogus".into()], "contentful-cli"), 2);
    }
}
