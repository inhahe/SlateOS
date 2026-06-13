#![deny(clippy::all)]

//! hotjar-cli — SlateOS Hotjar (the affordable heatmaps + session replay tool — now part of Contentsquare)
//!
//! Single personality: `hotjar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hotjar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hotjar [OPTIONS]");
        println!("Hotjar (Slate OS) — heatmaps + session recordings + surveys for websites");
        println!();
        println!("Options:");
        println!("  --observe-basic        Observe Basic FREE (35 sessions/day)");
        println!("  --observe-plus         Observe Plus $32/mo (100 sessions/day)");
        println!("  --observe-business     Observe Business from $80/mo (500 sessions/day+)");
        println!("  --observe-scale        Observe Scale from $171/mo (1,500+/day)");
        println!("  --ask-basic            Ask (surveys + feedback) — also has free tier");
        println!("  --engage               Engage (user interviews) — recruit users from Hotjar pool");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Hotjar 2024 (Slate OS)"); return 0; }
    println!("Hotjar 2024 (Slate OS)");
    println!("  Vendor: Hotjar Ltd. (Malta — now part of Contentsquare, acquired Sep 2021)");
    println!("        Hotjar incorporated in Malta for EU/AU favorable tax regime");
    println!("        Contentsquare paid ~$700M for Hotjar (cash + stock)");
    println!("  Founders: David Darmanin (CEO), Marc von Brockdorff, Erik Naess, 2014");
    println!("          David: ex-CRO consultant, founded Hotjar after years selling Crazy Egg integrations");
    println!("          all three Maltese — founded entirely remote-first from day 1");
    println!("  Founded: 2014 in Malta — bootstrapped to $50M ARR before acquisition");
    println!("          poster child for 'bootstrapped + remote + small market HQ' SaaS success");
    println!("  Acquisition timeline:");
    println!("    - Acquired by Contentsquare Sep 2021 for ~$700M");
    println!("    - Hotjar kept as a separate product brand (entry-level vs Contentsquare's enterprise tier)");
    println!("    - Strategy: Hotjar = SMB, Contentsquare = enterprise, Heap = product analytics");
    println!("    - combined entity ~$200M ARR, ~$1.4B valuation");
    println!("  Defining brand position: 'qualitative analytics for the rest of us':");
    println!("    - Drop simple snippet, get heatmaps + recordings + surveys");
    println!("    - Designed for the marketer/CRO/PM who can't justify $50K/yr FullStory");
    println!("    - Free tier is generous — many startups never upgrade past free");
    println!("    - Famously easy onboarding (5 minutes to value)");
    println!("  Pricing (much more transparent than enterprise competitors):");
    println!("    Observe Basic — FREE (35 sessions/day, 6-month retention)");
    println!("    Observe Plus — $32/mo (100 sessions/day, 12-month retention)");
    println!("    Observe Business — from $80/mo (500/day starter, scales by usage)");
    println!("    Observe Scale — from $171/mo (1,500/day starter, scales)");
    println!("    Ask + Engage products priced separately, similar tier structure");
    println!("    annual billing -20%");
    println!("  Observe (heatmaps + recordings):");
    println!("    - Click heatmaps (where users click on a page)");
    println!("    - Scroll heatmaps (how far down users scroll)");
    println!("    - Move heatmaps (cursor movement = proxy for attention)");
    println!("    - Session recordings (full DOM replay)");
    println!("    - Frustration signals: rage clicks, u-turns, error events");
    println!("    - Page filtering by URL + segment");
    println!("    - Cross-device + mobile recordings");
    println!("  Ask (surveys + feedback):");
    println!("    - On-site surveys (slide-in, popup, embedded)");
    println!("    - Multi-question + conditional logic");
    println!("    - Incoming feedback widget (smiley/sad face + comment)");
    println!("    - NPS surveys with trend tracking");
    println!("    - Templates library (PMs/UXers can launch surveys in 5 min)");
    println!("  Engage (formerly PingPong, acquired 2021):");
    println!("    - Recruit real users for moderated interviews");
    println!("    - Hotjar maintains a participant pool — pay-per-interview");
    println!("    - Integrated scheduling + Zoom/Teams + payment");
    println!("    - Compete with UserTesting + UserInterviews at lower entry price");
    println!("  AI features (recent):");
    println!("    - AI Survey Builder (describe what you want, AI drafts question set)");
    println!("    - Automated survey response summarization");
    println!("    - Insights — auto-surface themes from open-ended feedback");
    println!("  Integrations: 30+ apps");
    println!("              Google Analytics, Slack, Microsoft Teams");
    println!("              Optimizely + VWO (link A/B test to heatmap)");
    println!("              Segment as upstream");
    println!("              Webhooks + REST API");
    println!("  Customers: 1.3 million accounts (including free), ~$60M+ ARR");
    println!("            Adobe (parts), Hubspot, Trustpilot, Decathlon, Hotmart, Stack Overflow");
    println!("            heavy SMB + mid-market — strong in EU + LATAM + ANZ");
    println!("            sweet spot: any website team without $50K analytics budget");
    println!("  Critique: ceiling on session volume — recording costs add up");
    println!("           less enterprise-grade than FullStory (no Privacy/HIPAA hardening features)");
    println!("           Contentsquare era pushed price up modestly + introduced upsell pressure");
    println!("           limited compared to Heap on event analytics breadth");
    println!("           recordings retention windows tight at lower tiers");
    println!("  Differentiator: most accessible heatmaps + recordings tool — generous free tier + 5-min setup");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hotjar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hotjar(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hotjar};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hotjar"), "hotjar");
        assert_eq!(basename(r"C:\bin\hotjar.exe"), "hotjar.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hotjar.exe"), "hotjar");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hotjar(&["--help".to_string()], "hotjar"), 0);
        assert_eq!(run_hotjar(&["-h".to_string()], "hotjar"), 0);
        let _ = run_hotjar(&["--version".to_string()], "hotjar");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hotjar(&[], "hotjar");
    }
}
