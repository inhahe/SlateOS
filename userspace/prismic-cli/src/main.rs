#![deny(clippy::all)]
//! prismic-cli — SlateOS personality CLI for Prismic, the slice-based headless CMS.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Prismic — the headless website builder with Slice Machine.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Founders, Paris origins, Play Framework heritage");
    println!("    slices      Slice Machine and the slice-based page-builder model");
    println!("    api         Content APIs, ref tokens, Prismic GraphQL");
    println!("    funding     The Sadek Drobi / Guillaume Bort founding bet");
    println!("    customers   Google, Deliveroo, Hinge, Dropbox, others");
    println!("    pricing     Free, Starter, Small, Medium, Platinum, Enterprise");
    println!("    sdks        Official Next.js / Nuxt / SvelteKit / vanilla SDKs");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Slices, releases, and the headless CMS for marketers who ship.");
}

fn print_version() {
    println!("prismic-cli 0.1.0");
    println!("Prismic.io SAS — Paris, France. Founded 2013.");
}

fn cmd_about() {
    println!("Prismic — the slice-based headless CMS");
    println!();
    println!("FOUNDED");
    println!("  2013 in Paris, France, by Sadek Drobi (ex-Zenexity / Play");
    println!("  Framework lead, Coursera engineering) and Guillaume Bort");
    println!("  (creator of the Play Framework). Both founders had spent");
    println!("  years thinking about content + Scala + functional approaches");
    println!("  to web infrastructure at Zenexity (which became Zengularity).");
    println!();
    println!("THE THESIS");
    println!("  Most content systems force a choice between rigid page");
    println!("  templates (marketers love them, developers hate them) and");
    println!("  pure structured-content APIs (developers love them, marketers");
    println!("  can't compose new layouts). Prismic's answer is the Slice:");
    println!("  a developer-defined component with strongly-typed fields, that");
    println!("  marketers can stack into pages via a Slice Zone — the best of");
    println!("  both worlds.");
    println!();
    println!("HEADQUARTERS");
    println!("  Paris (HQ), with remote engineers across France, Eastern");
    println!("  Europe, and Latin America. ~80 employees. Privately held;");
    println!("  remains independent and capital-efficient.");
}

fn cmd_slices() {
    println!("Slice Machine and slice-based modeling");
    println!();
    println!("WHAT IS A SLICE?");
    println!("  A Slice is a reusable, developer-defined component template");
    println!("  with strongly-typed Prismic fields. Examples: 'CallToAction'");
    println!("  with title/description/button fields; 'ImageGallery' with");
    println!("  variants for 2-up / 3-up / carousel; 'CustomerLogos' with a");
    println!("  repeating image group. Each slice has multiple variations.");
    println!();
    println!("THE SLICE ZONE");
    println!("  A field on a Page (or any custom type) that holds an ordered");
    println!("  list of slices. Marketers add slices, reorder them, switch");
    println!("  variations, fill in fields — and the page is composed.");
    println!();
    println!("SLICE MACHINE");
    println!("  A locally-running dev tool (npx slicemachine) that pairs your");
    println!("  Prismic schema with your codebase. You design slices in the");
    println!("  Slice Machine UI; it generates the matching component file");
    println!("  in src/slices/<Name>/index.tsx with TypeScript types for the");
    println!("  slice's fields. Mock data is auto-generated for Storybook.");
    println!();
    println!("WHY IT WORKS");
    println!("  Developers ship strongly-typed components; marketers compose");
    println!("  without filing dev tickets for every layout tweak; the");
    println!("  schema and the code stay in sync because Slice Machine");
    println!("  owns both sides of the boundary.");
}

fn cmd_api() {
    println!("Prismic APIs");
    println!();
    println!("REST CONTENT API");
    println!("  Base:    https://<repo>.cdn.prismic.io/api/v2");
    println!("  Pattern: GET /api/v2  -> returns an 'ref' (immutable content version)");
    println!("           GET /api/v2/documents/search?ref=<ref>&q=<predicate>");
    println!("  Refs:    Every publish produces a new ref. Front-ends fetch");
    println!("           with the ref returned from /api/v2 to ensure a");
    println!("           consistent snapshot across many parallel requests.");
    println!();
    println!("PREDICATES");
    println!("  q=[at(document.type, 'page')]");
    println!("  q=[at(my.page.uid, 'home')]");
    println!("  q=[fulltext(my.page.title, 'hello world')]");
    println!("  Composable with and/or; supports date ranges, geo, similarity.");
    println!();
    println!("GRAPHQL");
    println!("  https://<repo>.cdn.prismic.io/graphql with full schema");
    println!("  introspection. Read-only; write APIs remain REST + Migration.");
    println!();
    println!("MIGRATION API");
    println!("  Batched create/update/delete of documents. Used by official");
    println!("  Prismic CLI for content imports/exports + repo cloning.");
    println!();
    println!("PREVIEW + RELEASES");
    println!("  Preview tokens deliver unpublished content via a short-lived");
    println!("  cookie. Releases bundle multiple draft documents to ship");
    println!("  together (e.g., a campaign launch).");
}

fn cmd_funding() {
    println!("Prismic — funding history");
    println!();
    println!("  2013  Founded, bootstrapped from Zengularity consulting revenue.");
    println!("        Sadek Drobi + Guillaume Bort fund development with");
    println!("        agency profits while building the product.");
    println!();
    println!("  2019  Seed: $7M, Idinvest Partners (now Eurazeo Investment");
    println!("        Manager) lead. First institutional round, 6 years in.");
    println!();
    println!("  ~2022 Series A: undisclosed, with Eurazeo + later additions.");
    println!("        Prismic has been deliberately quiet about subsequent");
    println!("        rounds, leaning into capital efficiency over headlines.");
    println!();
    println!("Total disclosed: in the low double-digit millions, far less");
    println!("than Contentful ($333M) or Storyblok ($135M+). Prismic is the");
    println!("longest-running and most lightly-capitalized of the European");
    println!("headless CMS pack — and one of the more profitable.");
}

fn cmd_customers() {
    println!("Selected Prismic customers");
    println!();
    println!("  Google              — selected internal sites + Cloud customer stories");
    println!("  Deliveroo           — global marketing");
    println!("  Hinge               — relationship app, marketing site");
    println!("  Dropbox             — selected campaign sites");
    println!("  Castorama (Kingfisher) — UK retail content");
    println!("  Eventbrite          — selected verticals");
    println!("  Le Wagon            — coding bootcamp global sites");
    println!("  Doctolib            — French healthcare booking, marketing");
    println!("  Trainline           — European rail booking marketing");
    println!("  Yousign             — French esignature SaaS");
    println!();
    println!("Sweet spot: marketing teams at European tech companies and");
    println!("ambitious agencies who pair Prismic with Next.js, Nuxt, or");
    println!("SvelteKit and ship slice-based marketing sites with weekly");
    println!("design iteration cadence.");
}

fn cmd_pricing() {
    println!("Prismic pricing (as of 2024)");
    println!();
    println!("  Free       $0/mo");
    println!("             1 user, public repos only, all core features.");
    println!();
    println!("  Starter    $7/mo  (1 user, private repos, scheduled publishing)");
    println!("  Small      $15/mo (3 users, releases, advanced roles)");
    println!("  Medium     $100/mo (7 users, custom roles, audit log)");
    println!("  Platinum   $500/mo (25 users, SLA, advanced security)");
    println!("  Enterprise Custom (SSO/SAML, dedicated CSM, custom DPA)");
    println!();
    println!("Pricing per repository (i.e., per project). Users are billed");
    println!("by access tier; viewers are free. API requests + asset bandwidth");
    println!("metered but generous on the included quotas.");
}

fn cmd_sdks() {
    println!("Official Prismic SDKs");
    println!();
    println!("FRAMEWORK INTEGRATIONS");
    println!("  @prismicio/client      — vanilla JS/TS, the foundation library");
    println!("  @prismicio/next        — Next.js (App Router + Pages Router)");
    println!("  @prismicio/nuxt        — Nuxt 3 module");
    println!("  @prismicio/svelte      — SvelteKit (community-led, official soon)");
    println!("  @prismicio/vue         — Vue 3 plugin");
    println!("  @prismicio/react       — React components for rich text + images");
    println!("  prismic-helpers (PHP, Python, Ruby, Java, Elixir) — community");
    println!();
    println!("SLICE MACHINE");
    println!("  npx slicemachine init  — bootstraps Slice Machine in your project.");
    println!("  Runs locally on http://localhost:9999, edits slice schemas,");
    println!("  scaffolds slice components, syncs the model to your repo.");
    println!();
    println!("CLI");
    println!("  npx @slicemachine/init       Set up Slice Machine");
    println!("  npx @slicemachine/manager    Manage repos from CLI");
    println!("  npx prismic-cli              Legacy CLI (deprecated)");
}

fn run_prismic(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "slices" => { cmd_slices(); 0 }
        "api" => { cmd_api(); 0 }
        "funding" => { cmd_funding(); 0 }
        "customers" => { cmd_customers(); 0 }
        "pricing" => { cmd_pricing(); 0 }
        "sdks" => { cmd_sdks(); 0 }
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
        .unwrap_or_else(|| "prismic".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_prismic(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/prismic"), "prismic");
        assert_eq!(basename("C:\\Tools\\prismic.exe"), "prismic.exe");
        assert_eq!(basename("prismic"), "prismic");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("prismic.exe"), "prismic");
        assert_eq!(strip_ext("prismic"), "prismic");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_prismic(&["help".to_string()], "prismic"), 0);
        let _ = run_prismic(&[], "prismic");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_prismic(&["nope".to_string()], "prismic"), 2);
    }
}
