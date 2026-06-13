#![deny(clippy::all)]

//! smartsheet-cli — SlateOS Smartsheet (NYSE:SMAR, spreadsheet-meets-PM)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ss(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: smartsheet [OPTIONS]");
        println!("Smartsheet (SlateOS) — spreadsheet-meets-project-management for enterprise");
        println!();
        println!("Options:");
        println!("  --pro                  Pro — $9/user/mo (annual)");
        println!("  --business             Business — $19/user/mo");
        println!("  --enterprise           Enterprise — custom ($35+/user/mo typical)");
        println!("  --advance              Advance — bundled enterprise edition (Control Center, Data Shuttle)");
        println!("  --control-center       Smartsheet Control Center (PPM at scale)");
        println!("  --data-shuttle         Data Shuttle (ETL between sheets)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Smartsheet 2024 (SlateOS)"); return 0; }
    println!("Smartsheet 2024 (SlateOS)");
    println!("  Vendor: Smartsheet Inc. (Bellevue, WA — NYSE:SMAR, taken private 2024)");
    println!("  Founders: Brent Frei, Mark Mader (CEO since 2006), John Creason, Maria Colacurcio, 2005");
    println!("          Mader was VP at Onyx Software, took CEO role early and is still CEO 19 years later");
    println!("          Frei went on to start Triberg Capital (private equity)");
    println!("          one of the most patient enterprise SaaS builds: 13 years private before IPO");
    println!("  Founded: 2005 in Bellevue, Washington");
    println!("          IPO Apr 2018 NYSE:SMAR at $15.40 (~$1.6B valuation)");
    println!("          peaked ~$90 in late 2021");
    println!("          dropped to $35-50 range 2022-2024");
    println!("          Vista Equity + Blackstone agreed to take Smartsheet PRIVATE Sep 2024 at $56.50/share (~$8.4B)");
    println!("          closing expected Q4 2024-Q1 2025");
    println!("          FY2024 revenue ~$960M (+26% YoY), still operating losses (but improving)");
    println!("          ~5,000 employees");
    println!("  Strategic position: 'enterprise PM that looks like a spreadsheet':");
    println!("                    primary competitor: Asana, Monday.com, Wrike, Adobe Workfront, Microsoft Project");
    println!("                    advantage: Excel-native interface = zero learning curve for analysts/PMs in F1000");
    println!("                    historically strong in: government, manufacturing, professional services, IT PMO");
    println!("                    pitch: 'configurable work platform for any team, any process'");
    println!("                    Vista take-private likely focused on tightening to enterprise-only + raising prices");
    println!("  Pricing (per user, annual prepay):");
    println!("    Pro — $9/user/mo (10 sheets, basic features)");
    println!("    Business — $19/user/mo (unlimited sheets, automations, integrations, proofing)");
    println!("    Enterprise — custom (SSO, SAML, advanced security, premium support)");
    println!("    Advance — custom (Control Center + Data Shuttle + premier connectors)");
    println!("    typically enterprise customers spend $50K-$2M+/yr on Smartsheet");
    println!("  Core architecture (spreadsheet-rooted):");
    println!("    - Sheets = projects/lists/whatever — grid of rows + columns");
    println!("    - Column types: text/number, contact, date, checkbox, dropdown, symbols, formula, predecessor, attachment");
    println!("    - Formulas (Excel-like syntax, 100+ functions)");
    println!("    - Cell linking + cross-sheet formulas");
    println!("    - Views: Grid, Gantt, Card (Kanban), Calendar, Timeline");
    println!("    - Reports (pull rows from many sheets into a single view)");
    println!("    - Dashboards (charts + widgets aggregating data from sheets/reports)");
    println!("  Automations (huge differentiator):");
    println!("    - Trigger-based: when row added/changed/date hit → action");
    println!("    - Actions: notify user, send approval request, change cell, lock row, copy/move row, request update");
    println!("    - Approval workflows (sequential, parallel, conditional)");
    println!("    - Recurring automations (daily/weekly snapshots, status pings)");
    println!("    - Bridge by Smartsheet (no-code automation builder)");
    println!("  Smartsheet Control Center (the enterprise PPM extension):");
    println!("    - Manage many similar projects from one blueprint");
    println!("    - Bulk-create projects, roll up portfolios");
    println!("    - Used by: F1000 PMOs, NASA, Cisco, BMW for portfolio management");
    println!("    - This is where Smartsheet wins vs Asana/Monday — enterprise PPM");
    println!("  Data Shuttle:");
    println!("    - ETL between Smartsheet + Excel, CSV, Salesforce, Google Sheets");
    println!("    - Scheduled offload/upload of data");
    println!("    - Common pattern: nightly Salesforce → Smartsheet sync, Smartsheet → Snowflake export");
    println!("  Resource Management (acquired 10,000ft 2019):");
    println!("    - Capacity planning + utilization forecasts");
    println!("    - Skills matching for project assignments");
    println!("    - Reporting on billable utilization");
    println!("    - Add-on, not included in Business tier");
    println!("  Brandfolder (acquired Sep 2020 for ~$155M):");
    println!("    - DAM (Digital Asset Management) for brand assets");
    println!("    - Adds 'creative + work' integrated workflow vs Wrike/Workfront");
    println!("    - Separate SKU from core Smartsheet");
    println!("  Proofing (acquired in Brandfolder + native):");
    println!("    - Annotate + approve creative files inside Smartsheet");
    println!("    - Versioning + approval status tracked on cell");
    println!("  Smartsheet AI:");
    println!("    - Generate formulas + functions from natural language");
    println!("    - Auto-summarize project status");
    println!("    - 2023-2024 push but lagged behind Asana Intelligence + ClickUp AI");
    println!("  Integrations: 100+ direct + Zapier/Make + iPaaS");
    println!("              Salesforce, Jira (deep — bi-directional sync), Adobe Creative Cloud");
    println!("              Microsoft Teams + 365 + Power BI (deep)");
    println!("              Slack, Google Workspace, DocuSign, Tableau, Snowflake");
    println!("              REST API + JSON + webhooks + Smartsheet API SDKs (Java, Python, .NET, Node, Ruby, C#)");
    println!("  Customers: ~14,000 enterprise customers (>$5K ACV), >85% of Fortune 100 use it somewhere");
    println!("            NASA, Cisco, Pfizer, P&G, BMW, Comcast, McGraw-Hill, US Navy, US Army");
    println!("            Roche, Hilton Hotels, Sodexo, ABB, Carlsberg");
    println!("            sweet spot: enterprise PMO + IT + construction + manufacturing + professional services");
    println!("            historically WEAK in: dev teams (Jira wins), tiny SMBs (Asana/Trello win)");
    println!("  Critique: UI shows spreadsheet roots — modern teams find it dated vs Linear/Monday");
    println!("           grid-centric model limits power for non-grid workflows (whiteboarding, docs)");
    println!("           AI features behind Asana Intelligence + ClickUp AI in coverage");
    println!("           heavy enterprise sales motion: 6-9 month deal cycles, high CAC");
    println!("           per-user pricing means usage caps at scale (Asana/Monday more aggressive on volume discounts)");
    println!("           Vista take-private: typical PE concerns about pricing increases + R&D cuts");
    println!("           mobile experience weaker than desktop");
    println!("  Differentiator: most spreadsheet-native enterprise PM platform + Control Center PPM + Excel power users feel at home — for F1000 PMOs running 100+ projects in parallel");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "smartsheet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ss(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ss};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/smartsheet"), "smartsheet");
        assert_eq!(basename(r"C:\bin\smartsheet.exe"), "smartsheet.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("smartsheet.exe"), "smartsheet");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ss(&["--help".to_string()], "smartsheet"), 0);
        assert_eq!(run_ss(&["-h".to_string()], "smartsheet"), 0);
        let _ = run_ss(&["--version".to_string()], "smartsheet");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ss(&[], "smartsheet");
    }
}
