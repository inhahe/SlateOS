#![deny(clippy::all)]
//! helpcrunch-cli — personality CLI for HelpCrunch, the budget-priced
//! Intercom-style customer-comms platform.
//!
//! Founded 2016 in Kyiv, Ukraine by Andrii Sidashov as a deliberately
//! lower-priced answer to Intercom's increasingly enterprise-focused
//! pricing. Bundles live chat, email automation, knowledge base, chatbots,
//! and a unified customer profile into one product at SMB-friendly
//! monthly pricing. Fully bootstrapped + funded from revenue; remained
//! operational through the 2022+ wartime period with distributed team
//! across Ukraine + EU. The bet has always been the same: same
//! Intercom-shaped feature box for roughly a third of the price.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — HelpCrunch budget Intercom-alternative personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Andrii Sidashov 2016 Kyiv; bootstrapped");
    println!("    intercom      The intentional 'cheaper Intercom' positioning");
    println!("    chat          Live + delayed chat widget + agent desktop");
    println!("    email         Email marketing + auto-message + segmentation");
    println!("    kb            Knowledge-base + chatbot deflection");
    println!("    profiles      Unified customer profile + custom-attributes");
    println!("    pricing       Per-team-member-per-month flat tier pricing");
    println!("    customers     SMB SaaS + e-commerce customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("helpcrunch-cli 0.1.0 (budget-customer-comms personality build)"); }

fn run_about() {
    println!("HelpCrunch.");
    println!("  Founded:    2016, Kyiv, Ukraine.");
    println!("  Founder:    Andrii Sidashov (CEO; still founder-led).");
    println!("  Status:     Bootstrapped, revenue-funded.");
    println!("  Team:       distributed across Ukraine + EU; operational through and");
    println!("              post the 2022+ wartime period — typical Ukrainian-SaaS resilience.");
    println!("  Position:   ~3rd of Intercom's per-seat price for a comparable feature set,");
    println!("              aimed squarely at SMBs that were priced out of Intercom's market move.");
}

fn run_intercom() {
    println!("'Cheaper Intercom' positioning.");
    println!("  The intentional pitch: Intercom progressively moved up-market with");
    println!("  per-active-user + per-message billing complexity. SMBs got squeezed.");
    println!("  HelpCrunch shipped the same conceptual box — live chat, in-app messages,");
    println!("  email automation, KB + bot — at a deliberately simpler per-team-member fee.");
    println!("  Direct landing pages comparing HelpCrunch line-by-line vs Intercom on price,");
    println!("  features, fairness of billing.");
    println!("  Common path: Intercom customer hits an unexpected $X,XXX/month renewal,");
    println!("  cancels, lands on HelpCrunch within a few searches.");
}

fn run_chat() {
    println!("Chat widget + agent desktop.");
    println!("  Embeddable web widget + native iOS / Android SDKs.");
    println!("  Live chat with agent presence + canned responses + file uploads.");
    println!("  Delayed chat: customer leaves a message off-hours, gets emailed when the");
    println!("  agent responds — same conversation continues either channel.");
    println!("  Chat-routing rules by page / segment / customer attribute.");
    println!("  Pre-chat surveys + post-chat CSAT collection.");
}

fn run_email() {
    println!("Email + auto-message campaigns.");
    println!("  Behavioral triggers: send email N days after signup, on page X visited,");
    println!("  on event Y fired from product (custom events fed via JS SDK).");
    println!("  Drip sequences + welcome series + reactivation campaigns.");
    println!("  Segmentation by custom attributes + tags + lifecycle stage.");
    println!("  A/B testing on subject lines + body content.");
    println!("  Less depth than Customer.io / Klaviyo but more than basic transactional-email tools.");
}

fn run_kb() {
    println!("Knowledge base + chatbot deflection.");
    println!("  Public help-centre with branded subdomain + custom-CSS.");
    println!("  Article authoring with rich text + categories + tags.");
    println!("  Chat widget surfaces relevant KB articles before connecting to an agent —");
    println!("  the standard 'deflect first, escalate second' SMB-CS pattern.");
    println!("  Chatbot: visual rule-based bot for FAQ + lead capture; newer LLM-backed");
    println!("  generative answers from the KB content.");
    println!("  Multi-language: KB articles can be authored per locale.");
}

fn run_profiles() {
    println!("Unified customer profile.");
    println!("  Per-visitor record: email, name, device, location, plan, lifecycle stage.");
    println!("  Custom attributes: any product-side concept (subscription tier, MRR, last");
    println!("  feature used, etc.) pushed via JS SDK + visible in agent desktop.");
    println!("  Conversation history across chat + email channels on the same record.");
    println!("  Segmentation: filter by any attribute combination, save as segments,");
    println!("  use as targeting in campaigns.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Basic:    ~$15 per team member per month, basic chat + KB + email.");
    println!("  Pro:      ~$25 per team member per month, adds advanced reports + chatbots.");
    println!("  Unlimited: ~$620/month flat (any number of team members), enterprise tier.");
    println!("  Annual contracts ~30%% discount vs month-to-month.");
    println!("  Comparison page maintained showing HelpCrunch vs Intercom on identical");
    println!("  scenarios — explicit underdog pricing strategy.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: 5-50 employee SMB SaaS + e-commerce + agencies.");
    println!("  Industries: bootstrapped + small-VC SaaS, Shopify + WooCommerce stores,");
    println!("  digital agencies handling client comms, indie product companies.");
    println!("  Geographic: heavy EU + Ukraine + commonwealth + LATAM presence.");
    println!("  Common origin: ex-Intercom customer that downgraded after a price shock.");
    println!("  Rarely sells into enterprise — explicitly SMB / mid-market by design.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "helpcrunch-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "intercom" => run_intercom(),
        "chat" => run_chat(),
        "email" => run_email(),
        "kb" => run_kb(),
        "profiles" => run_profiles(),
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
        run_intercom();
        run_chat();
        run_email();
        run_kb();
        run_profiles();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("helpcrunch-cli");
        print_version();
    }
}
