#![deny(clippy::all)]
//! front-cli — personality CLI for Front, the shared-inbox + customer
//! operations platform.
//!
//! Founded 2013 in Paris by Mathilde Collin (CEO, ex-eFounders) and
//! Laurent Perrin. The original wedge: turn shared email aliases
//! (support@, sales@, billing@) into actually-collaborative team inboxes —
//! with assignment, internal comments threaded next to messages, SLAs,
//! and rules. Distinct from Zendesk-style help desks because Front
//! preserves the email thread as the primary object; the help-desk-ticket
//! abstraction sits on top. Relocated HQ to San Francisco mid-2010s.
//! Series D Jan 2022 at $1.7B valuation led by Sequoia + Salesforce
//! Ventures, with strategic investments from Atlassian + Slack
//! (pre-Salesforce).

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Front shared-inbox + customer-ops personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Mathilde Collin + Laurent Perrin 2013 Paris/SF");
    println!("    inbox         Shared inbox + assignment + collision detection");
    println!("    channels      Email + SMS + WhatsApp + chat + Twitter unified");
    println!("    rules         Workflow rules + macros + auto-assignment");
    println!("    analytics     Reporting, SLAs, response-time + workload metrics");
    println!("    api           Front API + AI Answers + integrations marketplace");
    println!("    pricing       Per-seat-per-month tiered pricing");
    println!("    customers     Operations-heavy team customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("front-cli 0.1.0 (shared-inbox-team personality build)"); }

fn run_about() {
    println!("Front (Front App, Inc.).");
    println!("  Founded:    2013, Paris (eFounders-incubated).");
    println!("  Founders:   Mathilde Collin (CEO) + Laurent Perrin (CTO).");
    println!("  HQ:         San Francisco (since 2014/2015 US relocation).");
    println!("  Backers:    Sequoia, Initialized, eFounders, Salesforce Ventures,");
    println!("              Atlassian, Slack Fund, DAG Ventures.");
    println!("  Funding:    $65M Series D Jan 2022 at $1.7B valuation;");
    println!("              ~$204M total raised.");
    println!("  Differentiator: email-thread-first collaboration, not ticket-first.");
}

fn run_inbox() {
    println!("Shared inbox.");
    println!("  Each shared address (support@, sales@, etc.) appears as a team inbox.");
    println!("  Assign threads to teammates; @-mention internally without leaking to");
    println!("  the customer (internal comments are invisible to external recipients).");
    println!("  Collision detection: if two agents are typing replies to the same");
    println!("  thread, both see a warning before either sends.");
    println!("  Read-status sync: when one teammate reads a message, the team sees it");
    println!("  as handled — eliminates duplicate replies.");
    println!("  Snooze, tag, archive flow that matches modern personal-email habits.");
}

fn run_channels() {
    println!("Channels supported in the unified inbox.");
    println!("  Email (IMAP + Gmail API + Microsoft 365 native).");
    println!("  SMS (Twilio + Front number provisioning).");
    println!("  WhatsApp Business + Facebook Messenger + Instagram DM.");
    println!("  Front Chat (live chat widget) embeddable on websites.");
    println!("  Twitter/X DM + mentions.");
    println!("  Custom channels via API for proprietary message sources.");
    println!("  Single thread can span multiple channels — e.g. continues over SMS");
    println!("  after starting in email.");
}

fn run_rules() {
    println!("Rules + workflows.");
    println!("  Trigger + condition + action no-code rule builder.");
    println!("  Triggers: message in, tag added, SLA breached, schedule (e.g. business");
    println!("            hours start), webhook from external system.");
    println!("  Conditions: sender domain, subject regex, customer attribute from CRM,");
    println!("              previous thread history, tag presence.");
    println!("  Actions: assign to teammate or rota, apply tags, set SLA, auto-reply");
    println!("           with template, post Slack notification, run plugin.");
    println!("  Macros: one-click templated reply + state-change combos.");
    println!("  Rotating assignment (round-robin / least-loaded) for support pods.");
}

fn run_analytics() {
    println!("Analytics + reporting.");
    println!("  Per-teammate workload + response-time + resolution metrics.");
    println!("  SLA dashboards: first-response time, total-resolution time, breach alerts.");
    println!("  Channel-level reporting (email vs SMS vs chat performance).");
    println!("  CSAT survey send-after-resolution + rating collection.");
    println!("  Customer-level conversation history + sentiment trends.");
    println!("  Custom-report builder + scheduled-export to CSV / BI tools.");
}

fn run_api() {
    println!("API + extensibility.");
    println!("  Public REST API: send messages, create rules, fetch threads + comments.");
    println!("  Webhook subscriptions on most inbox events.");
    println!("  Plugin SDK: embed external apps as side-pane plugins next to each thread");
    println!("  (e.g. show Salesforce account info, Stripe customer data, Shopify orders).");
    println!("  AI Answers: AI-assisted drafting + summarisation in-thread, GPT-backed.");
    println!("  Marketplace of pre-built integrations: Salesforce, HubSpot, Stripe,");
    println!("  Shopify, Linear, Jira, GitHub, Slack, Asana, Gong, ChiliPiper.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Starter:  ~$19 per seat per month (annual), basic shared inbox.");
    println!("  Growth:   ~$59 per seat per month (annual), rules + analytics + AI.");
    println!("  Scale:    ~$99 per seat per month (annual), advanced analytics + SAML.");
    println!("  Premier:  custom enterprise pricing, dedicated CSM + custom contracts.");
    println!("  Annual discounts ~20%% vs month-to-month; min ~2 seats.");
    println!("  AI Answers may be priced separately depending on plan + usage.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: ops-heavy teams 20-500 employees needing shared mailbox");
    println!("  collaboration on customer or partner threads.");
    println!("  Strong verticals: logistics + freight forwarding (Flexport-style),");
    println!("  fintech / B2B finance ops, professional-services CS, agencies handling");
    println!("  client comms, software vendors managing partner channels.");
    println!("  Named customers: Shopify (selectively), Lyft, MongoDB, Pinterest,");
    println!("  Cousins Maine Lobster, several large logistics + 3PL operators.");
    println!("  Often coexists with Zendesk (tickets) + Salesforce (CRM) — Front sits");
    println!("  on top of shared *mailboxes*, not the main support-ticket queue.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "front-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "inbox" => run_inbox(),
        "channels" => run_channels(),
        "rules" => run_rules(),
        "analytics" => run_analytics(),
        "api" => run_api(),
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
        run_inbox();
        run_channels();
        run_rules();
        run_analytics();
        run_api();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("front-cli");
        print_version();
    }
}
