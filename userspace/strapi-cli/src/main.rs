#![deny(clippy::all)]
//! strapi-cli — SlateOS personality CLI for Strapi, the open-source Node.js headless CMS.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Strapi — the open-source headless CMS that respects your code.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Founders, Paris origins, the open-source bet");
    println!("    v5          Strapi 5 — TypeScript-first, doc service, plugins");
    println!("    api         REST API and GraphQL plugin, content-types, components");
    println!("    cloud       Strapi Cloud — managed hosting for Strapi projects");
    println!("    funding     OSS funding rounds, Insight Partners lead");
    println!("    license     Open-source license, enterprise edition");
    println!("    customers   Toyota, IBM, Walmart, NASA, Discovery, others");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("100% JavaScript/TypeScript. Self-hostable. Yours forever.");
}

fn print_version() {
    println!("strapi-cli 0.1.0");
    println!("Strapi SAS — Paris, France. Founded 2015. Strapi v5 GA in 2024.");
}

fn cmd_about() {
    println!("Strapi — the leading open-source headless CMS");
    println!();
    println!("FOUNDED");
    println!("  2015 in Paris, France, by Pierre Burgy, Aurelien Georget, and");
    println!("  Jim Laurie. The trio met at Epitech, an engineering school");
    println!("  outside Paris, while building a side project. They needed a");
    println!("  flexible content API and could not find one — so they wrote");
    println!("  one in Node.js and open-sourced it as Strapi (from 'boot-strap'");
    println!("  + 'API').");
    println!();
    println!("OPEN SOURCE FIRST");
    println!("  Strapi has never gated its core behind a paywall. The code is");
    println!("  MIT-licensed for the community edition and has been GitHub-");
    println!("  trending for years. As of 2024: ~63K GitHub stars, ~600 open-");
    println!("  source contributors, ~50M total npm downloads.");
    println!();
    println!("HEADQUARTERS");
    println!("  Paris, France. Roughly 150 employees, remote-friendly across");
    println!("  Europe + North America.");
}

fn cmd_v5() {
    println!("Strapi 5 — released October 2024");
    println!();
    println!("HIGHLIGHTS");
    println!("  - TypeScript-first: full TS types generated from your schema");
    println!("  - New Document Service API replacing the legacy Entity Service");
    println!("  - Draft & Publish redesigned as first-class document state");
    println!("  - Content History — see and restore previous versions");
    println!("  - Release management — schedule grouped content releases");
    println!("  - Plugin SDK 2 — Vite-based, faster builds, simpler API");
    println!("  - Admin panel rebuilt in React 18 with refreshed UI");
    println!();
    println!("UPGRADE FROM v4");
    println!("  npx @strapi/upgrade  — runs codemods on your codebase.");
    println!("  Breaking changes: lifecycle hooks signature, populate syntax,");
    println!("  removal of deprecated REST query params. Most v4 projects");
    println!("  port in under a day for typical content shapes.");
    println!();
    println!("STACK");
    println!("  Node.js 18+, Koa (HTTP), Knex (SQL), supports PostgreSQL,");
    println!("  MySQL, MariaDB, SQLite. Mongo support dropped in v4.");
}

fn cmd_api() {
    println!("Strapi APIs and content modeling");
    println!();
    println!("CONTENT TYPES");
    println!("  Define your data model in the admin panel or as TypeScript");
    println!("  files under src/api/<type>/content-types/<type>/schema.json.");
    println!("  Fields: text, richtext, integer, boolean, date, json, email,");
    println!("  password, enum, media, relation, component, dynamic zone, uid.");
    println!();
    println!("COMPONENTS & DYNAMIC ZONES");
    println!("  Components: reusable field groups (e.g., a 'seo' component");
    println!("  with title/description/og-image fields).");
    println!("  Dynamic zones: arrays of polymorphic components — perfect for");
    println!("  page-builder content where each section can be a different shape.");
    println!();
    println!("REST API");
    println!("  Auto-generated. GET /api/articles?populate=author&filters[status][$eq]=published");
    println!("  Query params: filters, sort, populate, fields, pagination, locale.");
    println!();
    println!("GRAPHQL");
    println!("  Official plugin (@strapi/plugin-graphql). One endpoint, full");
    println!("  schema introspection, supports filters/sort/populate equivalents.");
    println!();
    println!("ROLES & PERMISSIONS");
    println!("  Public + Authenticated roles by default. Custom roles, JWT");
    println!("  auth, API tokens, SSO via enterprise edition.");
}

fn cmd_cloud() {
    println!("Strapi Cloud — official managed hosting");
    println!();
    println!("WHAT IT IS");
    println!("  Strapi's first commercial offering (launched 2023). You point");
    println!("  it at your GitHub repo containing a Strapi project; it builds,");
    println!("  deploys, and operates the instance plus its PostgreSQL.");
    println!();
    println!("REGIONS");
    println!("  AWS us-east-1, eu-west-3, ap-south-1, with more being added.");
    println!("  CDN via CloudFront for media uploads.");
    println!();
    println!("PLANS (as of 2024)");
    println!("  Free Trial   14 days, 1 project, no credit card.");
    println!("  Essential    ~$15/mo project (entry-level traffic).");
    println!("  Pro          ~$99/mo project (production workloads).");
    println!("  Team         ~$499/mo project (multi-env: dev/staging/prod).");
    println!("  Custom       Enterprise SLA, custom regions, dedicated CSM.");
    println!();
    println!("SELF-HOST REMAINS FREE");
    println!("  Strapi Cloud is convenience, not lock-in. Same Strapi core,");
    println!("  same DB schema, same code. You can move on or off Cloud at");
    println!("  any time with a git push and a database dump.");
}

fn cmd_funding() {
    println!("Strapi — funding history");
    println!();
    println!("  2019  Seed: $4M, Accel + Index Ventures + Stride. Reinforces");
    println!("        commercial open-source positioning.");
    println!();
    println!("  2021  Series A: $10M, Index Ventures lead. Hiring expanded");
    println!("        toward enterprise edition and dev tooling.");
    println!();
    println!("  2022  Series B: $31M, Insight Partners lead. Reported");
    println!("        valuation ~$150-200M in ZIRP-era pricing. Used to");
    println!("        build Strapi Cloud + accelerate v5 development.");
    println!();
    println!("Total disclosed: ~$45M across known rounds.");
    println!("Strapi has emphasized capital efficiency since 2023, keeping");
    println!("headcount steady and prioritizing Cloud revenue ramp.");
}

fn cmd_license() {
    println!("Strapi licensing");
    println!();
    println!("COMMUNITY EDITION (CE)");
    println!("  License: MIT.");
    println!("  Everything you need to build production sites: content");
    println!("  modeling, REST + GraphQL, plugins, media library, i18n,");
    println!("  webhooks. Self-host on any infra. Free forever.");
    println!();
    println!("ENTERPRISE EDITION (EE)");
    println!("  License: Strapi commercial license (proprietary).");
    println!("  Adds: SSO/SAML, audit logs, advanced role-based access");
    println!("  control with granular field-level permissions, content");
    println!("  releases (in CE since v5), review workflows, premium support.");
    println!("  Sold per environment; pricing on request.");
    println!();
    println!("STRAPI'S LICENSE PROMISE");
    println!("  Strapi has publicly committed to never relicense the core");
    println!("  away from MIT. Unlike some open-source vendors who flipped to");
    println!("  BUSL/SSPL under VC pressure, Strapi's CE is the product, and");
    println!("  the company sells convenience and support around it.");
}

fn cmd_customers() {
    println!("Selected Strapi customers");
    println!();
    println!("  Toyota          — Lexus EU + global product configurators");
    println!("  IBM             — internal portals and event sites");
    println!("  Walmart         — selected product content pipelines");
    println!("  NASA            — public outreach microsites");
    println!("  Discovery Inc.  — branded content sites");
    println!("  Capgemini       — agency-built client deployments");
    println!("  Accenture       — internal knowledge bases");
    println!("  Societe Generale — campaign and recruitment sites");
    println!("  Rakuten         — content ops platforms");
    println!();
    println!("Sweet spot: orgs that want a real CMS for their content team,");
    println!("a real Node.js codebase for their developers, and either self-");
    println!("hosted or managed Cloud deployment — without per-seat SaaS lock-in.");
}

fn run_strapi(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "v5" => { cmd_v5(); 0 }
        "api" => { cmd_api(); 0 }
        "cloud" => { cmd_cloud(); 0 }
        "funding" => { cmd_funding(); 0 }
        "license" => { cmd_license(); 0 }
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
        .unwrap_or_else(|| "strapi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_strapi(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/strapi"), "strapi");
        assert_eq!(basename("C:\\Tools\\strapi.exe"), "strapi.exe");
        assert_eq!(basename("strapi"), "strapi");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("strapi.exe"), "strapi");
        assert_eq!(strip_ext("strapi"), "strapi");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_strapi(&["help".to_string()], "strapi"), 0);
        let _ = run_strapi(&[], "strapi");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_strapi(&["nope".to_string()], "strapi"), 2);
    }
}
