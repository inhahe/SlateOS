#![deny(clippy::all)]
//! kit-cli — personality CLI for Kit (formerly ConvertKit), the email
//! marketing platform built for creators.
//!
//! Founded 2013 by Nathan Barry, who very publicly bootstrapped the
//! company starting from a small subscriber list of his own ebooks and
//! grew it to a 9-figure ARR business while writing detailed monthly
//! transparency reports on the company blog. Renamed from ConvertKit to
//! Kit in mid-2024. The product is opinionated around the creator
//! economy — newsletters, paid subscriptions, recommendations between
//! creators, paid sponsorships — rather than general SMB email.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Kit creator-economy email personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Nathan Barry 2013, bootstrapped, ConvertKit -> Kit");
    println!("    sequences     Tag-based subscriber model");
    println!("    automations   Visual rules + sequences");
    println!("    creator       Tip Jar, Commerce, recommendations, sponsor network");
    println!("    forms         Embed + popup + landing page builder");
    println!("    deliverability Domain auth, IP warm, sender reputation tools");
    println!("    pricing       Free up to 10K subs, then per-subscriber");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("kit-cli 0.1.0 (creator-economy personality build)"); }

fn run_about() {
    println!("Kit (formerly ConvertKit, Inc.).");
    println!("  Founded:    2013 by Nathan Barry.");
    println!("  HQ:         Fully remote, US-based core team.");
    println!("  Funding:    Bootstrapped to ~$40M+ ARR before any outside funding.");
    println!("  Transparency: Barry famously published detailed monthly revenue +");
    println!("             team + customer metrics for years on the company blog.");
    println!("  Rename:     ConvertKit -> Kit mid-2024 (shorter, broader brand).");
    println!("  Audience:   Bloggers, podcasters, YouTubers, course creators,");
    println!("             musicians, authors, newsletter writers.");
}

fn run_sequences() {
    println!("Subscriber model — tags first, lists second.");
    println!("  One subscriber row per email address, regardless of how they joined.");
    println!("  Tags + segments describe interests; sequences are time-based.");
    println!("  This avoids paying for the same subscriber on multiple lists,");
    println!("  the classic 'dupe billing' problem at older ESPs.");
    println!("  Sequences = drip emails; broadcasts = one-off newsletter sends.");
}

fn run_automations() {
    println!("Visual Automations.");
    println!("  Canvas of events + actions + filters.");
    println!("  Events: subscribed to form, completed sequence, tag added,");
    println!("          purchased product, custom field updated.");
    println!("  Actions: add tag, remove tag, subscribe to sequence, run wait,");
    println!("           send broadcast, webhook out.");
    println!("  Filters: tags, segments, custom fields, sequence completion.");
}

fn run_creator() {
    println!("Creator-economy features — what makes Kit not just an ESP.");
    println!("  Kit Commerce: paid subscriptions + one-time digital product sales,");
    println!("    Kit handles Stripe + delivery + email receipts.");
    println!("  Tip Jar: accept tips inline in newsletters.");
    println!("  Creator Network + Recommendations: subscribers opt into other");
    println!("    creators' lists at signup; mutual cross-promotion baked in.");
    println!("  Sponsor Network: opt-in marketplace for paid newsletter sponsorships.");
}

fn run_forms() {
    println!("Forms + landing pages.");
    println!("  Inline embed forms, slide-in, modal, sticky bar forms.");
    println!("  Hosted landing page templates (no website required).");
    println!("  Custom subdomain or full custom-domain landing pages.");
    println!("  Lead magnet delivery: subscriber confirmation auto-delivers a file.");
    println!("  Built-in form analytics: views, signups, conversion rate.");
}

fn run_deliverability() {
    println!("Deliverability tooling.");
    println!("  Sender domain authentication: SPF, DKIM, DMARC config wizard.");
    println!("  Custom domain sending: from@creators-own-domain.com.");
    println!("  Dedicated IP option on higher tiers.");
    println!("  Engagement-based list hygiene + cold-subscriber detection.");
    println!("  Inbox preview + spam test in the broadcast editor.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Newsletter   free up to 10,000 subscribers, basic features only.");
    println!("  Creator      per-subscriber tier, adds automations + integrations.");
    println!("  Creator Pro  adds newsletter referrals, subscriber scoring, etc.");
    println!("  All paid tiers: unlimited sends, unlimited forms, Commerce 3.5% fee.");
    println!("  Pricing scales by subscriber count, not by send volume.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Tim Ferriss, James Clear (Atomic Habits), Tim Urban (Wait But Why),");
    println!("  Cup & Leaf, Pat Flynn, Mariah Coz, and tens of thousands of");
    println!("  Substack-class but ESP-style independent newsletter writers.");
    println!("  Course creators on Teachable + Podia + Gumroad integrate heavily.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "kit-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "sequences" => run_sequences(),
        "automations" => run_automations(),
        "creator" => run_creator(),
        "forms" => run_forms(),
        "deliverability" => run_deliverability(),
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
        run_sequences();
        run_automations();
        run_creator();
        run_forms();
        run_deliverability();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("kit-cli");
        print_version();
    }
}
