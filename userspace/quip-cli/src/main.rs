#![deny(clippy::all)]

//! quip-cli — SlateOS Quip (Salesforce-owned docs+spreadsheets, deprecated 2025)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_quip(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: quip [OPTIONS]");
        println!("Quip (Slate OS) — collaborative docs + spreadsheets + chat (Salesforce-owned, end-of-life Jan 2025)");
        println!();
        println!("Options:");
        println!("  --starter              Starter — $10/user/mo (historical, legacy customers)");
        println!("  --plus                 Plus — $25/user/mo (legacy enterprise tier)");
        println!("  --advanced             Advanced — $100/user/mo (Salesforce-integrated)");
        println!("  --salesforce-anywhere  Quip embedded in Salesforce Lightning (live docs in CRM)");
        println!("  --eol                  END OF LIFE: Salesforce sunsetting Quip Jan 31 2025");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Quip 2024 (Slate OS) — EOL Jan 2025"); return 0; }
    println!("Quip 2024 (Slate OS) — END OF LIFE Jan 31 2025");
    println!("  Vendor: Salesforce, Inc. (San Francisco, CA — NYSE:CRM, acquired Quip 2016)");
    println!("  Original: Quip, Inc. — founded 2012 by Bret Taylor + Kevin Gibbs");
    println!("  Founders: Bret Taylor (CEO of Quip), Kevin Gibbs, 2012");
    println!("          Taylor was Facebook CTO 2009-2012, before that co-created Google Maps");
    println!("          went on to be Twitter board chair, CEO of Sierra (AI 2023), board member at OpenAI");
    println!("          Gibbs created Google Suggest, started App Engine at Google");
    println!("          one of the most decorated founder duos of 2010s");
    println!("  Founded: 2012 in Mountain View as 'Quip, Inc.'");
    println!("          raised ~$45M (Benchmark, Greylock, NEA)");
    println!("          acquired by Salesforce Aug 2016 for $750M (Taylor became Salesforce COO, then president, left 2022)");
    println!("          relaunched as 'Salesforce Anywhere' briefly (rebrand abandoned)");
    println!("          gradually deprioritized vs Slack (Salesforce acquired Slack Dec 2020 for $27.7B)");
    println!("          Salesforce announced END OF LIFE for Quip in 2024, full shutdown Jan 31 2025");
    println!("  Strategic position (was): 'docs + chat in one for sales teams in Salesforce':");
    println!("                    competitor at launch: Google Docs, Microsoft Office Online, Dropbox Paper, Notion (newer)");
    println!("                    Salesforce-pitched as: 'collaborative content layer inside CRM workflows'");
    println!("                    Salesforce mobile/desktop apps embedded Quip for account plans, opportunity briefs, internal wikis");
    println!("                    failed strategy: Quip never broke out of Salesforce ecosystem post-acquisition");
    println!("                    Slack acquisition + Microsoft 365 + Notion ate the use case Quip aimed to own");
    println!("  Pricing (historical, mostly legacy customers):");
    println!("    Starter — $10/user/mo (long-grandfathered, no new signups in late stage)");
    println!("    Plus — $25/user/mo (enterprise standalone — also no new signups by 2024)");
    println!("    Advanced — $100/user/mo (Salesforce-bundled tier)");
    println!("    new customers acquired 2023+ mostly within Salesforce Enterprise/UE bundles");
    println!("  Core features (the 'docs reimagined for mobile' pitch):");
    println!("    - Documents: collaborative rich-text docs with inline chat");
    println!("    - Spreadsheets: collaborative spreadsheets with formulas");
    println!("    - Per-document chat thread (chat lives alongside the doc, not separate)");
    println!("    - Mobile-first design (Quip's original UX strength)");
    println!("    - Offline mode (worked offline before Google Docs)");
    println!("    - Live Apps: embeddable widgets (Salesforce records, Jira tickets, polls, polls)");
    println!("    - Mentions + tasks inline in documents");
    println!("    - Folder hierarchy for organization");
    println!("    - Threaded inline comments");
    println!("    - Excellent diff history + version compare");
    println!("    - Markdown shortcuts");
    println!("  Salesforce integration (the main differentiator while alive):");
    println!("    - Embed Salesforce records in Quip docs that update live");
    println!("    - 'Quip for Salesforce' add-on for opportunity briefs, account plans");
    println!("    - Build templates pulling Account / Opportunity / Case data into living docs");
    println!("    - Edit Salesforce data directly from Quip cells");
    println!("    - this integration was Quip's main value-add to Salesforce customers");
    println!("  Mobile apps:");
    println!("    - Originally a mobile-first product");
    println!("    - iOS + Android apps with full editing + chat");
    println!("    - One of the better mobile doc-editing experiences of the 2010s");
    println!("    - Native macOS + Windows desktop apps");
    println!("  Live Apps (Quip's plug-in framework):");
    println!("    - Calendar Live App, Salesforce Record Live App, Project Tracker Live App");
    println!("    - Process Bar, Countdown Timer, Survey, Code Block");
    println!("    - JSON-driven embeddable interactive widgets");
    println!("    - Developers could build custom Live Apps for company-specific use cases");
    println!("  Why it failed (the post-mortem):");
    println!("    - Salesforce never integrated Quip deeply enough into CRM workflows beyond static templates");
    println!("    - Slack acquisition (2020) overlapped Quip's chat function — Salesforce had no incentive to invest");
    println!("    - Notion + Coda + ClickUp + Microsoft Loop offered better modern UX");
    println!("    - Founder Bret Taylor left Salesforce 2022, removing internal champion");
    println!("    - Quip received minimal engineering investment 2021-2024");
    println!("    - Net Notion bookings 2024 > 'all of Quip ever'");
    println!("    - End-of-life announcement Apr 2024, full shutdown Jan 31 2025");
    println!("  Migration path (the wind-down):");
    println!("    - Salesforce providing CSV/PDF export tools for Quip data");
    println!("    - Recommended targets: Salesforce Files, Slack Canvas (acquired internally), Notion, Confluence");
    println!("    - Migration deadline strict: data inaccessible after Jan 31 2025");
    println!("  Lessons for the industry:");
    println!("    - Notable failure of post-acquisition product strategy (vs Slack at Salesforce which thrived)");
    println!("    - Confirmed that 'docs inside CRM' is a feature, not a product (CRM-native docs in Salesforce Lightning largely replace Quip)");
    println!("    - The acquired-team-founder-leaves pattern: when Bret Taylor left, Quip lost its champion");
    println!("    - Microsoft Loop + Notion + Slack Canvas all converging on what Quip pioneered (chat + docs together) — but better");
    println!("  Differentiator (historically): chat-inside-docs UX years ahead of competitors + best mobile editing of the 2010s + Salesforce live data embeds — pioneering work whose category got commoditized while Salesforce focused on Slack");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "quip".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_quip(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_quip};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/quip"), "quip");
        assert_eq!(basename(r"C:\bin\quip.exe"), "quip.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("quip.exe"), "quip");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_quip(&["--help".to_string()], "quip"), 0);
        assert_eq!(run_quip(&["-h".to_string()], "quip"), 0);
        let _ = run_quip(&["--version".to_string()], "quip");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_quip(&[], "quip");
    }
}
