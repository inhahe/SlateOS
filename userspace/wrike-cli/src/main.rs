#![deny(clippy::all)]

//! wrike-cli — SlateOS Wrike (Citrix-owned enterprise work management)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wrike(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wrike [OPTIONS]");
        println!("Wrike (Slate OS) — enterprise collaborative work management (Citrix-owned)");
        println!();
        println!("Options:");
        println!("  --free                 Free — basic task management for small teams");
        println!("  --team                 Team — $9.80/user/mo");
        println!("  --business             Business — $24.80/user/mo (most popular)");
        println!("  --enterprise           Enterprise — custom");
        println!("  --pinnacle             Pinnacle — custom (advanced analytics, dependencies, BI)");
        println!("  --marketing            Wrike for Marketing (PSA-style add-on)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Wrike 2024 (Slate OS)"); return 0; }
    println!("Wrike 2024 (Slate OS)");
    println!("  Vendor: Wrike, Inc. (San Jose, CA — owned by Citrix Systems, Symphony Tech)");
    println!("  Founders: Andrew Filev (CEO), 2006");
    println!("          Filev started Wrike as a side project running another company (Imobix consultancy)");
    println!("          one of earliest 'work management' (vs PM-only) positioned tools");
    println!("  Founded: 2006 in California");
    println!("          raised modest venture (~$26M) over years");
    println!("          acquired by Vista Equity Partners Jan 2019 for $800M");
    println!("          re-acquired by Citrix Feb 2021 for $2.25B (Vista 3x return)");
    println!("          Citrix taken private Sep 2022 by Vista + Elliott (so Wrike now under Symphony Tech Group)");
    println!("          ~30K customers, ~3M+ users, est. ~$200M ARR before Citrix");
    println!("  Strategic position: 'enterprise work management for marketing + professional services + IT teams':");
    println!("                    primary competitor: Asana, Monday.com, Smartsheet, ClickUp, Workfront (Adobe-owned)");
    println!("                    Wrike's wedge: enterprise depth (custom workflows, approvals, request forms) + marketing-specific features");
    println!("                    pitch shifted post-Citrix: 'work management for the digital workforce' (more enterprise IT angle)");
    println!("                    Adobe's Workfront is closest enterprise-positioned competitor");
    println!("  Pricing (per user, transparent for SMB tiers):");
    println!("    Free — 2-5 users, basic task list");
    println!("    Team — $9.80/user/mo (custom fields, Gantt, time tracking)");
    println!("    Business — $24.80/user/mo (most popular; project portfolios, request forms, approvals)");
    println!("    Enterprise — custom (SSO, 2FA, audit, advanced security)");
    println!("    Pinnacle — custom (most expensive — adds AI workload, advanced analytics, BI)");
    println!("    Wrike for Marketing — typically Enterprise+ tier with PSA add-ons");
    println!("    Wrike for Professional Services — separate edition");
    println!("  Core features (work management depth):");
    println!("    - Folders → Projects → Tasks → Subtasks (4-level hierarchy)");
    println!("    - Custom fields, custom statuses, custom workflows");
    println!("    - Views: List, Board (Kanban), Gantt, Table, Calendar, Workload, Dashboard");
    println!("    - Dependencies (start-to-start, finish-to-finish, FS/SS lag)");
    println!("    - Resource management + workload heatmap");
    println!("    - Time tracking + timesheets + approvals");
    println!("    - Request forms (dynamic forms → auto-create tasks with assigned workflow)");
    println!("    - Approvals (formal task/file review-and-sign-off cycles)");
    println!("    - Proofing tool for image/video review (annotation directly on creative)");
    println!("  Wrike for Marketing (the marketing PSA differentiator):");
    println!("    - Adobe Creative Cloud integration (proof directly from Photoshop/Illustrator)");
    println!("    - Brand asset management (DAM-lite)");
    println!("    - Marketing performance dashboards");
    println!("    - Campaign templates + content calendar");
    println!("    - Direct competitor: Asana for marketing, Adobe Workfront");
    println!("  Wrike for Professional Services:");
    println!("    - Billable hours + project profitability");
    println!("    - Project budgets + actuals + invoicing data");
    println!("    - PSA-lite (not Kantata/Mavenlink/FinancialForce-deep but covers basics)");
    println!("  Wrike AI / Work Intelligence:");
    println!("    - Smart Replies (Slack-style AI suggestions in comments)");
    println!("    - Smart Search (semantic search across all spaces)");
    println!("    - Risk Prediction (forecast project slip likelihood)");
    println!("    - Document AI (summarize attached files)");
    println!("    - launched 2023-2024, behind Asana Intelligence in feature breadth");
    println!("  Integrations: 400+");
    println!("              Salesforce (deep, especially for PS teams)");
    println!("              Adobe Creative Cloud (Photoshop, Premiere, InDesign, XD)");
    println!("              Slack, Microsoft Teams (deep, including Wrike for Teams add-in)");
    println!("              Google Workspace (Drive, Calendar, Gmail)");
    println!("              Microsoft 365 (Outlook, OneDrive, SharePoint)");
    println!("              Citrix Workspace (deeper post-acquisition)");
    println!("              Jira, GitHub, GitLab (Dev integrations)");
    println!("              Zapier + Make + native API + webhooks + custom Wrike Integrate iPaaS");
    println!("  Mobile + offline: iOS + Android apps, basic offline access");
    println!("  Customers: ~30K+ paying customers, ~3M users");
    println!("            Hawaiian Airlines, Stanley Black & Decker, Tiffany & Co, Estée Lauder, L'Oreal");
    println!("            Hootsuite, Sony Pictures TV, Lyft (some teams), Siemens, Verizon (departments)");
    println!("            sweet spot: 100-5,000-employee enterprises with marketing, creative, or PS workflows");
    println!("            historically strong: marketing teams in B2C brands");
    println!("            weaker: dev teams (Jira/Linear win), tiny SMBs (Asana/ClickUp win)");
    println!("  Critique: UI complexity — Wrike has more features than most competitors but at the cost of intuitive UX");
    println!("           learning curve significantly steeper than Asana or Trello (especially folder/project model)");
    println!("           pricing opaque beyond Team/Business tiers — enterprise quotes vary wildly");
    println!("           Pinnacle tier perception: 'we keep features for highest tier as enterprise pressure tactic'");
    println!("           AI features lag Asana + ClickUp in marketing buzz, real depth catching up");
    println!("           Citrix → Vista → Symphony ownership churn raises long-term roadmap concerns");
    println!("           mobile app weaker than Asana, ClickUp, Monday");
    println!("  Differentiator: deepest enterprise customization (folders/spaces/workflows/forms/approvals) + marketing-specific PSA features + Adobe Creative Cloud integration — for marketing + creative + PS teams in enterprises");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wrike".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wrike(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wrike};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wrike"), "wrike");
        assert_eq!(basename(r"C:\bin\wrike.exe"), "wrike.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wrike.exe"), "wrike");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wrike(&["--help".to_string()], "wrike"), 0);
        assert_eq!(run_wrike(&["-h".to_string()], "wrike"), 0);
        let _ = run_wrike(&["--version".to_string()], "wrike");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wrike(&[], "wrike");
    }
}
