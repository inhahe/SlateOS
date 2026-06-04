#![deny(clippy::all)]
//! storyblok-cli — OurOS personality CLI for Storyblok, the visual-editing headless CMS.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Storyblok — the headless CMS that lets marketers see what they edit.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Founders, Linz Austria origins, visual-edit thesis");
    println!("    visual      The Visual Editor — live preview alongside the iframe");
    println!("    blocks      Block-based content modeling and reusable nestables");
    println!("    api         Content Delivery API, Management API, image service");
    println!("    funding     $80M Series C and the European-SaaS unicorn arc");
    println!("    customers   Adidas, Tesla, Renault, Marc O'Polo, others");
    println!("    plans       Free, Entry, Teams, Business, Enterprise");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Headless API + the visual editor that headless CMSes always forgot.");
}

fn print_version() {
    println!("storyblok-cli 0.1.0");
    println!("Storyblok GmbH — Linz, Austria. Founded 2017.");
}

fn cmd_about() {
    println!("Storyblok — the visual-edit-first headless CMS");
    println!();
    println!("FOUNDED");
    println!("  2017 in Linz, Austria, by Dominik Angerer and Alexander");
    println!("  Feiglstorfer. Both came from agency work building Symfony/PHP");
    println!("  CMS sites and were frustrated that the new wave of headless");
    println!("  CMSes (Contentful, Prismic) traded marketing-team usability");
    println!("  for developer ergonomics. Their thesis: developers can have");
    println!("  the headless API they want AND marketers can have the live");
    println!("  visual preview they need — if the editor renders inside the");
    println!("  actual front-end, not a fake preview pane.");
    println!();
    println!("HEADQUARTERS");
    println!("  Linz, Austria (HQ), plus distributed teams across Europe");
    println!("  and Latin America. ~250 employees. CEO Dominik Angerer.");
    println!();
    println!("FUN FACT");
    println!("  The company name comes from 'Story' (content) + 'block'");
    println!("  (composable units). Visual block composition is the through-");
    println!("  line of the whole product.");
}

fn cmd_visual() {
    println!("The Storyblok Visual Editor");
    println!();
    println!("HOW IT WORKS");
    println!("  Storyblok's admin renders your actual front-end inside an");
    println!("  iframe. You point the visual editor at https://your-site/some-");
    println!("  page, your front-end loads, and Storyblok injects a small");
    println!("  bridge script. Clicking on a block in the iframe focuses the");
    println!("  corresponding field in the side editor. Editing the field");
    println!("  updates the iframe in real time via postMessage.");
    println!();
    println!("THE BRIDGE");
    println!("  @storyblok/js (or @storyblok/react, /nuxt, /astro, /vue)");
    println!("  attaches editable={{true}} attributes that mark which DOM nodes");
    println!("  correspond to which Storyblok blocks. Clicks become focus");
    println!("  events; content updates trigger re-render.");
    println!();
    println!("DRAFT / PUBLISHED");
    println!("  Two parallel API delivery modes: 'draft' (current edits, served");
    println!("  with a preview token) and 'published' (last published version,");
    println!("  CDN-cached). The editor iframe loads 'draft' so marketers see");
    println!("  unpublished changes; production renders 'published'.");
}

fn cmd_blocks() {
    println!("Storyblok content modeling");
    println!();
    println!("CONTENT TYPES");
    println!("  - Content type:    top-level document (e.g., Page, Article)");
    println!("  - Nestable block:  reusable block embedded in others");
    println!("  - Universal block: can be either content-type or nestable");
    println!();
    println!("FIELD TYPES");
    println!("  text, textarea, markdown, richtext, number, boolean, datetime,");
    println!("  asset (single + multi), multilink (internal / external / email /");
    println!("  asset / story), option (single-select), options (multi-select),");
    println!("  blocks (nestable composition), table, custom (plugin field).");
    println!();
    println!("THE 'blocks' FIELD");
    println!("  The star of the show. A blocks field on a Page can contain an");
    println!("  ordered list of any number of Hero, Teaser, Grid, Feature,");
    println!("  Testimonial blocks — each with their own fields and visual");
    println!("  preview. This is what marketers compose pages from.");
    println!();
    println!("CONTENT DELIVERY");
    println!("  Stories are JSON. The 'content' field of a story is a tree:");
    println!("  every nested block contributes a {{ component: 'name', ... }}");
    println!("  shape. Front-end code maps component name -> React/Vue/Svelte");
    println!("  component to render.");
}

fn cmd_api() {
    println!("Storyblok APIs");
    println!();
    println!("CONTENT DELIVERY API v2");
    println!("  Base:   https://api.storyblok.com/v2/cdn/");
    println!("  Routes: /stories, /stories/<slug>, /datasource_entries, /tags");
    println!("  Filters: by component, by field value, by tag, by language, by date");
    println!("  Resolve: links (resolve_links=url|story), relations (resolve_relations)");
    println!("  Languages: alternates per content type, ?language=de-DE selector");
    println!("  CDN: edge-cached globally; cv (content_version) cache-buster on publish");
    println!();
    println!("MANAGEMENT API");
    println!("  Base: https://mapi.storyblok.com/v1/spaces/<id>/");
    println!("  Personal access tokens or OAuth. Full CRUD over stories,");
    println!("  components, datasources, presets, assets, releases, workflows.");
    println!();
    println!("IMAGE SERVICE");
    println!("  https://a.storyblok.com/f/<spaceId>/<dim>/<hash>/<filename>/m/<transform>");
    println!("  On-the-fly resize/crop/format/quality. WebP + AVIF support.");
    println!("  /smart suffix uses face/edge detection for crops.");
    println!();
    println!("WEBHOOKS");
    println!("  Outbound on publish, unpublish, delete, move. Common targets:");
    println!("  Vercel ISR revalidation, Netlify build hooks, Algolia indexers.");
}

fn cmd_funding() {
    println!("Storyblok — funding history");
    println!();
    println!("  2018  Bootstrapped beginnings, agency-funded.");
    println!();
    println!("  2020  Seed: ~$2M, Mubadala (then), HV Capital, local Austrian VCs.");
    println!();
    println!("  2022  Series B: $47M, Mubadala Capital lead, with HV Capital,");
    println!("        3VC, Notion Capital. Valuation ~$240M.");
    println!();
    println!("  2024  Series C: $80M, announced May 2024. Mubadala Capital lead");
    println!("        with NEA, HV Capital, and 3VC participating. Valuation");
    println!("        reportedly crossed $500M post-money — a rare European");
    println!("        SaaS up-round after the 2022-2023 reset. Use of proceeds:");
    println!("        AI features (composable content generation), expansion of");
    println!("        the Linz + Lisbon + Sao Paulo engineering centers.");
    println!();
    println!("Total disclosed: ~$135M+ across known rounds.");
}

fn cmd_customers() {
    println!("Selected Storyblok customers");
    println!();
    println!("  Adidas              — selected global ecommerce campaigns");
    println!("  Tesla               — internal events and partner sites");
    println!("  Renault             — European model microsites");
    println!("  Deliveroo           — selected market sites");
    println!("  Marc O'Polo         — global storefront content");
    println!("  Decathlon Engineering — internal portals");
    println!("  Lufthansa           — selected route landing pages");
    println!("  Pizza Hut UK        — campaign + menu CMS");
    println!("  Education First (EF) — global edu marketing");
    println!("  Netflix Tudum (early) — fan portal experiments");
    println!();
    println!("Sweet spot: brands with serious marketing teams who want");
    println!("page-builder ergonomics without sacrificing the headless");
    println!("API stack — large enough to hate Webflow's render limits");
    println!("but tired of Adobe Experience Manager's TCO.");
}

fn cmd_plans() {
    println!("Storyblok plans (as of 2024)");
    println!();
    println!("  Community    $0/mo");
    println!("               1 user, 1 space, basic visual editor, free CDN.");
    println!();
    println!("  Entry        ~$90/mo");
    println!("               5 users, releases, scheduling, basic webhooks.");
    println!();
    println!("  Teams        ~$420/mo");
    println!("               15+ users, workflows, advanced roles, audit log.");
    println!();
    println!("  Business     ~$1,790/mo");
    println!("               unlimited users, SSO, audit log, premium support,");
    println!("               higher API request + asset bandwidth.");
    println!();
    println!("  Enterprise   Custom");
    println!("               dedicated infra, advanced SSO/SAML, custom SLAs,");
    println!("               regional residency, AI Translate add-on.");
    println!();
    println!("Pricing dimension is 'users + included traffic + features'.");
    println!("Generous content-version & multi-locale allowances vs. peers.");
}

fn run_storyblok(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "visual" => { cmd_visual(); 0 }
        "blocks" => { cmd_blocks(); 0 }
        "api" => { cmd_api(); 0 }
        "funding" => { cmd_funding(); 0 }
        "customers" => { cmd_customers(); 0 }
        "plans" => { cmd_plans(); 0 }
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
        .unwrap_or_else(|| "storyblok".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_storyblok(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/storyblok"), "storyblok");
        assert_eq!(basename("C:\\Tools\\storyblok.exe"), "storyblok.exe");
        assert_eq!(basename("storyblok"), "storyblok");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("storyblok.exe"), "storyblok");
        assert_eq!(strip_ext("storyblok"), "storyblok");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_storyblok(&["help".to_string()], "storyblok"), 0);
        let _ = run_storyblok(&[], "storyblok");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_storyblok(&["nope".to_string()], "storyblok"), 2);
    }
}
