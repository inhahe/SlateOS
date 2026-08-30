#![deny(clippy::all)]

//! hive-cli — Slate OS Hive (NYC democratic-input PM platform)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hive(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hive [OPTIONS]");
        println!("Hive (Slate OS) — project management built by user votes (democratic product development)");
        println!();
        println!("Options:");
        println!("  --free                 Hive Solo — free for individuals");
        println!("  --teams                Hive Teams — $5/user/mo");
        println!("  --teams-plus           Hive Teams Plus — $12/user/mo");
        println!("  --enterprise           Hive Enterprise — custom");
        println!("  --analytics            Goals + Analytics (HiveAnalytics add-on)");
        println!("  --time-tracking        Native time tracking");
        println!("  --hivemind             Hivemind AI (auto-suggest + automate)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Hive 2024 (Slate OS)"); return 0; }
    println!("Hive 2024 (Slate OS)");
    println!("  Vendor: Hive Technology, Inc. (New York City — private)");
    println!("  Founders: John Furneaux (CEO), Eric Typaldos, 2015");
    println!("          Furneaux ex-Saba Software, ex-Eloqua exec");
    println!("          first hires were ex-WeWork people in NYC");
    println!("          unusual: 'built by users' product development philosophy from day one");
    println!("  Founded: 2015 in New York City");
    println!("          raised ~$30M total venture (Andreessen Horowitz, Resolute Ventures)");
    println!("          ~$50M ARR estimate (private, not disclosed)");
    println!("          ~150 employees, NYC + remote");
    println!("  Strategic position: 'democratically-built PM for teams of teams':");
    println!("                    primary competitor: Asana, Monday.com, ClickUp, Trello, Basecamp");
    println!("                    Hive's wedge: rapid feature shipping based on user votes (every feature went through 'Hive.com/forum')");
    println!("                    'productivity platform' positioning + chat + meeting notes + time tracking + analytics in one");
    println!("                    customer base skews to media + creative agencies + NYC-area companies");
    println!("                    much smaller than Monday/Asana but fervent fans");
    println!("  Pricing (transparent — per user, annual):");
    println!("    Hive Solo — free (limited projects, 1 user)");
    println!("    Hive Teams — $5/user/mo (paid annually, ~$7/mo monthly)");
    println!("    Hive Teams Plus — $12/user/mo (timesheets, advanced views, custom fields)");
    println!("    Hive Enterprise — custom (SSO, security, 24/7 support)");
    println!("    add-ons: HiveAnalytics ($5/user/mo), HiveTime (timesheets), HiveAutomate (workflow automation)");
    println!("  Core features:");
    println!("    - 'Actions' (Hive's word for tasks) inside projects");
    println!("    - 6 views: Table, Kanban, Gantt, Calendar, Portfolio, Summary, Team");
    println!("    - Custom statuses, custom fields, subactions");
    println!("    - Templates for repeated workflows");
    println!("    - Native time tracking (built-in)");
    println!("    - Resource management + workload");
    println!("    - Recurring actions");
    println!("    - Dependencies + Gantt scheduling");
    println!("  Hive built-ins (the 'all in one' pitch):");
    println!("    - Hive Mail (email integration — turn emails into actions)");
    println!("    - Hive Chat (in-app chat tied to actions)");
    println!("    - Hive Notes (meeting notes + action items, AI-assisted)");
    println!("    - Hive Forms (form builder → auto-create actions)");
    println!("    - Hive Files (file storage + Adobe Creative Cloud + Loom integration)");
    println!("    - Goals + OKRs tracking");
    println!("  HiveAnalytics:");
    println!("    - Productivity dashboards: who's done what, project health, workload distribution");
    println!("    - Goal tracking with progress bars");
    println!("    - Custom reports + saved views");
    println!("    - Add-on, not in core Teams tier");
    println!("  Hivemind (AI features, 2023+):");
    println!("    - Generate action items from meeting notes");
    println!("    - Auto-summarize project status");
    println!("    - Suggest next steps based on project type");
    println!("    - Image generation for project marketing artifacts");
    println!("    - Powered by mix of OpenAI + custom models");
    println!("  HiveAutomate (no-code workflows):");
    println!("    - When-then-rule automations across Hive");
    println!("    - Trigger: action created, status changed, due date, etc.");
    println!("    - Action: assign, notify, change status, post to chat, send email");
    println!("    - Cross-tool: trigger Slack message, send Gmail, post Webhook");
    println!("  Democratic product development (the philosophy):");
    println!("    - hive.com/forum — public roadmap voting");
    println!("    - Every feature decided + ranked by user votes");
    println!("    - Roadmap published as live document");
    println!("    - Marketing slogan: 'the world's first democratically-built productivity platform'");
    println!("  Integrations: 1,000+ (via Zapier + direct)");
    println!("              Slack, Microsoft Teams, Gmail, Outlook, Google Workspace");
    println!("              Zoom, Salesforce, HubSpot");
    println!("              Adobe Creative Cloud, Loom, Figma");
    println!("              GitHub, GitLab");
    println!("              Zapier + Make + native API + webhooks");
    println!("  Customers: ~7,000 paying customers");
    println!("            Google (some teams), Starbucks (departments), Toyota (regional), Comcast");
    println!("            Anheuser-Busch, Electrolux, IBM (departments), Booz Allen Hamilton");
    println!("            NYU, Vox Media, Conde Nast (media-heavy customer base)");
    println!("            sweet spot: 50-500 person media + creative + consulting orgs in NYC area");
    println!("  Critique: smaller ecosystem than Asana/Monday/ClickUp");
    println!("           brand recognition outside NYC/media verticals is limited");
    println!("           UI is dense — lots of features visible, can feel overwhelming");
    println!("           features-per-tier model confusing (HiveAnalytics + HiveTime + HiveAutomate all separate add-ons)");
    println!("           AI features mid-pack vs Asana/ClickUp marketing");
    println!("           growth slowed vs competitors 2023-2024");
    println!("           pricing aggressive at entry ($5) but add-ons stack up quickly");
    println!("  Differentiator: built-by-user-votes democratic product dev + 'everything in one app' (chat + notes + email + actions) + NYC media verticals — for teams who want one tool not five");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hive".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hive(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hive};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hive"), "hive");
        assert_eq!(basename(r"C:\bin\hive.exe"), "hive.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hive.exe"), "hive");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hive(&["--help".to_string()], "hive"), 0);
        assert_eq!(run_hive(&["-h".to_string()], "hive"), 0);
        let _ = run_hive(&["--version".to_string()], "hive");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hive(&[], "hive");
    }
}
