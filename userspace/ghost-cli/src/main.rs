#![deny(clippy::all)]
//! ghost-cli — SlateOS personality CLI for Ghost, the open-source publishing platform.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Ghost — turn your audience into a business.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Founders, the Kickstarter, the non-profit foundation");
    println!("    editor      Koenig editor and the publication workflow");
    println!("    memberships Members, subscriptions, and Stripe integration");
    println!("    pro         Ghost(Pro) managed hosting");
    println!("    api         Content API and Admin API");
    println!("    foundation  Why Ghost is a non-profit and what that means");
    println!("    customers   Sites that publish on Ghost");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Open source. Independent. Built for publishers, not advertisers.");
}

fn print_version() {
    println!("ghost-cli 0.1.0");
    println!("The Ghost Foundation — Singapore. Founded 2013. Non-profit since 2014.");
}

fn cmd_about() {
    println!("Ghost — the modern publishing platform");
    println!();
    println!("FOUNDED");
    println!("  Conceived in late 2012 by John O'Nolan, an ex-WordPress UI lead.");
    println!("  John published a blog post titled \"Ghost\" describing what a");
    println!("  publishing-first WordPress fork might look like. Hannah Wolfe");
    println!("  saw it, said \"let's build it,\" and the two of them launched");
    println!("  the Kickstarter on April 29, 2013. They asked for £25K and");
    println!("  raised £196K from 5,236 backers in 30 days.");
    println!();
    println!("INCORPORATION");
    println!("  After the Kickstarter, John and Hannah incorporated as");
    println!("  The Ghost Foundation, a non-profit registered in Singapore.");
    println!("  No external investors, no equity holders, no exit pressure.");
    println!("  Every dollar of revenue is reinvested into the product or");
    println!("  donated to the open-source ecosystem.");
    println!();
    println!("HEADQUARTERS");
    println!("  Fully remote since day one. ~30 employees across ~15 time");
    println!("  zones. Annual company retreats are the only in-person time.");
}

fn cmd_editor() {
    println!("Koenig — Ghost's editor");
    println!();
    println!("ARCHITECTURE");
    println!("  Block-based, built on Lexical (Facebook's text editor framework).");
    println!("  Each block is a typed Markdown-ish unit: paragraph, header,");
    println!("  image, gallery, bookmark card, embed, code, callout, toggle,");
    println!("  product card, button, header card, file, signup form, etc.");
    println!();
    println!("WRITING FLOW");
    println!("  Slash command (/) to insert any block type. Markdown shortcuts");
    println!("  (## for h2, --- for HR, > for blockquote, ``` for code). Drag");
    println!("  to reorder, click to edit. The editor stays out of your way.");
    println!();
    println!("CARDS WORTH KNOWING");
    println!("  - Bookmark: paste a URL, get a rich link preview card.");
    println!("  - Email: content that only appears in the newsletter, not on web.");
    println!("  - Email-CTA: subscription CTA that only shows for non-members.");
    println!("  - Public preview: split a paywalled post — free above the line.");
    println!("  - HTML: arbitrary embed for power users.");
    println!();
    println!("OUTPUT");
    println!("  Lexical state stored as JSON. Rendered to HTML on save.");
    println!("  Email newsletters use a separate render pipeline targeting");
    println!("  Outlook + Gmail tables.");
}

fn cmd_memberships() {
    println!("Members — turn readers into subscribers");
    println!();
    println!("WHAT IT IS");
    println!("  Built-in subscription & paid newsletter system. Launched in");
    println!("  Ghost 3.0 (2019), made first-class in 4.0 (2021).");
    println!();
    println!("FLOWS");
    println!("  Free signup -> magic link email -> account created (no password).");
    println!("  Paid signup -> Stripe Checkout -> recurring subscription.");
    println!("  Tiers      -> free / monthly / yearly / one-time / multiple tiers.");
    println!("  Gates      -> public / members-only / paid-only / specific-tier.");
    println!();
    println!("EMAIL");
    println!("  Newsletters delivered via Mailgun. Configurable senders");
    println!("  per newsletter (multiple newsletters per site supported).");
    println!("  Member analytics: opens, clicks, click-by-link, churn.");
    println!();
    println!("STRIPE");
    println!("  Direct API key integration — Ghost takes ZERO percent.");
    println!("  Stripe's standard processing fees apply (2.9% + $0.30).");
    println!("  Compare: Substack takes 10%, Medium gates entirely.");
}

fn cmd_pro() {
    println!("Ghost(Pro) — the official managed hosting");
    println!();
    println!("WHAT IT IS");
    println!("  Hosted Ghost run by The Ghost Foundation. All proceeds fund");
    println!("  the open-source project. As of 2024 the platform powers");
    println!("  ~40K active sites and is the foundation's primary revenue source.");
    println!();
    println!("PLANS (as of 2024)");
    println!("  Starter      $9/mo  — 500 members, 1 staff user, ghost.io domain.");
    println!("  Creator      $25/mo — 1K members, 2 staff, custom domain, themes.");
    println!("  Team         $50/mo — 1K members, 5 staff, advanced themes.");
    println!("  Business     $199/mo — 10K members, 15 staff, priority support.");
    println!("  (Annual billing discounts apply; member-tier scales price.)");
    println!();
    println!("WHAT YOU GET");
    println!("  - Server + DB managed; daily backups");
    println!("  - Custom domain + automatic SSL");
    println!("  - CDN-fronted media");
    println!("  - Mailgun for newsletters (Foundation pays the bill)");
    println!("  - Stripe integration, no platform fees");
    println!("  - All updates auto-applied");
    println!();
    println!("SELF-HOST IS ALWAYS FREE");
    println!("  ghost install on any Ubuntu box. The Foundation provides");
    println!("  the same code; Pro is convenience, not lock-in.");
}

fn cmd_api() {
    println!("Ghost APIs — Content and Admin");
    println!();
    println!("CONTENT API (public, read-only)");
    println!("  Base:   /ghost/api/content/");
    println!("  Auth:   ?key=<content-api-key>  (per-integration)");
    println!("  Reads:  posts, pages, tags, authors, settings, tiers");
    println!("  Format: JSON, with content rendered to HTML or plaintext on demand");
    println!();
    println!("ADMIN API (private, full read/write)");
    println!("  Base:   /ghost/api/admin/");
    println!("  Auth:   JWT signed with per-integration secret");
    println!("  Writes: posts, pages, members, newsletters, webhooks, themes");
    println!("  Used by: integrations, custom dashboards, ETL pipelines");
    println!();
    println!("WEBHOOKS");
    println!("  Outbound on: post.published, member.added, member.deleted,");
    println!("  page.published, tag.added, and many others. Used to wire");
    println!("  Ghost to Zapier, n8n, Discord, search indexers, etc.");
    println!();
    println!("THEMES");
    println!("  Handlebars-based. Themes are just folders; install via the");
    println!("  admin panel or via gscan + the API. Default theme is Source.");
}

fn cmd_foundation() {
    println!("Why Ghost is a non-profit");
    println!();
    println!("THE BET");
    println!("  Software for publishers should not be built by ad-funded");
    println!("  platforms whose interests diverge from their writers. A non-");
    println!("  profit structure removes the pressure to monetize attention,");
    println!("  add growth hacks, or sell user data.");
    println!();
    println!("THE STRUCTURE");
    println!("  The Ghost Foundation is a Singapore-registered non-profit");
    println!("  organisation that owns the trademark, runs Ghost(Pro), and");
    println!("  employs the team. There are no shareholders. There are no");
    println!("  investors to satisfy. Surplus revenue funds product and grants.");
    println!();
    println!("THE PROOF");
    println!("  - Source code under MIT license since 2013, never relicensed");
    println!("  - No tracking pixels on customer sites");
    println!("  - No platform fee on Stripe subscriptions");
    println!("  - Public financials (annual report on ghost.org/about)");
    println!("  - 11+ years profitable, no layoffs");
    println!();
    println!("This is the OS-foundation model (Mozilla, Apache, Linux) applied");
    println!("to publishing software. It is the model Slate OS most aligns with.");
}

fn cmd_customers() {
    println!("Notable sites running on Ghost");
    println!();
    println!("  StackOverflow Blog   — engineering culture publication");
    println!("  Tinkoff Bank         — internal product blog");
    println!("  OkCupid              — data + insights blog");
    println!("  DuckDuckGo Blog      — privacy + product posts");
    println!("  Mozilla Hacks        — web technology updates");
    println!("  Sky News             — selected verticals");
    println!("  Apple Newsroom (clone) — community projects");
    println!("  Lenny's Newsletter (early) — moved to Substack later");
    println!("  Casey Newton's Platformer — paid newsletter, ~100K members");
    println!("  The Browser          — curated reading by Robert Cottrell");
    println!("  404 Media            — investigative tech journalism collective");
    println!();
    println!("Sweet spot: independent writers, journalists, technology blogs,");
    println!("and publishers who want a real CMS for their site + a real");
    println!("newsletter platform — without a platform tax on their revenue.");
}

fn run_ghost(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "editor" => { cmd_editor(); 0 }
        "memberships" => { cmd_memberships(); 0 }
        "pro" => { cmd_pro(); 0 }
        "api" => { cmd_api(); 0 }
        "foundation" => { cmd_foundation(); 0 }
        "customers" => { cmd_customers(); 0 }
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'. Try '{prog} help'.");
            2
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "ghost".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_ghost(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/ghost"), "ghost");
        assert_eq!(basename("C:\\Tools\\ghost.exe"), "ghost.exe");
        assert_eq!(basename("ghost"), "ghost");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("ghost.exe"), "ghost");
        assert_eq!(strip_ext("ghost"), "ghost");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_ghost(&["help".to_string()], "ghost"), 0);
        let _ = run_ghost(&[], "ghost");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_ghost(&["nope".to_string()], "ghost"), 2);
    }
}
