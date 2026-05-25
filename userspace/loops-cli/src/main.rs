#![deny(clippy::all)]
//! loops-cli — OurOS Loops.so SaaS email lifecycle platform personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Loops.so email platform for modern SaaS.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           Loops.so origin and indie-founder ethos");
    println!("    products        Transactional + Loops (sequences) + Campaigns");
    println!("    sequences       Event-driven email sequences for SaaS");
    println!("    pricing         Per-contact tiers");
    println!("    customers       Notable users");
    println!("    differentiator  Why founders pick Loops over Customer.io");
    println!("    critique        Honest critique");
    println!("    help / version");
}

fn print_about() {
    println!("Loops — email for modern SaaS, indie-founded.");
    println!();
    println!("Founded 2022 in the UK by Marcel Pociot, Chris White, and a");
    println!("small team. Marcel is a long-time indie maker (Beyond Code,");
    println!("Tinkerwell, Expose, Pingping). Chris brought design and email-");
    println!("delivery operational experience. The cofounder ethos: 'we");
    println!("kept building the same internal email tool for every SaaS we");
    println!("worked on — so we built it once properly and made it a product.'");
    println!();
    println!("Loops launched March 2023 with a focused product vision: email");
    println!("for SaaS that needs (1) transactional emails (welcome, receipts,");
    println!("password resets), (2) event-driven sequences (onboarding flows,");
    println!("trial reminders, churn nudges), and (3) one-off campaigns");
    println!("(product announcements, newsletters). The argument: existing");
    println!("tools serve either marketing (Mailchimp, Klaviyo) or transactional");
    println!("(SendGrid, Postmark) but few do both with a calm DX.");
    println!();
    println!("Funding posture is mostly bootstrapped + small angel investment.");
    println!("Marcel publishes revenue progress publicly — Loops hit $1M ARR");
    println!("about 18 months in. Strong indie/maker brand affinity. Customer");
    println!("acquisition primarily through founder networks, indie-hacker");
    println!("communities, and word-of-mouth in the Laravel/Next.js/maker world.");
}

fn print_products() {
    println!("Loops product line:");
    println!();
    println!("• Transactional Email");
    println!("    Send programmatic email via REST API. Receipts, password");
    println!("    resets, magic links. Built on top of major underlying ESP");
    println!("    networks for delivery, with Loops handling templating,");
    println!("    sending, and analytics.");
    println!();
    println!("• Loops (Sequences)");
    println!("    The eponymous product feature. Event-driven email sequences:");
    println!("    'when user signs up, send Welcome email; 3 days later send");
    println!("    Quick Start; on Day 7 send Pro Features.' Configurable");
    println!("    branches, delays, conditions, exit criteria. Loops can");
    println!("    even branch on whether previous emails were opened/clicked.");
    println!();
    println!("• Campaigns");
    println!("    Newsletter-style email sends to segments. Visual editor,");
    println!("    block-based templates, A/B test subject lines, schedule");
    println!("    in advance. Analytics on opens/clicks per recipient.");
    println!();
    println!("• Audience Management");
    println!("    Contacts + audiences + segments. Sync from your app via");
    println!("    API or via integrations (Stripe, Supabase, Clerk, Posthog,");
    println!("    Vercel, Convex, Mixpanel). Tag, segment, suppress.");
    println!();
    println!("• Templates");
    println!("    Visual block-based template editor. HTML-export option for");
    println!("    teams that want to hand-code. Variables, conditional");
    println!("    blocks, unsubscribe footer auto-injection.");
    println!();
    println!("• Webhooks & API");
    println!("    Bi-directional: webhooks for events (subscribed, unsubscribed,");
    println!("    bounced, etc.). API for contact CRUD + send + segment");
    println!("    management. Official SDKs for Node, Python.");
}

fn print_sequences() {
    println!("Loops sequences — the differentiating feature.");
    println!();
    println!("The problem most SaaS founders face: 'I want to send a 5-email");
    println!("onboarding sequence triggered by signup, with a branch for users");
    println!("who upgrade to Pro early.' Implementing this on top of a pure");
    println!("transactional API requires you to build:");
    println!("  • A queue with delayed jobs");
    println!("  • State per recipient (which step are they on?)");
    println!("  • Branching/exit logic");
    println!("  • Pause-on-upgrade behavior");
    println!("  • Analytics on each step's open/click/conversion rates");
    println!();
    println!("This is non-trivial. Customer.io and Iterable solve it but at");
    println!("enterprise pricing/complexity. Loops solves it for the indie");
    println!("SaaS niche:");
    println!();
    println!("  • Visual sequence designer — drag steps onto a timeline");
    println!("  • Trigger from event: contact.created, contact.updated,");
    println!("    custom-event (POST /events from your app)");
    println!("  • Per-step delays: minutes, hours, days, business days");
    println!("  • Conditional branches: 'if subscription.status == active'");
    println!("  • Exit conditions: 'stop sending if user.upgraded == true'");
    println!("  • A/B testing within sequences");
    println!("  • Recipient timeline view: see exactly which emails each");
    println!("    contact has been sent and where they are in each loop");
    println!();
    println!("The mental model is closer to what you'd build yourself in code,");
    println!("but visual and operated. For most SaaS founders this is the");
    println!("'80% feature for 20% of Customer.io cost' sweet spot.");
}

fn print_pricing() {
    println!("Loops pricing (USD, 2025):");
    println!();
    println!("• Free");
    println!("    1,000 contacts, unlimited emails, all features, 1 team");
    println!("    member. Generous enough for early-stage indie projects.");
    println!();
    println!("• Pro tiers (per-contact, monthly):");
    println!("    5K contacts — $49/mo");
    println!("    10K contacts — $99/mo");
    println!("    20K contacts — $179/mo");
    println!("    50K contacts — $399/mo");
    println!("    100K contacts — $699/mo");
    println!("    Higher tiers — custom");
    println!();
    println!("• All Pro plans include unlimited emails (no per-email charge),");
    println!("  all sequence + campaign + transactional features, unlimited");
    println!("  team members.");
    println!();
    println!("Honest take: Loops's per-contact model is friendly for low-");
    println!("send-per-contact SaaS (e.g., a tool with 20K users but only");
    println!("a few emails per user per month). Less friendly for high-volume");
    println!("transactional senders where Postmark/Resend per-email pricing");
    println!("works out cheaper.");
}

fn print_customers() {
    println!("Loops customer references (observable from public mentions):");
    println!();
    println!("  • Cal.com — meeting scheduler email lifecycle");
    println!("  • Bento (formerly Resend competitor) — transitioned use cases");
    println!("  • Pieces for Developers — onboarding sequences");
    println!("  • Buildkite — internal product email");
    println!("  • Many bootstrapped SaaS in the maker/Twitter community");
    println!("  • Laravel + indie-PHP-stack startups (Marcel's network)");
    println!("  • Bun (Oven) — product update emails");
    println!("  • Various Y Combinator and bootstrapped companies");
    println!();
    println!("Pattern: founder-led SaaS, indie-maker projects, post-launch");
    println!("startups that want both transactional + lifecycle without");
    println!("integrating Customer.io's enterprise complexity.");
}

fn print_differentiator() {
    println!("Why founders pick Loops:");
    println!();
    println!("• Unified transactional + sequences + campaigns. Most");
    println!("  competitors do one well; Loops does all three at the level");
    println!("  most SaaS startups need.");
    println!();
    println!("• Excellent visual sequence designer. As good or better than");
    println!("  Customer.io's, at a fraction of the price.");
    println!();
    println!("• Indie/maker brand alignment. Founder publicly shares revenue,");
    println!("  builds in public, ships fast. Customer support feels personal.");
    println!();
    println!("• Per-contact pricing with unlimited emails. Predictable bills;");
    println!("  no anxiety about a high-traffic campaign spiking your bill.");
    println!();
    println!("• Modern integrations: Stripe, Supabase, Clerk, Convex, Posthog,");
    println!("  Vercel, Mixpanel, Segment. Built for the post-2020 SaaS stack.");
    println!();
    println!("• Visual editor is clean and uncluttered. No 90s-era marketing-");
    println!("  tool feel like Mailchimp's heritage UI.");
    println!();
    println!("vs. Customer.io: Customer.io has deeper behavioral analytics,");
    println!("  more powerful segmentation, larger feature surface. Loops is");
    println!("  5-10x cheaper at SaaS-startup volumes and faster to set up.");
    println!();
    println!("vs. Mailchimp: Mailchimp is marketing-first with weaker");
    println!("  transactional + sequencing. Loops is more SaaS-shaped.");
    println!();
    println!("vs. Resend/Postmark: those are transactional-only. Loops adds");
    println!("  sequences and campaigns. Different product surfaces — many");
    println!("  teams use Resend + Customer.io or just Loops alone.");
}

fn print_critique() {
    println!("Honest critique of Loops:");
    println!();
    println!("• Young product. Some features (advanced segmentation, multi-");
    println!("  workspace, fine-grained team permissions) are still maturing.");
    println!();
    println!("• Smaller integration ecosystem than Customer.io/Iterable.");
    println!("  Custom integrations require API work.");
    println!();
    println!("• Per-contact pricing penalizes high-contact-count low-engagement");
    println!("  use cases. If you have 200K free-tier signups who never");
    println!("  upgrade, the bill at 200K contacts is hard to justify.");
    println!();
    println!("• Limited HTML email control. The visual editor is great for");
    println!("  most cases but advanced HTML email designers may find it");
    println!("  constraining.");
    println!();
    println!("• No inbound parsing. If you need to receive replies and");
    println!("  process them programmatically, Loops doesn't do that.");
    println!();
    println!("• Less enterprise feature surface: no SOC 2 audit history of");
    println!("  Customer.io's depth, fewer RBAC primitives, less mature");
    println!("  audit logs. Loops's brand is indie SaaS, not Fortune 500.");
    println!();
    println!("• Brand awareness still small outside the indie/maker network.");
    println!("  CTO of a 500-person company may not have heard of Loops.");
    println!();
    println!("• Marcel's Loops vs. competitors' confusion: 'loops.so' is");
    println!("  distinct from the Loop messaging app, Loops.so workout app,");
    println!("  and other Loop-named products. Brand collision noise.");
}

fn run_loops(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (OurOS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "sequences" | "seq" => { print_sequences(); 0 }
        "pricing" => { print_pricing(); 0 }
        "customers" => { print_customers(); 0 }
        "differentiator" | "diff" => { print_differentiator(); 0 }
        "critique" => { print_critique(); 0 }
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'");
            eprintln!("Try '{prog} help' for usage.");
            2
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "loops".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_loops(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/loops"), "loops"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("loops.exe"), "loops"); }
    #[test] fn t_help() { assert_eq!(run_loops(&[], "loops"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_loops(&["xx".to_string()], "loops"), 2); }
}
