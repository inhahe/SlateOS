#![deny(clippy::all)]
//! directus-cli — SlateOS personality CLI for Directus, the open-data platform.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Directus — the open data platform that wraps any SQL database.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Founders, the rewrite that bet on Node + Vue");
    println!("    database    Bring-your-own SQL: Postgres, MySQL, SQLite, etc.");
    println!("    app         The admin app — instant CRUD on any schema");
    println!("    api         REST and GraphQL, both auto-generated");
    println!("    flows       Visual automation flows + extensions");
    println!("    license     BUSL 1.1 — the source-available pivot of 2023");
    println!("    cloud       Directus Cloud — official managed hosting");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Your data, your database, your platform.");
}

fn print_version() {
    println!("directus-cli 0.1.0");
    println!("Monospace, Inc. (dba Directus) — New York. Founded 2017.");
}

fn cmd_about() {
    println!("Directus — the open data platform");
    println!();
    println!("FOUNDED");
    println!("  Started in 2004 as RANGER, an internal PHP CMS at the design");
    println!("  studio RANGER Studio in NYC. Open-sourced as Directus in 2009.");
    println!("  Rewritten from scratch as Directus 9 in 2017 on Node.js + Vue 3");
    println!("  by Ben Haynes (CEO) and Rijk van Zanten (CTO). The rewrite");
    println!("  shed PHP, restructured the schema-first model, and pivoted");
    println!("  from 'headless CMS' to 'open data platform.'");
    println!();
    println!("THE THESIS");
    println!("  Most CMS vendors force you into their proprietary database");
    println!("  schema. Directus inverts this: point it at any existing SQL");
    println!("  database and Directus introspects the schema, generating a");
    println!("  full admin UI and REST + GraphQL APIs in seconds. Your DB");
    println!("  remains canonical and portable; Directus is an interchangeable");
    println!("  presentation layer.");
    println!();
    println!("HEADQUARTERS");
    println!("  New York City + remote. ~80 employees. Backed by True Ventures.");
}

fn cmd_database() {
    println!("Bring-your-own database");
    println!();
    println!("SUPPORTED ENGINES");
    println!("  - PostgreSQL    9.5+      (recommended)");
    println!("  - MySQL         5.7.8+    (and MariaDB 10.2+)");
    println!("  - SQLite        3.x       (development / small projects)");
    println!("  - Microsoft SQL Server 2019+");
    println!("  - Oracle Database 19c+");
    println!("  - CockroachDB   21.1+");
    println!("  - AWS Aurora    (MySQL + Postgres flavors)");
    println!();
    println!("SCHEMA INTROSPECTION");
    println!("  Point Directus at any existing database. It reads INFORMATION_SCHEMA");
    println!("  (or pg_catalog), generates a directus_collections + directus_fields");
    println!("  metadata layer, and exposes everything via the admin app + APIs.");
    println!();
    println!("  Your existing tables stay yours. Directus stores its config in");
    println!("  a handful of directus_* tables alongside. Drop Directus, your");
    println!("  database keeps working exactly as it did before.");
    println!();
    println!("MIGRATIONS");
    println!("  Schema changes go through directus.knex.js migrations or via");
    println!("  the schema snapshot/apply CLI, suitable for git-based workflows.");
}

fn cmd_app() {
    println!("The Directus App");
    println!();
    println!("WHAT IT IS");
    println!("  A Vue 3 single-page application that becomes a full admin UI");
    println!("  for any database Directus is pointed at. Tables become");
    println!("  Collections; columns become Fields; FKs become Relations.");
    println!();
    println!("INTERFACES");
    println!("  Each field uses an Interface — the editor UI for that data");
    println!("  type. Built-in: text input, WYSIWYG, code, color, image,");
    println!("  file upload, M2A relations, repeater, datetime, map, slider,");
    println!("  tags, dropdown, autocomplete, slug. Custom interfaces are");
    println!("  Vue components in the extensions/ folder.");
    println!();
    println!("LAYOUTS");
    println!("  How Collection lists are visualized: tabular, cards, calendar,");
    println!("  kanban, map. Layouts are pluggable extensions too.");
    println!();
    println!("INSIGHTS");
    println!("  Dashboards with charts (line, bar, pie, KPI, list, time series)");
    println!("  bound to SQL queries — turns Directus into a lightweight BI tool.");
    println!();
    println!("ROLES & PERMISSIONS");
    println!("  Row-level + field-level granularity. Conditional permissions");
    println!("  (\"user can edit Posts where author = $CURRENT_USER\")");
    println!("  expressed as a filter rule, applied to all APIs uniformly.");
}

fn cmd_api() {
    println!("Directus APIs");
    println!();
    println!("REST API");
    println!("  /items/<collection>?filter[status][_eq]=published");
    println!("    &fields=*,author.name&deep[comments][_limit]=5");
    println!("  Auto-generated per collection. Powerful filter operators,");
    println!("  field selection, relational deep parameters, aggregations.");
    println!();
    println!("GRAPHQL");
    println!("  /graphql with full schema introspection. Same access");
    println!("  control as REST; permissions enforced server-side regardless");
    println!("  of which API you use.");
    println!();
    println!("WEBSOCKETS");
    println!("  Realtime subscriptions for collection changes — useful for");
    println!("  collaborative editing, live dashboards, monitoring use cases.");
    println!();
    println!("ASSETS API");
    println!("  /assets/<file-id>?key=hero&width=1200&format=webp");
    println!("  Transforms on the fly: resize, crop, format, quality. Named");
    println!("  presets configured per-project for predictable URLs.");
    println!();
    println!("AUTHENTICATION");
    println!("  Email/password, OAuth (Google, Facebook, GitHub, Discord),");
    println!("  OpenID Connect, LDAP, SAML. JWT tokens with refresh rotation.");
}

fn cmd_flows() {
    println!("Flows and extensions");
    println!();
    println!("FLOWS");
    println!("  Visual no-code automation builder added in Directus 9.18.");
    println!("  Trigger (event, schedule, webhook, manual) -> chain of");
    println!("  Operations (notification, condition, run-script, write-item,");
    println!("  http-request, send-email, slack, twilio, etc.). Stored as");
    println!("  metadata; executable from anywhere; versioned with the schema.");
    println!();
    println!("EXTENSIONS");
    println!("  Drop a folder into extensions/ — it loads on next restart.");
    println!("  Types:");
    println!("    - interfaces  : custom field editors (Vue)");
    println!("    - displays    : custom read-only renderers");
    println!("    - layouts     : custom collection views");
    println!("    - modules     : custom admin app sections");
    println!("    - panels      : custom insights widgets");
    println!("    - hooks       : server-side lifecycle handlers (Node)");
    println!("    - endpoints   : custom REST routes (Node)");
    println!("    - operations  : custom Flow operations");
    println!("    - bundles     : multi-extension packages");
    println!();
    println!("MARKETPLACE");
    println!("  In-app Marketplace (Directus 10.10+) for installing community");
    println!("  extensions directly from the admin UI.");
}

fn cmd_license() {
    println!("Directus licensing");
    println!();
    println!("HISTORICAL");
    println!("  Directus 6 -> 9   GPLv3 (1.0 through 9.x).");
    println!();
    println!("CURRENT (Oct 2023 onward)");
    println!("  Directus 10+      Business Source License 1.1 (BUSL 1.1).");
    println!("                    Change Date: 4 years after each release.");
    println!("                    Change License: GPLv3.");
    println!("  Free for use, modification, self-hosting. Restriction: you");
    println!("  may not offer a commercial managed Directus service that");
    println!("  competes with Directus Cloud. Practical effect for ~99.9%");
    println!("  of users: zero, because they're not running a Directus");
    println!("  hosting business.");
    println!();
    println!("WHY THE PIVOT");
    println!("  Directus is venture-funded and felt the Elastic / Mongo /");
    println!("  HashiCorp pressure: hyperscalers cloning open-source projects");
    println!("  and running them as managed services without contributing back.");
    println!("  BUSL preserves source visibility + community use while");
    println!("  removing the perverse incentive.");
    println!();
    println!("ALTERNATIVE LICENSING");
    println!("  Commercial licenses available for SaaS providers who do want");
    println!("  to resell Directus-as-a-service. Contact sales@directus.io.");
}

fn cmd_cloud() {
    println!("Directus Cloud — official managed hosting");
    println!();
    println!("ARCHITECTURE");
    println!("  Multi-region (us-east-2, eu-central-1, ap-southeast-2 baseline).");
    println!("  Each project runs as an isolated Docker container with a");
    println!("  dedicated Postgres database. Files go to managed S3 with a");
    println!("  Cloudflare CDN in front.");
    println!();
    println!("PLANS (as of 2024)");
    println!("  Community    $15/mo project  — solo / small site");
    println!("  Professional $99/mo project  — production sites, daily backups");
    println!("  Enterprise   Custom         — HA database, custom domains, SSO,");
    println!("                                priority support, dedicated CSM");
    println!();
    println!("SELF-HOST");
    println!("  Container image: directus/directus on Docker Hub.");
    println!("  Run with a connection string to any supported database +");
    println!("  storage backend (local FS, S3, GCS, Azure Blob, Cloudinary).");
    println!("  Most operations users self-host on Fly.io, Railway, Render,");
    println!("  or DIY EC2/Hetzner.");
}

fn run_directus(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "database" => { cmd_database(); 0 }
        "app" => { cmd_app(); 0 }
        "api" => { cmd_api(); 0 }
        "flows" => { cmd_flows(); 0 }
        "license" => { cmd_license(); 0 }
        "cloud" => { cmd_cloud(); 0 }
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
        .unwrap_or_else(|| "directus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_directus(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/directus"), "directus");
        assert_eq!(basename("C:\\Tools\\directus.exe"), "directus.exe");
        assert_eq!(basename("directus"), "directus");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("directus.exe"), "directus");
        assert_eq!(strip_ext("directus"), "directus");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_directus(&["help".to_string()], "directus"), 0);
        let _ = run_directus(&[], "directus");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_directus(&["nope".to_string()], "directus"), 2);
    }
}
