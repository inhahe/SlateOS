#![deny(clippy::all)]

//! gladly-cli — OurOS Gladly (radically personal customer service for consumer brands)
//!
//! Single personality: `gladly`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gladly(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gladly [OPTIONS]");
        println!("Gladly (OurOS) — radically personal customer service for consumer brands");
        println!();
        println!("Options:");
        println!("  --hero                 Hero pricing (concurrent agent licenses, $150/agent/mo)");
        println!("  --sidekick             Sidekick AI agent (autonomous resolution)");
        println!("  --liaison              Liaison — KB-driven AI assistant");
        println!("  --voice                Native voice/IVR (no Twilio bolt-on)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Gladly 2024 (OurOS)"); return 0; }
    println!("Gladly 2024 (OurOS)");
    println!("  Vendor: Gladly Software, Inc. (San Francisco, CA — private)");
    println!("  Founders: Joseph Ansanelli (CEO), Michael Wolfe, others; 2014");
    println!("          Ansanelli: serial entrepreneur, previously co-founded Connectify (Microsoft acq)");
    println!("          and Vontu (Symantec acq for $350M) — third major exit setting up Gladly");
    println!("  Founded: 2014 — incubated by Greylock for ~2 years before launch");
    println!("  Funding: ~$166M raised over Series A-E");
    println!("          Greylock, NEA, GGV Capital, Glynn Capital");
    println!("          Last round 2022 Series E at $750M valuation (down round to ~$500M rumored 2023)");
    println!("  Defining philosophy — 'one lifelong customer conversation':");
    println!("    - No 'tickets', no 'cases' — one continuous conversation per customer for life");
    println!("    - When customer reaches out (regardless of channel or year), same thread continues");
    println!("    - 'People not tickets' — every agent screen is customer-centric, not ticket-centric");
    println!("    - Customers have a single profile with full history across all channels");
    println!("  Pricing: 'Hero' license $150/agent/mo (concurrent licenses — pay for simultaneous agents, not seats)");
    println!("         most competitors charge per named seat — Gladly's model favors shift-based teams");
    println!("         minimum typically $50K+/yr contracts, 25+ heroes");
    println!("         Sidekick AI add-on: usage-based (per resolution)");
    println!("  Channels (all unified into one Conversation):");
    println!("    - Voice (native — Gladly built its own IVR/cloud telephony, not a Twilio shell)");
    println!("    - Email, SMS, WhatsApp, FB Messenger, Instagram DM, X DM, Apple Business Chat");
    println!("    - Web chat, in-app chat");
    println!("    - Self-service Help Center");
    println!("  Sidekick AI (2024+):");
    println!("    - Autonomous AI agent — handles end-to-end conversations without a human");
    println!("    - Pulls from KB + order history + customer profile");
    println!("    - Can take actions (cancel order, refund, change shipping) via Sidekick Actions framework");
    println!("    - Pricing: per autonomous resolution (similar to Intercom Fin)");
    println!("    - Brand voice training — Sidekick learns tone from past human-handled threads");
    println!("  Liaison AI:");
    println!("    - Real-time agent assist — drafts replies, surfaces relevant KB articles");
    println!("    - Conversation summary on handoff between channels/agents");
    println!("    - Translation across 100+ languages, preserving brand voice");
    println!("  Agent features:");
    println!("    - 'Customer Profile' — order history, lifetime value, preferences, past conversations, NPS");
    println!("    - Channel-agnostic timeline (email/SMS/chat/voice mixed chronologically per customer)");
    println!("    - Hero Hub — agent workspace with simultaneous voice + chat handling");
    println!("    - Schedules + skills-based routing (skills tagged per agent)");
    println!("  Reporting:");
    println!("    - Agent productivity by channel mix");
    println!("    - Average Handle Time, First Contact Resolution, CSAT");
    println!("    - Real-time dashboards (queues, wait times, occupancy)");
    println!("    - Conversation insights with topic clustering (Liaison AI)");
    println!("  Voice features (rare among modern helpdesks):");
    println!("    - Built-in IVR with skill-based routing");
    println!("    - Call recording + post-call transcription");
    println!("    - Real-time agent assist during calls");
    println!("    - Callback queues (no waiting on hold)");
    println!("    - Cold + warm transfer; conference calls");
    println!("  Integrations: 50+ native");
    println!("              Shopify, Magento, BigCommerce, Salesforce CRM");
    println!("              Stripe, Recharge, Loop Returns, Klaviyo");
    println!("              REST API + webhooks + custom app framework");
    println!("  Customers: 150+ enterprise consumer brands");
    println!("            Crate & Barrel, JOANN, Warby Parker, Tradesy, Andie Swim, Allbirds, Bombas, FabFitFun");
    println!("            heavy DTC + retail + travel + nonprofit (Crisis Text Line)");
    println!("            sweet spot: 50-1,000 agent contact centers serving consumers");
    println!("  Critique: enterprise-priced — not approachable for SMB");
    println!("           concurrent license model great for shift work but confusing in evaluation");
    println!("           B2B/SaaS use cases harder fit (model assumes consumer-facing brand)");
    println!("           less name-brand awareness than Zendesk/Intercom");
    println!("           sales cycles long because requires re-thinking 'ticket' workflows");
    println!("  Differentiator: only major helpdesk natively built on customer-centric (not ticket-centric) data model with native voice");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gladly".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gladly(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gladly};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gladly"), "gladly");
        assert_eq!(basename(r"C:\bin\gladly.exe"), "gladly.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gladly.exe"), "gladly");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gladly(&["--help".to_string()], "gladly"), 0);
        assert_eq!(run_gladly(&["-h".to_string()], "gladly"), 0);
        let _ = run_gladly(&["--version".to_string()], "gladly");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gladly(&[], "gladly");
    }
}
