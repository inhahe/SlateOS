#![deny(clippy::all)]

//! airtable-cli — OurOS Airtable spreadsheet-database hybrid
//!
//! Single personality: `airtable`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_at(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: airtable [OPTIONS]");
        println!("Airtable (OurOS) — Spreadsheet-database hybrid platform");
        println!();
        println!("Options:");
        println!("  --base NAME            Open base (collection of tables)");
        println!("  --view TYPE            grid/calendar/gallery/kanban/timeline/gantt/form");
        println!("  --automation           Airtable Automations (workflow triggers/actions)");
        println!("  --interface            Interface Designer (build apps on bases)");
        println!("  --plan PLAN            free/team/business/enterprise-scale");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Airtable Desktop 2.10.0 (OurOS)"); return 0; }
    println!("Airtable Desktop 2.10.0 (OurOS)");
    println!("  Vendor: Formagrid Inc. dba Airtable (San Francisco, founded 2012)");
    println!("  Founders: Howie Liu, Andrew Ofstad, Emmett Nicholas");
    println!("  Concept: low-code database that looks like a spreadsheet");
    println!("  Field types: text, number, attachment, link to record, formula, rollup,");
    println!("               lookup, count, date, checkbox, select, user, barcode, button");
    println!("  Views: grid, calendar, gallery, kanban, timeline, gantt, form (separate per view)");
    println!("  Free: 1,000 records/base, 1GB attachments — entry tier");
    println!("  Team: $20/user/mo — 50K records/base, 20GB, Gantt/timeline views");
    println!("  Business: $45/user/mo — 125K records, 100GB, admin panel, SSO");
    println!("  Enterprise Scale: custom — 500K records, 1TB, audit logs, EKM");
    println!("  Cobuilder: AI-assisted app building (Airtable AI suite)");
    println!("  API: REST + JS client, automations, webhooks, sync from external sources");
    println!("  Integrations: Slack, Salesforce, Zapier, Make, native sync (50+ sources)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "airtable".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_at(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_at};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/airtable"), "airtable");
        assert_eq!(basename(r"C:\bin\airtable.exe"), "airtable.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("airtable.exe"), "airtable");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_at(&["--help".to_string()], "airtable"), 0);
        assert_eq!(run_at(&["-h".to_string()], "airtable"), 0);
        let _ = run_at(&["--version".to_string()], "airtable");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_at(&[], "airtable");
    }
}
