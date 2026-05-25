#![deny(clippy::all)]
//! resend-cli — OurOS Resend modern developer-first email API personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Resend modern developer-first email API.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           Resend's YC W23 origin and Drauger lineage");
    println!("    products        Email API, Audiences, Broadcasts, React Email");
    println!("    reactemail      The React Email open-source library");
    println!("    pricing         Free tier and growth tiers");
    println!("    customers       Notable users in the modern stack");
    println!("    differentiator  Why developers love Resend in 2025");
    println!("    critique        Honest critique");
    println!("    help / version");
}

fn print_about() {
    println!("Resend — email API rebuilt for the React/Next.js/Vercel era.");
    println!();
    println!("Founded December 2022 in San Francisco by Zeno Rocha (CEO) and");
    println!("Bu Kinoshita (CTO). Both are Brazilian-born engineers. Zeno was");
    println!("previously Chief Product Officer at WorkOS and before that built");
    println!("Dracula UI (the popular dark-theme open-source project) and was");
    println!("a Microsoft Regional Director. Bu was a senior engineer who'd");
    println!("worked across modern Brazilian tech (QuintoAndar, Liferay).");
    println!();
    println!("Y Combinator W23 batch. Original product launched February 2023.");
    println!("The thesis: SendGrid (2009), Mailgun (2010), Postmark (2010) were");
    println!("all built before React, before TypeScript, before the modern web");
    println!("framework era. Their developer experience reflects that. Resend");
    println!("would be the email API specifically built for teams shipping on");
    println!("Next.js, Remix, Astro, SvelteKit — TypeScript-first, React Email-");
    println!("first, Vercel-deployment-first.");
    println!();
    println!("Funding: ~$3M seed Mar 2023 (YC + angels), $18M Series A Mar");
    println!("2024 led by Benchmark with participation from Conviction, Y");
    println!("Combinator, and a long roster of well-known angels (Guillermo");
    println!("Rauch / Vercel, Lee Robinson, Pieter Levels, Tom Preston-Werner,");
    println!("Naval Ravikant, Ev Williams). Total raised ~$21M.");
    println!();
    println!("Growth has been remarkable — by mid-2024 Resend was sending");
    println!("billions of emails monthly with a small team and strong");
    println!("retention. Now considered the default email API for new");
    println!("Next.js / Vercel projects.");
}

fn print_products() {
    println!("Resend product surface:");
    println!();
    println!("• Email API");
    println!("    Single endpoint: POST /emails. Pass HTML, plain text, or a");
    println!("    React component (rendered server-side via React Email).");
    println!("    Returns an ID for tracking. Webhooks for delivered, bounced,");
    println!("    complained, opened, clicked.");
    println!();
    println!("• SDK Suite");
    println!("    Official SDKs: Node, Python, Ruby, Go, .NET, PHP, Java,");
    println!("    Rust. All thin wrappers around the REST API — minimal");
    println!("    surface area, type-safe, fast iteration.");
    println!();
    println!("• Audiences");
    println!("    Contact list management. Group contacts into audiences for");
    println!("    Broadcasts. Manage subscriptions, double opt-in, unsubscribe");
    println!("    links automatically.");
    println!();
    println!("• Broadcasts");
    println!("    Send newsletter-style email to an audience. Visual editor,");
    println!("    React Email templates, scheduled sends, A/B test subject");
    println!("    lines, real-time analytics. Marketing-tier feature.");
    println!();
    println!("• Webhooks");
    println!("    Per-event delivery to your HTTPS endpoint: email.sent,");
    println!("    email.delivered, email.bounced, email.complained,");
    println!("    email.opened, email.clicked, etc. Signed payloads.");
    println!();
    println!("• Domains");
    println!("    DNS-record-driven domain verification. Resend gives you");
    println!("    the SPF, DKIM, DMARC, and MX records to add. Verifies");
    println!("    automatically. Tracks reputation per domain.");
    println!();
    println!("• React Email (open source)");
    println!("    The open-source library that puts React Email on the map.");
    println!("    Components for buttons, hr, container, etc. Inline-styles");
    println!("    everything for email-client compatibility. Used standalone,");
    println!("    not Resend-specific.");
}

fn print_reactemail() {
    println!("React Email — the open-source layer.");
    println!();
    println!("Building HTML email templates is famously awful. Email clients");
    println!("have inconsistent CSS support: Outlook's Word-based rendering,");
    println!("Gmail's CSS sanitization, Apple Mail's relative modernity,");
    println!("Yahoo's quirks. The industry standard pre-2023 was 'paste an");
    println!("HTML table-soup template you bought on Themeforest and tweak");
    println!("until it looks OK in Litmus.'");
    println!();
    println!("React Email's approach: write email templates as React");
    println!("components, with a curated component library for email-safe");
    println!("primitives. Compile to MIME-friendly HTML with all CSS inlined,");
    println!("nested tables for layout where needed, automatic plaintext");
    println!("fallback generation, dark-mode support.");
    println!();
    println!("Components:");
    println!("  • Html, Head, Body, Container, Section, Row, Column");
    println!("  • Text, Heading, Link, Img, Hr");
    println!("  • Button (CSS-button-compatible-with-Outlook trick built in)");
    println!("  • CodeBlock, CodeInline (syntax-highlighted code blocks)");
    println!("  • Tailwind component (Tailwind CSS support via Twind)");
    println!("  • Preview (the preview text shown by inbox clients)");
    println!();
    println!("Used by Resend customers but also independently by teams that");
    println!("send via SendGrid / Postmark / SES — the library has no Resend");
    println!("lock-in. Open source MIT, ~17K GitHub stars, growing fast.");
    println!();
    println!("Pre-built starter templates: AWS-style verification email,");
    println!("Linear-style invite email, Vercel-style domain verification");
    println!("email, Notion-style magic link, etc. The starter gallery is");
    println!("called 'react-email.com' and remixes are encouraged.");
}

fn print_pricing() {
    println!("Resend pricing (USD, 2025):");
    println!();
    println!("• Free");
    println!("    100 emails/day (3K/month) free forever. 1 verified domain.");
    println!("    100 contacts. Adequate for personal projects and prototypes.");
    println!();
    println!("• Pro — $20/month");
    println!("    50K emails/month, unlimited domains, 1 user, 60-day data");
    println!("    retention, 5 webhooks, automated suppression management.");
    println!();
    println!("• Scale — $90/month");
    println!("    150K emails/month, 5 users, 365-day retention, dedicated");
    println!("    IP available, priority support.");
    println!();
    println!("• Enterprise — custom");
    println!("    Higher volumes, SLA, SSO, audit logs, custom contracts.");
    println!();
    println!("• Overage / per-email above plan: $1 per 1K emails on Pro,");
    println!("  $0.85 per 1K on Scale, custom on Enterprise.");
    println!();
    println!("Pricing is positioned below Postmark but above SES. The");
    println!("free tier is genuinely useful — many indie projects fit");
    println!("inside the 3K/month forever-free tier.");
}

fn print_customers() {
    println!("Resend customer references (public + observable):");
    println!();
    println!("  • Vercel — uses Resend for transactional emails (early customer)");
    println!("  • Cal.com — meeting scheduling notifications");
    println!("  • Linear — issue notifications");
    println!("  • Hashnode — blog notifications");
    println!("  • Raycast — product update emails");
    println!("  • Convex — backend platform transactional");
    println!("  • Buildbox / Bun (community accounts)");
    println!("  • Many YC W23/S23 and post startups");
    println!("  • Significant unannounced customers across the Next.js");
    println!("    ecosystem due to default integrations");
    println!();
    println!("Pattern: companies shipping on Vercel/Next.js/Remix, AI startups,");
    println!("dev-tool companies, content/community SaaS. The modern web");
    println!("ecosystem. Older enterprises and Microsoft-stack shops are");
    println!("less commonly Resend customers.");
}

fn print_differentiator() {
    println!("Why teams pick Resend:");
    println!();
    println!("• React Email integration. Pass React components directly to");
    println!("  the send API. No template DSL, no separate IDE for templates");
    println!("  — your email templates live in your repo as TSX files.");
    println!();
    println!("• Excellent DX. The docs are best-in-class. SDKs are tiny and");
    println!("  obvious. Onboarding from zero to first sent email is <5");
    println!("  minutes. The dashboard is clean and uncluttered.");
    println!();
    println!("• TypeScript-first. Strong types, autocomplete-friendly, error");
    println!("  messages teach you what you got wrong.");
    println!();
    println!("• Tight Next.js / Vercel ecosystem integration. Templates,");
    println!("  starter projects, Vercel domain integration.");
    println!();
    println!("• Modern brand and pricing. Free tier is generous; paid tiers");
    println!("  are accessible to small startups. No 'contact sales for");
    println!("  reasonable pricing' barriers.");
    println!();
    println!("• Open-source React Email is independently valuable, building");
    println!("  brand and trust outside the Resend product.");
    println!();
    println!("vs. SendGrid: Resend's DX is dramatically better. SendGrid is");
    println!("  the legacy default, has higher volume tiers and more enterprise");
    println!("  features (event webhook batching, sub-user accounts).");
    println!();
    println!("vs. Postmark: Postmark has longer deliverability track record");
    println!("  and broader product surface (inbound parsing, more enterprise");
    println!("  features). Resend has better React/Next.js integration.");
    println!();
    println!("vs. Mailgun: Mailgun has stronger marketing-email features.");
    println!("  Resend is purer transactional + dev-first focus.");
    println!();
    println!("vs. SES: SES is cheaper but you build reputation, bounce");
    println!("  handling, templating yourself. Resend is managed + opinionated.");
}

fn print_critique() {
    println!("Honest critique of Resend:");
    println!();
    println!("• Young company. Founded 2022, Series A 2024. Less track record");
    println!("  than 15-year incumbents. For risk-averse enterprises this");
    println!("  matters.");
    println!();
    println!("• Smaller feature surface than older incumbents. No inbound");
    println!("  parsing (yet). No dedicated transactional analytics like");
    println!("  Postmark's. Multi-user, RBAC, audit logs still maturing.");
    println!();
    println!("• Newer IP reputation pool. While Resend's deliverability has");
    println!("  been good, IP warming, ISP relationships, and reputation");
    println!("  accumulation take years. Postmark and SendGrid have a");
    println!("  decade-plus head start.");
    println!();
    println!("• Limited regional infrastructure. As of 2025, Resend is");
    println!("  primarily US-based — no dedicated EU region option yet.");
    println!("  Mailgun and SES offer EU/AU/AP regional sending.");
    println!();
    println!("• Marketing automation features are basic. Broadcasts exists");
    println!("  but doesn't compete with Customer.io, Klaviyo, Iterable for");
    println!("  behavioral marketing.");
    println!();
    println!("• Heavy Next.js/React ecosystem positioning. If you're a");
    println!("  Python/Django/Rails shop with no React, Resend feels less");
    println!("  natural — though the underlying API works fine, you miss");
    println!("  the React Email magic.");
    println!();
    println!("• Pricing tier gaps. Between Pro $20 and Scale $90, then to");
    println!("  Enterprise — some teams find no comfortable fit between");
    println!("  50K and 150K emails/month.");
}

fn run_resend(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (OurOS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "reactemail" | "react" => { print_reactemail(); 0 }
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
        .unwrap_or_else(|| "resend".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_resend(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/resend"), "resend"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("resend.exe"), "resend"); }
    #[test] fn t_help() { assert_eq!(run_resend(&[], "resend"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_resend(&["xx".to_string()], "resend"), 2); }
}
