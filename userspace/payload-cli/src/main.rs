#![deny(clippy::all)]
//! payload-cli — OurOS personality CLI for Payload, the Next.js-native headless CMS.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Payload — the Next.js-native, TypeScript-first headless CMS.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Founders, Camden NJ origins, code-first thesis");
    println!("    config      Collections, globals, fields — config is the schema");
    println!("    v3          Payload 3 — installs into your Next.js app");
    println!("    api         REST, GraphQL, and the Local API");
    println!("    cloud       Payload Cloud — official managed hosting");
    println!("    vercel      The Vercel acquisition (October 2024)");
    println!("    customers   Microsoft, Apple, NASA-adjacent agency builds");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("No dashboard. No UI builder. Just code, types, and your data.");
}

fn print_version() {
    println!("payload-cli 0.1.0");
    println!("Payload CMS, Inc. — Camden, NJ. Founded 2018. Acquired by Vercel Oct 30, 2024.");
}

fn cmd_about() {
    println!("Payload — the Next.js-native headless CMS");
    println!();
    println!("FOUNDED");
    println!("  Started in 2018 as an internal tool at TRBL, a Camden NJ");
    println!("  design+development studio co-founded by James Mikrut, Dan");
    println!("  Ribbens, and Elliot DeNolf. They were repeatedly building");
    println!("  similar headless-CMS scaffolding for client projects in");
    println!("  Node + React, and decided to extract a reusable core.");
    println!();
    println!("OPEN SOURCE LAUNCH");
    println!("  Payload v1 went open-source under MIT in 2020. The thesis:");
    println!("  developers want a headless CMS configured as code (TypeScript),");
    println!("  not assembled in a no-code dashboard. The admin UI is the");
    println!("  output of your config, not the input.");
    println!();
    println!("CORPORATE STRUCTURE");
    println!("  Spun out from TRBL as Payload CMS, Inc. in 2022. Raised");
    println!("  $4.7M seed in 2023 led by Bowery Capital. Acquired by");
    println!("  Vercel on October 30, 2024 — terms not disclosed, but the");
    println!("  team and product remain headquartered in Camden NJ.");
}

fn cmd_config() {
    println!("Payload's config-as-schema model");
    println!();
    println!("THE BIG IDEA");
    println!("  Your entire CMS — collections, globals, fields, access");
    println!("  control, hooks, admin UI — is defined in payload.config.ts.");
    println!("  Edit a field, it appears in the admin UI; edit a hook, it");
    println!("  runs on save; edit access control, it propagates everywhere.");
    println!();
    println!("COLLECTIONS");
    println!("  Repeatable document types: Posts, Products, Users, Media.");
    println!("  Each collection has its own slug, fields, hooks, and access.");
    println!();
    println!("GLOBALS");
    println!("  Singletons: Header, Footer, Site Settings. Same field");
    println!("  primitives as collections but only one instance.");
    println!();
    println!("FIELDS");
    println!("  text, textarea, email, code, number, date, point, checkbox,");
    println!("  select, radio, relationship, upload, group, array, blocks,");
    println!("  tabs, collapsible, row, ui, richText (Lexical-based),");
    println!("  json. Custom fields are first-class.");
    println!();
    println!("ACCESS CONTROL");
    println!("  A function per operation per collection: read, create,");
    println!("  update, delete. Receives req+user, returns boolean | Where.");
    println!("  Returning a Where automatically filters list/query results.");
}

fn cmd_v3() {
    println!("Payload 3 — released July 2024");
    println!();
    println!("THE BIG SHIFT");
    println!("  Payload 3 installs directly into your Next.js app. Previous");
    println!("  versions ran as a standalone Express server alongside your");
    println!("  frontend; v3 mounts as a Next.js route handler so admin UI,");
    println!("  API, and your storefront/blog share one runtime, one deploy,");
    println!("  one set of env vars.");
    println!();
    println!("RUNTIME REQUIREMENTS");
    println!("  Node.js 20+, Next.js 14.2+, App Router, React 19 compatible.");
    println!("  Drizzle is the official ORM (Postgres + SQLite supported");
    println!("  natively; Mongo via mongoose retains v2 parity).");
    println!();
    println!("MAJOR NEW FEATURES");
    println!("  - Server components for the admin UI (faster, leaner)");
    println!("  - Live preview with iframe overlay on real production data");
    println!("  - Lexical-based rich text replacing Slate (better paste, embeds)");
    println!("  - Jobs queue (cron + adhoc) running inside Next.js");
    println!("  - Postgres-native auto-migrations during dev");
    println!();
    println!("MIGRATION FROM v2");
    println!("  npx create-payload-app --template v2-to-v3 walks the");
    println!("  conversion. Most schemas port unchanged; the route");
    println!("  mounting and richText migration need attention.");
}

fn cmd_api() {
    println!("Payload APIs");
    println!();
    println!("REST API");
    println!("  /api/<collection>?where[status][equals]=published&depth=2&limit=10");
    println!("  Auto-generated per collection. Supports filters, sort,");
    println!("  pagination, populate depth, locale selection.");
    println!();
    println!("GRAPHQL");
    println!("  /api/graphql with full introspection. Schema generated from");
    println!("  your config. Mutations for create/update/delete; queries");
    println!("  for findOne/find/count. GraphQL playground in dev mode.");
    println!();
    println!("LOCAL API");
    println!("  payload.find({{ collection: 'posts', where: {{ ... }} }})");
    println!("  Direct in-process database access — no HTTP — bypassing");
    println!("  access control unless you opt in. Perfect for getStaticProps,");
    println!("  server components, cron jobs, custom scripts.");
    println!();
    println!("AUTH");
    println!("  Built-in JWT auth per collection. Mark a collection as");
    println!("  'auth: true' and you get users, login, logout, email-verify,");
    println!("  password-reset, max-login-attempts, lockouts, all included.");
    println!();
    println!("HOOKS");
    println!("  beforeChange, afterChange, beforeRead, afterRead, beforeDelete,");
    println!("  afterDelete, beforeOperation, afterOperation, beforeLogin,");
    println!("  afterLogin — at collection, field, and global level.");
}

fn cmd_cloud() {
    println!("Payload Cloud — official managed hosting");
    println!();
    println!("WHAT IT IS");
    println!("  Launched 2023. AWS-backed (us-east, eu-west, ap-southeast).");
    println!("  Push your Payload project to a connected GitHub repo;");
    println!("  Cloud builds, deploys, provides Postgres, S3-style media,");
    println!("  CDN, and the admin URL.");
    println!();
    println!("PLANS (pre-Vercel acquisition pricing as of mid-2024)");
    println!("  Standard      $35/mo project");
    println!("  Pro           $199/mo project (larger DB, dedicated resources)");
    println!("  Enterprise    Custom (HA Postgres, custom regions, SLA)");
    println!();
    println!("POST-VERCEL");
    println!("  Payload Cloud continues to operate. Roadmap signals a deeper");
    println!("  integration with Vercel hosting (deploy your Payload-in-Next.js");
    println!("  app to Vercel + Vercel Postgres in one flow). Existing Cloud");
    println!("  customers honored on current terms.");
}

fn cmd_vercel() {
    println!("The Vercel acquisition");
    println!();
    println!("ANNOUNCED");
    println!("  October 30, 2024. Acquired by Vercel Inc. Terms undisclosed.");
    println!("  Payload remains in Camden NJ; the team keeps its identity and");
    println!("  branding. James Mikrut continues as CEO of the Payload business");
    println!("  unit reporting into Vercel.");
    println!();
    println!("RATIONALE (from both companies' announcements)");
    println!("  Vercel: completes the Next.js stack — Vercel for infra,");
    println!("  Next.js for framework, Payload for data + content layer.");
    println!("  Payload: brings the scale and reach of a public-comps-grade");
    println!("  distribution partner without compromising open-source roots.");
    println!();
    println!("COMMITMENTS");
    println!("  - Payload remains MIT-licensed");
    println!("  - Self-host stays first-class, no feature flagging");
    println!("  - Roadmap published in public repo, community proposals open");
    println!("  - Database-agnostic (no forced Vercel Postgres lock-in)");
    println!();
    println!("WHAT TO WATCH");
    println!("  Whether Vercel will maintain Payload's neutrality if another");
    println!("  framework (Astro, Remix, SvelteKit) demands first-class");
    println!("  Payload integrations. Early signals say yes — Payload 3 ships");
    println!("  Astro and Remix adapters alongside the Next.js install path.");
}

fn cmd_customers() {
    println!("Selected Payload customers");
    println!();
    println!("  Microsoft         — selected campaign sites (via agencies)");
    println!("  Apple             — internal events, partner portals");
    println!("  HelloFresh        — recipe content infrastructure");
    println!("  Hawaiian Airlines — marketing properties");
    println!("  Yale University   — selected departmental sites");
    println!("  Tesco             — UK ecommerce content streams");
    println!("  Outvio            — logistics SaaS marketing site");
    println!("  Reuters           — selected verticals");
    println!("  Mythical Games    — Web3 game studio portals");
    println!("  Atomic Industries — manufacturing marketing");
    println!();
    println!("Sweet spot: Next.js-shop agencies and product teams who want a");
    println!("real CMS without leaving the TypeScript toolchain or running");
    println!("a separate Node service alongside their Next.js app.");
}

fn run_payload(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "config" => { cmd_config(); 0 }
        "v3" => { cmd_v3(); 0 }
        "api" => { cmd_api(); 0 }
        "cloud" => { cmd_cloud(); 0 }
        "vercel" => { cmd_vercel(); 0 }
        "customers" => { cmd_customers(); 0 }
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
        .unwrap_or_else(|| "payload".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_payload(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/payload"), "payload");
        assert_eq!(basename("C:\\Tools\\payload.exe"), "payload.exe");
        assert_eq!(basename("payload"), "payload");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("payload.exe"), "payload");
        assert_eq!(strip_ext("payload"), "payload");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_payload(&["help".to_string()], "payload"), 0);
        assert_eq!(run_payload(&[], "payload"), 0);
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_payload(&["nope".to_string()], "payload"), 2);
    }
}
