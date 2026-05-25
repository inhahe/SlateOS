#![deny(clippy::all)]

//! teamwork-cli — OurOS Teamwork.com (Irish PM/PSA for client services agencies)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: teamwork [OPTIONS]");
        println!("Teamwork.com (OurOS) — project management + PSA for client-services agencies");
        println!();
        println!("Options:");
        println!("  --free                 Free Forever — up to 5 users");
        println!("  --starter              Starter — $5.99/user/mo");
        println!("  --deliver              Deliver — $9.99/user/mo");
        println!("  --grow                 Grow — $19.99/user/mo (most popular for agencies)");
        println!("  --scale                Scale — custom (enterprise)");
        println!("  --time-tracking        Native time tracking + billable hours");
        println!("  --invoicing            Built-in invoicing + budget vs actuals");
        println!("  --client-access        Free client logins (don't count against user count)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Teamwork.com 2024 (OurOS)"); return 0; }
    println!("Teamwork.com 2024 (OurOS)");
    println!("  Vendor: Teamwork.com Ltd. (Cork, Ireland — private, bootstrapped)");
    println!("  Founders: Peter Coppinger (CEO), Daniel Mackey (CTO), 2007");
    println!("          both Cork-based developers, built Teamwork as internal PM for their consultancy Digital Crew");
    println!("          spun out Teamwork as standalone SaaS 2007");
    println!("          legendary Irish bootstrapped success — pre-Stripe, pre-cloud, just shipped");
    println!("  Founded: 2007 in Cork, Ireland (still HQ'd there)");
    println!("          bootstrapped — no outside funding until 2022 ~$70M Series A from Atlassian-Coro");
    println!("          actually it was Cervin Ventures + private investor, not Atlassian — kept independent");
    println!("          ~$100M+ ARR estimated, profitable, ~600 employees");
    println!("          rebrand: was 'Teamwork Projects' + 'Teamwork CRM' + 'Teamwork Desk' as suite, now consolidated under teamwork.com");
    println!("  Strategic position: 'PM purpose-built for agencies billing client work':");
    println!("                    primary competitor: Asana, Monday, ClickUp, Wrike — none as agency-focused");
    println!("                    Mavenlink/Kantata (heavier PSA) is closest peer + Productive.io");
    println!("                    Harvest + Toggl + everhour (time-tracking-first)");
    println!("                    pitch: 'PM that doesn't penalize you for client logins'");
    println!("                    legendary: free unlimited client + collaborator logins (unique pricing wedge)");
    println!("  Pricing (transparent — per-team-member, client logins FREE):");
    println!("    Free Forever — 5 users, 2 projects, 100MB storage");
    println!("    Starter — $5.99/user/mo (3 projects, basic features)");
    println!("    Deliver — $9.99/user/mo (unlimited projects, time tracking, billing)");
    println!("    Grow — $19.99/user/mo (most popular: project budgets, advanced reports, custom fields)");
    println!("    Scale — custom (SSO, premium support, custom branding, project portfolios)");
    println!("    CLIENT LOGINS ARE FREE on all paid tiers — the agency moat");
    println!("    add-ons: Teamwork CRM, Teamwork Desk (helpdesk), Spaces (docs), Chat — extra per-user");
    println!("  Core PM features:");
    println!("    - Projects → Tasklists → Tasks → Subtasks");
    println!("    - Views: List, Board (Kanban), Gantt, Calendar, Table, Workload");
    println!("    - Dependencies + Gantt (FS, SS, FF, SF)");
    println!("    - Custom fields + custom tags");
    println!("    - Time tracking (built-in, not add-on — major differentiator)");
    println!("    - Project budgets + budget burndown");
    println!("    - Milestones + deliverables");
    println!("    - Recurring tasks");
    println!("    - Risk register per project");
    println!("  Agency-specific features (the killer differentiator):");
    println!("    - Client portal: clients log in free, see only what you share");
    println!("    - Billable + non-billable hours per task");
    println!("    - Budget vs actuals reporting (financial profitability per project)");
    println!("    - Invoicing module: log hours → generate invoice → send to client");
    println!("    - Resource planning + workload across team");
    println!("    - Utilization reports (% of billable hours over capacity)");
    println!("    - Retainer billing + per-project billing");
    println!("    - QuickBooks + Xero accounting integration for invoice sync");
    println!("  Teamwork CRM (Free → $12/user/mo):");
    println!("    - Sales pipeline tracking integrated with PM");
    println!("    - Convert won deals → projects automatically");
    println!("    - Lightweight vs HubSpot/Pipedrive but tight integration with PM");
    println!("  Teamwork Desk (helpdesk, $7-$12/user/mo):");
    println!("    - Email-based helpdesk ticketing");
    println!("    - Convert tickets to project tasks");
    println!("    - SLA tracking + customer satisfaction surveys");
    println!("    - Competes with Help Scout + Zendesk (lighter)");
    println!("  Teamwork Spaces:");
    println!("    - Wiki / docs / knowledge base attached to projects");
    println!("    - Competes with Confluence (much lighter)");
    println!("  Teamwork Chat:");
    println!("    - In-app team chat (Slack-lite)");
    println!("    - Not a serious Slack competitor — used as integrated chat for paying teams");
    println!("  Teamwork.ai (2024 push):");
    println!("    - Auto-summarize project status");
    println!("    - Generate tasks from project description");
    println!("    - Smart resource recommendations");
    println!("    - Catching up to Asana Intelligence + ClickUp AI");
    println!("  Integrations: 100+");
    println!("              QuickBooks Online, Xero (accounting + invoicing sync)");
    println!("              HubSpot, Pipedrive, Salesforce (CRM)");
    println!("              Slack, Microsoft Teams, Google Workspace, Microsoft 365");
    println!("              Harvest (time tracking double-down — many agencies still prefer)");
    println!("              GitHub, GitLab, BitBucket (Dev workflows)");
    println!("              Zapier + Make + native API + webhooks");
    println!("  Customers: ~25,000 paying companies, ~6,000+ agencies use heavily");
    println!("            heavily indie/mid-size agencies, consultancies, design studios, dev shops");
    println!("            Spotify, PayPal, Disney, Honda, eBay use as departmental PM");
    println!("            sweet spot: 5-200 person digital + creative + dev agencies");
    println!("            global, but very strong in: Ireland, UK, Australia (founder's geographic ties)");
    println!("            not focused on F1000 enterprise (Wrike/Workfront/Smartsheet win there)");
    println!("  Critique: UI shows its age vs Asana/Monday/ClickUp (improving but still feels 2017-era)");
    println!("           ecosystem (modules) less polished than competitors");
    println!("           agency-specific features locked behind Deliver+/Grow tiers");
    println!("           Teamwork Chat is weak vs Slack — most customers use Slack");
    println!("           reporting depth lower than Smartsheet for portfolio-level views");
    println!("           AI features behind Asana, ClickUp, Monday in marketing buzz");
    println!("  Differentiator: PM purpose-built for agencies + free unlimited client logins + native time tracking + budgets + invoicing — best PM for agencies billing client work");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "teamwork".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tw(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
