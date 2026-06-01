#![deny(clippy::all)]
//! hiver-cli — personality CLI for Hiver, the help desk that lives
//! entirely inside Gmail.
//!
//! Founded 2011 by Niraj Ranjan Rout in Bengaluru, India as a Gmail
//! browser extension that turns a shared Gmail label into a collaborative
//! support inbox — no separate help-desk app to log into, no separate
//! ticket UI to learn. The defining design choice: meet users where they
//! already live (Google Workspace), don't ask them to context-switch into
//! Zendesk. $24M Series C 2022 led by K1 Capital + Kalaari Capital. Strong
//! Google Workspace channel position; many customers come via Google
//! Workspace Marketplace search rather than direct top-of-funnel.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Hiver Gmail-native shared-inbox personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Niraj Ranjan Rout 2011 Bengaluru; Gmail-extension origin");
    println!("    workspace     Lives inside Gmail / Google Workspace");
    println!("    inbox         Shared label inbox + assignment + SLAs");
    println!("    automation    Workflow rules + auto-assignment + canned responses");
    println!("    channels      Email + live chat + WhatsApp + voice extensions");
    println!("    analytics     Reporting + CSAT + workload metrics");
    println!("    pricing       Per-user-per-month tiered pricing");
    println!("    customers     Google Workspace SMB + mid-market customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("hiver-cli 0.1.0 (gmail-native-shared-inbox personality build)"); }

fn run_about() {
    println!("Hiver (GrexIt, Inc.).");
    println!("  Founded:    2011, Bengaluru, India (legal entity GrexIt).");
    println!("  Founder:    Niraj Ranjan Rout (CEO; still founder-led).");
    println!("  Original:   browser extension turning Gmail shared labels into team inboxes.");
    println!("  Backers:    K1 Capital (Series C lead), Kalaari Capital, Kae Capital.");
    println!("  Funding:    $24M Series C Jul 2022; ~$31M total raised.");
    println!("  Scale:      ~10,000+ customer companies, ~150K+ users.");
    println!("  Position:   Google Workspace-native shared-inbox + help-desk leader.");
}

fn run_workspace() {
    println!("Lives inside Gmail / Google Workspace.");
    println!("  No separate app; Hiver is a Chrome extension + Gmail add-on +");
    println!("  background sync that lives in the existing Gmail UI.");
    println!("  Agent opens Gmail, sees the support@ shared inbox as a left-rail label,");
    println!("  with Hiver-injected UI for assignment, status, internal notes, SLA chips.");
    println!("  Onboarding pitch: 'your team already knows Gmail; we don't ask them to");
    println!("  learn a help-desk product'.");
    println!("  Google Workspace Marketplace top-rated app — major top-of-funnel channel.");
    println!("  Mirrors data into Hiver's cloud for reporting + automation, source of truth");
    println!("  remains the team's Gmail mailbox.");
}

fn run_inbox() {
    println!("Shared label inbox.");
    println!("  Map shared addresses (support@, sales@, billing@) onto Gmail shared labels.");
    println!("  Assign conversations to teammates with status (Open / Pending / Closed).");
    println!("  Internal @-mentions next to the customer thread, invisible to the customer.");
    println!("  Collision indicators when teammates are viewing / replying to the same thread.");
    println!("  SLA tracking with overdue chips + breach notifications.");
    println!("  Tag-based categorisation + saved-view filters per teammate.");
}

fn run_automation() {
    println!("Automation + workflow rules.");
    println!("  Trigger + condition + action no-code rule builder.");
    println!("  Triggers: new email, status change, tag added, SLA breach.");
    println!("  Conditions: sender, subject regex, label, customer attribute, time-of-day.");
    println!("  Actions: assign to teammate or round-robin, apply tag, change status, send");
    println!("  canned response, fire webhook.");
    println!("  Canned-response library with merge tags + per-team folders.");
    println!("  Round-robin + load-balanced assignment among support pods.");
}

fn run_channels() {
    println!("Channel coverage (expanded over time).");
    println!("  Email: native Gmail integration (the original product).");
    println!("  Live chat: embeddable widget, conversations route into Gmail + Hiver UI.");
    println!("  WhatsApp Business via WhatsApp Cloud API.");
    println!("  Voice: integrated phone channel (newer addition, partner-backed).");
    println!("  Knowledge base: customer-facing help-centre + agent suggestions.");
    println!("  Multi-channel customer profile: all channel touches stitched per contact.");
}

fn run_analytics() {
    println!("Analytics + reporting.");
    println!("  Volume + response-time + resolution-time dashboards.");
    println!("  Per-teammate workload + handling-time + CSAT.");
    println!("  Tag-based theme reporting (refund vs technical vs billing breakdown).");
    println!("  SLA-attainment reporting + breach trend tracking.");
    println!("  Export to CSV + Google Sheets sync.");
    println!("  Modest analytics depth — Hiver's center of gravity is in the Gmail workflow,");
    println!("  not BI-grade reporting.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Lite:     ~$15 per user per month, basic shared inbox + tagging.");
    println!("  Pro:      ~$39 per user per month, adds SLA + analytics + automation.");
    println!("  Elite:    ~$59 per user per month, adds live chat + voice + advanced reports.");
    println!("  Annual prepay discount.");
    println!("  Requires existing Google Workspace seats — Hiver does not stand alone.");
    println!("  No per-conversation or per-ticket fees; flat per-seat model.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: Google Workspace-based SMBs + mid-market, 10-500 employees.");
    println!("  Industries: professional services, e-commerce ops, finance + accounting");
    println!("  shared-mailbox teams, IT helpdesks at non-tech companies, education,");
    println!("  nonprofits, real-estate brokerages.");
    println!("  Geographic: nationwide US + UK + APAC; heavy India + India-adjacent footprint.");
    println!("  Frequently named customers: Vacasa, Flexport (selectively), Pluralsight,");
    println!("  Upwork (historical), several large education + nonprofit organisations.");
    println!("  Anti-segment: customers heavily invested in Microsoft 365 — Hiver Outlook");
    println!("  exists but is much less mature than the Gmail product.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "hiver-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "workspace" => run_workspace(),
        "inbox" => run_inbox(),
        "automation" => run_automation(),
        "channels" => run_channels(),
        "analytics" => run_analytics(),
        "pricing" => run_pricing(),
        "customers" => run_customers(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs() {
        run_about();
        run_workspace();
        run_inbox();
        run_automation();
        run_channels();
        run_analytics();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("hiver-cli");
        print_version();
    }
}
