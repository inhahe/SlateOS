#![deny(clippy::all)]
//! getresponse-cli — personality CLI for GetResponse, one of the
//! longest-running email marketing platforms.
//!
//! Founded 1998 in Gdańsk, Poland by Simon Grabowski. GetResponse has
//! been bootstrapped + profitable for most of its 25+ year history,
//! growing from a simple autoresponder into a full email + landing
//! page + webinar + marketing automation suite — explicitly aimed at
//! SMBs and creators rather than enterprise. Owns its own email
//! delivery infrastructure and runs data centres in Poland (with EU
//! data residency a marketing point against US-hosted competitors).

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — GetResponse SMB email marketing personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Grabowski 1998 Gdańsk, 25+ years bootstrapped");
    println!("    email         Autoresponders + drag-drop email editor");
    println!("    automation    Workflow builder, triggers, conditions");
    println!("    landing       Landing page + popup + form builder");
    println!("    webinar       Built-in webinar hosting, unusual for the segment");
    println!("    ai            Generative campaigns + subject-line scoring");
    println!("    pricing       Tiered per-contact + free-forever 500 list");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("getresponse-cli 0.1.0 (Polish SMB-suite personality build)"); }

fn run_about() {
    println!("GetResponse S.A.");
    println!("  Founded:   1998, Gdańsk, Poland.");
    println!("  Founder:   Simon (Szymon) Grabowski (CEO).");
    println!("  Funding:   Bootstrapped for most of its history; profitable.");
    println!("  Listing:   Listed on Warsaw Stock Exchange.");
    println!("  Posture:   SMB + creator-focused, multilingual UI from day one,");
    println!("             EU data residency by default.");
    println!("  Footprint: 350K+ customers across ~180 countries.");
}

fn run_email() {
    println!("Email + autoresponders.");
    println!("  Drag-drop email editor with hundreds of templates.");
    println!("  Autoresponders: day-based + event-based send sequences.");
    println!("  A/B testing on subject line, content, send time.");
    println!("  Perfect-Timing send (per-recipient ML-picked time).");
    println!("  Time-travel send: same local hour across time zones.");
    println!("  List hygiene: bounce + complaint tracking, hard-bounce remove.");
}

fn run_automation() {
    println!("Marketing Automation workflows.");
    println!("  Visual canvas of triggers + conditions + actions.");
    println!("  Triggers: signup, abandoned cart, page visit, tag changed,");
    println!("            URL clicked, custom field changed, webhook in.");
    println!("  Conditions: tag, score, segment, deal stage, custom field.");
    println!("  Actions: send email, wait, score change, tag, move to list.");
    println!("  Lead scoring + sales-tagging built-in.");
}

fn run_landing() {
    println!("Landing pages + forms + popups.");
    println!("  Drag-drop landing page builder, mobile-responsive.");
    println!("  Popup + signup form builder with exit-intent + scroll triggers.");
    println!("  Conversion funnels: visualise the path from ad to purchase.");
    println!("  Hosted at gr8.com (free) or custom domain (paid plans).");
    println!("  A/B test pages, track conversions, integrate with paid ads.");
}

fn run_webinar() {
    println!("Built-in webinars — uncommon in the segment.");
    println!("  Stream up to 1000 attendees, screen share, polls, Q&A, recording.");
    println!("  Tied directly into the contact database — signups become leads.");
    println!("  Auto-webinars: pre-recorded events that play on a schedule with");
    println!("  live-chat moderation.");
    println!("  Reduces stack: no Zoom + Eventbrite + email vendor combo needed.");
}

fn run_ai() {
    println!("AI features (added over 2023-2024).");
    println!("  AI Email Generator: prompt + brand voice -> draft campaign.");
    println!("  AI Subject Line Generator + scorer.");
    println!("  AI Campaign Generator: full multi-message sequence from a brief.");
    println!("  AI Product Recommendations for ecom integrations.");
    println!("  Tied to OpenAI under the hood.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free Forever       up to 500 contacts, basic email + 1 landing pg.");
    println!("  Email Marketing    per-contact tiers from ~$15/mo for 1K contacts.");
    println!("  Marketing Auto     adds workflows + scoring + webinars.");
    println!("  Ecommerce Marketing adds product recs + abandoned cart + SMS.");
    println!("  MAX (Enterprise)   dedicated IP, transactional, SLAs, custom infra.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Strong base of Eastern + Central European SMBs.");
    println!("  Creator + course-seller niche worldwide.");
    println!("  Some enterprise accounts: IKEA, Stripe (regional teams),");
    println!("  Carrefour, Zendesk affiliate program have used it.");
    println!("  Heavy GDPR-sensitive customer base wanting EU data residency.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "getresponse-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "email" => run_email(),
        "automation" => run_automation(),
        "landing" => run_landing(),
        "webinar" => run_webinar(),
        "ai" => run_ai(),
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
        run_email();
        run_automation();
        run_landing();
        run_webinar();
        run_ai();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("getresponse-cli");
        print_version();
    }
}
