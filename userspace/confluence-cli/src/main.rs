#![deny(clippy::all)]

//! confluence-cli — OurOS Confluence (Atlassian wiki + docs, NASDAQ:TEAM)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_conf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: confluence [OPTIONS]");
        println!("Confluence (OurOS) — Atlassian team workspace + wiki + docs (NASDAQ:TEAM)");
        println!();
        println!("Options:");
        println!("  --free                 Free — up to 10 users");
        println!("  --standard             Standard — $6.05/user/mo");
        println!("  --premium              Premium — $11.55/user/mo (analytics + automations)");
        println!("  --enterprise           Enterprise — custom (SAML, audit logs, residency)");
        println!("  --data-center          Data Center (self-hosted, $42K+/yr for 500 users)");
        println!("  --whiteboards          Confluence Whiteboards (Miro-like, 2023+)");
        println!("  --databases            Confluence Databases (Notion-like structured data, 2024)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Confluence 2024 (OurOS)"); return 0; }
    println!("Confluence 2024 (OurOS)");
    println!("  Vendor: Atlassian Corporation Plc (Sydney, Australia / SF, CA — NASDAQ:TEAM)");
    println!("  Founders: Mike Cannon-Brookes + Scott Farquhar, 2002 (Atlassian); Confluence shipped 2004");
    println!("          University of New South Wales graduates, started Atlassian on $10K credit card debt");
    println!("          Cannon-Brookes co-CEO + Farquhar co-CEO until Farquhar stepped down Aug 2024");
    println!("          billionaire founders, prominent in Australian tech + climate activism");
    println!("          built Atlassian without venture funding until 2010 (Accel growth equity)");
    println!("  Founded: 2004 (Confluence product), Atlassian 2002");
    println!("          Atlassian IPO Dec 2015 NASDAQ:TEAM at $21 (~$5.8B valuation)");
    println!("          peaked ~$480 late 2021, dropped to $130-200 range");
    println!("          Atlassian total FY2024 revenue ~$4.36B (+22% YoY)");
    println!("          Confluence: ~85K+ paying customers, ~$1B+ ARR estimated");
    println!("          ~12,000 employees Atlassian-wide");
    println!("  Strategic position: 'enterprise team workspace, often paired with Jira':");
    println!("                    primary competitor: Notion (newer + flashier), Microsoft Loop, Coda, Slab");
    println!("                    wiki competitor: SharePoint, GitBook, BookStack, Obsidian (personal)");
    println!("                    Notion has been eating Confluence for ~5 years — Confluence response: Whiteboards, Databases, Rovo AI");
    println!("                    legacy strength: deep Jira integration (Jira + Confluence = Atlassian PM stack)");
    println!("                    Data Center (self-hosted) retired Cloud is replacement for Server (EOL 2024)");
    println!("                    'Atlassian Cloud' migration is the big company push 2020-2025");
    println!("  Pricing (per user, transparent):");
    println!("    Free — 10 users, 2GB storage");
    println!("    Standard — $6.05/user/mo (up to 50K users, unlimited storage)");
    println!("    Premium — $11.55/user/mo (analytics, automations, archive, sandbox, premium support)");
    println!("    Enterprise — custom (SSO, SCIM, audit, data residency, 24/7 support)");
    println!("    Data Center — annual license: ~$42K/yr starting at 500 users, scales up");
    println!("    bundled with Atlassian Premium suite (Jira + Confluence + Bitbucket) — typical enterprise deals $100K-$2M+/yr");
    println!("  Core architecture (page-based wiki + structured editor):");
    println!("    - Spaces (top-level workspaces, e.g. Engineering, Marketing, Personal)");
    println!("    - Pages (hierarchical tree, infinite nesting)");
    println!("    - Editor: rich text (paragraphs, headings, tables, lists, code blocks, callouts)");
    println!("    - Macros: extensible inline widgets (Jira issue list, status, decision, date, mentions, Loom embed)");
    println!("    - Templates: page templates per use case (meeting notes, OKRs, decisions, PRDs)");
    println!("    - Comments: page-level + inline (highlight text + comment, like Google Docs)");
    println!("    - Version history + restore");
    println!("    - REST API + atlassian-document-format JSON storage");
    println!("  Jira integration (the moat):");
    println!("    - Embed live Jira issues in Confluence pages");
    println!("    - Auto-generate roadmap pages from Jira filters");
    println!("    - Create Jira issue from highlighted Confluence text");
    println!("    - Decisions tracked as Jira-linked items");
    println!("    - Most Atlassian customers buy both → 'Atlassian Tax' lock-in");
    println!("  Confluence Whiteboards (2023 GA):");
    println!("    - Embedded whiteboards inside pages");
    println!("    - Direct competitor: Miro, Mural, FigJam, Loom Boards");
    println!("    - Atlassian's response to Notion's free-form canvas + Miro's category leadership");
    println!("    - Convert sticky notes → Jira issues directly");
    println!("  Confluence Databases (2024):");
    println!("    - Notion-like structured tables embedded in pages");
    println!("    - Custom field types: text, number, date, select, person, link to Jira issue");
    println!("    - Multiple views (table, board, list) of same data");
    println!("    - Atlassian's direct response to Notion eating mid-market");
    println!("  Atlassian Intelligence + Rovo (2023-2024 AI push):");
    println!("    - Auto-summarize long pages");
    println!("    - Generate content from prompts");
    println!("    - Smart search across Confluence + Jira + connected apps (Slack, GitHub, Drive)");
    println!("    - 'Rovo Agents' — autonomous agents for tasks (Q4 2024)");
    println!("    - Atlassian's $1B+ AI bet vs Notion AI + Microsoft Copilot");
    println!("  Marketplace (Atlassian Marketplace):");
    println!("    - 5,500+ apps on Marketplace");
    println!("    - Top-grossing third-party apps include diagramming (Gliffy, draw.io)");
    println!("    - Comindware, Scroll Apps, Tempo (time tracking — used to be top earner)");
    println!("    - Atlassian's API/Forge platform for custom apps");
    println!("  Mobile + offline: iOS + Android apps, limited offline support");
    println!("  Customers: ~85,000+ paying customers");
    println!("            Spotify, NASA, Visa, Bayer, Verizon, eBay, Adobe (yes, uses Confluence)");
    println!("            DocuSign, Robinhood, Twilio, Square, AirBnB (some teams)");
    println!("            sweet spot: 100-100,000 employee enterprises, especially with engineering teams using Jira");
    println!("            historically WEAK in: marketing teams (Notion + Asana win), non-tech departments");
    println!("  Critique: Notion has been eating mid-market for years — younger teams prefer Notion's modern UX");
    println!("           Confluence's editor is slower + less polished than Notion (still improving)");
    println!("           Cloud migration painful for legacy on-prem customers — many delays, breakages");
    println!("           Atlassian Server EOL Feb 2024 forced customers to Cloud OR Data Center (expensive)");
    println!("           Marketplace app ecosystem has compatibility issues between Server/DC/Cloud editions");
    println!("           AI features behind Notion AI in adoption + behind Microsoft Copilot in enterprise");
    println!("           perceived as 'enterprise stodgy' vs Notion's design-driven appeal");
    println!("           cost adds up: Confluence + Jira + Bitbucket + Loom + Trello + Marketplace apps = $$$");
    println!("  Differentiator: deepest Jira integration + 85K+ enterprise install base + Whiteboards + Databases + Rovo AI — for engineering-led enterprises that already live in Atlassian");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "confluence".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_conf(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_conf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/confluence"), "confluence");
        assert_eq!(basename(r"C:\bin\confluence.exe"), "confluence.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("confluence.exe"), "confluence");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_conf(&["--help".to_string()], "confluence"), 0);
        assert_eq!(run_conf(&["-h".to_string()], "confluence"), 0);
        assert_eq!(run_conf(&["--version".to_string()], "confluence"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_conf(&[], "confluence"), 0);
    }
}
