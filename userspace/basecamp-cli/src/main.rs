#![deny(clippy::all)]

//! basecamp-cli — Slate OS Basecamp (37signals' opinionated PM, also where Rails was born)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: basecamp [OPTIONS]");
        println!("Basecamp (Slate OS) — 37signals opinionated project management + team comms");
        println!();
        println!("Options:");
        println!("  --plus                 Basecamp Plus — $15/user/mo (per-user pricing)");
        println!("  --pro-unlimited        Basecamp Pro Unlimited — $349/mo flat (unlimited users)");
        println!("  --hill-charts          Hill Charts (Shape Up methodology visualization)");
        println!("  --campfire             Campfire (group chat — separate one-time license)");
        println!("  --hey                  HEY email service (separate product, 37signals)");
        println!("  --once                 ONCE (37signals one-time-payment self-hosted apps)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Basecamp 4 (Slate OS)"); return 0; }
    println!("Basecamp 4 (Slate OS)");
    println!("  Vendor: 37signals LLC (Chicago, IL — private, founder-owned)");
    println!("  Founders: Jason Fried (CEO), David Heinemeier Hansson (CTO, 'DHH'), Ernest Kim, 2003");
    println!("          37signals started 1999 as a web design consultancy");
    println!("          built Basecamp as an internal PM tool, opened to public Feb 2004");
    println!("          while building Basecamp, DHH extracted Ruby on Rails (open-sourced Jul 2004)");
    println!("          'Getting Real' (2006), 'Rework' (2010), 'It Doesn't Have to Be Crazy at Work' (2018)");
    println!("          legendary opinionated tech-culture voice — anti-VC, pro-calm, work-fewer-hours");
    println!("          DHH famously moves the company from cloud → on-prem ('Leaving the cloud' 2023, Hey + Basecamp)");
    println!("  Founded: 2003 as Basecamp (the product), 1999 as 37signals (the company)");
    println!("          bootstrapped — Jeff Bezos personal investment 2006 only liquidity event");
    println!("          renamed Basecamp Inc. 2014, then back to 37signals 2022");
    println!("          ~80 employees, fully remote since founding");
    println!("          private + profitable + no funding pressure");
    println!("  Strategic position: 'opinionated PM for calm companies':");
    println!("                    primary competitor: Asana, Trello, ClickUp, Monday — none with the philosophy");
    println!("                    not chasing enterprise — explicitly anti-enterprise sales motion");
    println!("                    pitches against feature-creep: 'no, you don't need Gantt charts'");
    println!("                    sweet spot: small businesses, agencies, indie teams that share 37signals values");
    println!("                    influence outsized vs market share (~$80M+ ARR, 100K paying companies)");
    println!("  Pricing (radically simple):");
    println!("    Basecamp Plus — $15/user/mo (per-seat, like everyone else)");
    println!("    Basecamp Pro Unlimited — $349/mo flat (unlimited users — the famous flat rate)");
    println!("       includes 5TB storage, priority support, free first-year discount on annual");
    println!("    that's it. two prices. no 5-tier matrix. by design.");
    println!("    Campfire — separate $299 one-time self-hosted license (chat)");
    println!("    HEY email — separate $99/yr personal, $30/user/mo business");
    println!("    ONCE products — one-time payment, no subscription");
    println!("  Core architecture (intentionally minimal):");
    println!("    - Projects contain Tools (Message Board, To-dos, Schedule, Docs & Files, Campfire chat, Automatic Check-ins, Card Table)");
    println!("    - Every project has the same six tools by default");
    println!("    - HQ for company-wide announcements");
    println!("    - Teams for ongoing work without project boundaries");
    println!("    - Lineup (Pro): see all active projects in a single timeline view");
    println!("    - Hill Charts: visualize project status as 'climbing/cresting/descending'");
    println!("    - NO: Gantt charts, dependencies, time tracking, resource management, custom fields, custom workflows");
    println!("    - explicit feature absence is the product philosophy");
    println!("  Shape Up methodology (their PM philosophy):");
    println!("    - 6-week cycles + 2-week cooldown");
    println!("    - 'Bets' instead of tickets (small/medium pitches with clear appetite)");
    println!("    - 'Hill Charts' to visualize uncertainty resolving");
    println!("    - Fixed time + variable scope (vs Agile's fixed scope + variable time)");
    println!("    - Pitched as 'how 37signals actually builds Basecamp'");
    println!("    - free book at basecamp.com/shapeup");
    println!("    - influenced thousands of teams (often without using Basecamp itself)");
    println!("  Automatic Check-ins:");
    println!("    - Recurring questions to teams ('What did you work on today?', 'Wins this week?')");
    println!("    - Answers thread under each question — async standups");
    println!("    - Distinctive Basecamp feature — most competitors copied this");
    println!("  Card Table:");
    println!("    - Basecamp's Kanban board (added 2022 — DHH resisted for years)");
    println!("    - Triage / In Progress / On Hold / Done with cards");
    println!("    - Minimal swimlanes, no fancy automation");
    println!("  Campfire (chat):");
    println!("    - Group chat per-project, plus team-wide");
    println!("    - History permanently searchable");
    println!("    - Separate 'Campfire ONCE' product sold as self-hosted one-time license");
    println!("    - The original 'Campfire' was the 2006 web chat that inspired Slack");
    println!("  HEY (separate email product, 2020):");
    println!("    - Opinionated email re-imagined: Imbox/Feed/Paper Trail");
    println!("    - Screening senders (yes/no for new senders)");
    println!("    - Built with Rails 6 + Hotwire (showcase for Hotwire framework)");
    println!("    - Famous Apple App Store fight 2020 (DHH publicly battled Apple over 30% cut)");
    println!("  ONCE (2024, one-time-payment self-hosted apps):");
    println!("    - Campfire ONCE — $299 one-time, self-hosted chat");
    println!("    - Writebook ONCE — $199 one-time, self-hosted book writing");
    println!("    - 37signals' bet on 'post-SaaS' — pay once, own forever, run on your server");
    println!("  Mobile + native apps:");
    println!("    - iOS + Android + Windows + macOS native apps");
    println!("    - macOS app is a real native Mac app (not Electron)");
    println!("  Integrations: intentionally few (10-20)");
    println!("              email-in for to-dos, public-facing forwarding addresses for messages");
    println!("              Zapier integration as escape hatch");
    println!("              REST API + webhooks");
    println!("              37signals philosophy: minimize external dependencies");
    println!("  Customers: ~100,000 paying companies");
    println!("            indie agencies, small consultancies, family businesses, freelancer teams");
    println!("            no F1000 trophy customers — by design");
    println!("            avoids prominent customer logos pages (anti-marketing-theater)");
    println!("            sweet spot: 5-50 person small businesses + agencies who like 37signals' philosophy");
    println!("  Critique: feature absence is the product — frustrating if you need Gantt, dependencies, tracking");
    println!("           no time tracking, no resource management, no formal milestones");
    println!("           reporting basic (intentional)");
    println!("           customization minimal — workflows are Basecamp's way or no way");
    println!("           controversial 2021 'no political discussions at work' policy caused ~1/3 staff departure");
    println!("           niche by choice — Basecamp 4 (2024) still ~$80M ARR despite age");
    println!("  Differentiator: opinionated radical simplicity + flat unlimited-user pricing + 37signals philosophy/methodology + influence outsized vs scale — for teams who want the OPPOSITE of enterprise PM");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "basecamp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/basecamp"), "basecamp");
        assert_eq!(basename(r"C:\bin\basecamp.exe"), "basecamp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("basecamp.exe"), "basecamp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bc(&["--help".to_string()], "basecamp"), 0);
        assert_eq!(run_bc(&["-h".to_string()], "basecamp"), 0);
        let _ = run_bc(&["--version".to_string()], "basecamp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bc(&[], "basecamp");
    }
}
