#![deny(clippy::all)]

//! lark-cli — SlateOS Lark (ByteDance super-app: chat + docs + sheets + video + email)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lark(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lark [OPTIONS]");
        println!("Lark (SlateOS) — ByteDance's all-in-one work super-app (chat + docs + meetings + email + base)");
        println!();
        println!("Options:");
        println!("  --starter              Starter — free for up to 50 users");
        println!("  --pro                  Pro — $12/user/mo");
        println!("  --enterprise           Enterprise — custom ($25+/user/mo typical)");
        println!("  --feishu               Feishu — China-mainland version (same product, separate cloud)");
        println!("  --base                 Lark Base (Airtable-like database)");
        println!("  --minutes              Lark Minutes (auto-transcription + AI meeting notes)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Lark 2024 (SlateOS)"); return 0; }
    println!("Lark 2024 (SlateOS)");
    println!("  Vendor: ByteDance Ltd. (Beijing, China — private)");
    println!("  Original: Feishu, launched 2017 inside ByteDance to manage its own ~150K employees globally");
    println!("           ByteDance + Toutiao + Douyin + TikTok grew so fast Zhang Yiming wanted custom tooling");
    println!("           Lark = international version (different cloud, no PRC routing)");
    println!("  Founders: ByteDance (Zhang Yiming + early team), 2017 internally");
    println!("          spun out as standalone product to market internationally 2019");
    println!("          rebranded 'Feishu' for China, 'Lark' for international (especially APAC, SEA, EU)");
    println!("          run by Xie Xin (President of Lark / Feishu), reports up to ByteDance leadership");
    println!("  Founded: 2017 internally, 2019 commercially");
    println!("          part of ByteDance — no separate disclosed revenue, est. $100M+ ARR international + 10x in China");
    println!("          50M+ users on Lark + Feishu combined");
    println!("          tens of millions of users on Feishu in China alone");
    println!("  Strategic position: 'work super-app from the company that made TikTok':");
    println!("                    primary competitor: Slack + Microsoft 365 + Google Workspace + Zoom (each)");
    println!("                    aggressive in: SE Asia, India, Japan, Singapore, EU SMB");
    println!("                    in China: Feishu vs DingTalk (Alibaba) + WeCom (Tencent)");
    println!("                    pitch: 'one app for chat + docs + meetings + email + spreadsheets + database'");
    println!("                    enterprise IT geopolitical risk: ByteDance ownership creates concerns in US gov + defense");
    println!("                    distancing strategy: separate Lark Cloud (Singapore), no China data routing for international");
    println!("  Pricing (very aggressive — free 50-user tier, cheap Pro):");
    println!("    Starter — FREE for up to 50 users, includes ALL Lark features (Pro-tier features included on Starter)");
    println!("       this is unusually generous — most competitors free tier severely limited");
    println!("    Pro — $12/user/mo (50-500 users)");
    println!("    Enterprise — custom (>500 users, ~$25+/user/mo with full add-ons)");
    println!("       Approve add-on, OKR add-on, Field Service, Email custom domain extra");
    println!("    Lark is famously 'land-grab' priced — feature-rich free tier is the customer acquisition strategy");
    println!("  Core modules (the super-app pitch):");
    println!("    - Messenger (chat, channels, threads, 1:1 + group)");
    println!("    - Docs (Notion-like rich docs, collaborative)");
    println!("    - Sheets (Excel-like online spreadsheet, real collaboration)");
    println!("    - Slides (Google Slides equivalent)");
    println!("    - Meetings (Zoom-like video conferencing, 1,000+ participants)");
    println!("    - Mail (corporate email with own SMTP/IMAP + Lark UI)");
    println!("    - Calendar (group calendar, room booking, busy-detection)");
    println!("    - Base (Airtable-like database — Lark's secret weapon)");
    println!("    - Approval (workflow approvals for HR + expense + IT requests)");
    println!("    - OKR (Objectives + Key Results tracking — ByteDance is famous for OKR culture)");
    println!("    - Wiki (knowledge base)");
    println!("    - Field Service (mobile-first field worker tooling)");
    println!("    - Helpdesk (internal IT support tickets)");
    println!("  Lark Base (the killer feature):");
    println!("    - Airtable + Coda hybrid: relational database with multiple views");
    println!("    - Built-in BI charting from data");
    println!("    - Automations (when row added/changed → trigger)");
    println!("    - Custom forms → auto-populate Base");
    println!("    - Often used as: project trackers, CRM, asset trackers, HR pipelines");
    println!("    - Same UX as Airtable but bundled FREE with Lark Starter — undercuts Airtable's $$$ pricing");
    println!("  Lark Minutes (the AI killer):");
    println!("    - Auto-record + transcribe Lark Meetings in real time");
    println!("    - Generate AI meeting summary + action items in chat after meeting");
    println!("    - Search across all meetings by spoken phrase");
    println!("    - In multiple languages (Mandarin, English, Japanese, Korean, Indonesian)");
    println!("    - One of the strongest meeting-AI features of any platform");
    println!("  Approve (workflow engine):");
    println!("    - Build custom approval workflows for HR, expenses, leave, IT requests");
    println!("    - Conditional routing (if amount > $5K → CFO approval)");
    println!("    - Mobile-first signing");
    println!("    - Integrate with ERP / HRIS for data");
    println!("  OKR (built-in goal tracking):");
    println!("    - ByteDance is famous for using OKRs internally");
    println!("    - Set quarterly Objectives + Key Results");
    println!("    - Visualize tree of OKRs across org");
    println!("    - Direct competitor to: Lattice, 15Five (much lighter PMS)");
    println!("  Lark AI (2024 push):");
    println!("    - Auto-generate doc from prompt");
    println!("    - Smart reply suggestions in chat");
    println!("    - Translate messages in real-time (15+ languages)");
    println!("    - Meeting transcription + summarization (Minutes)");
    println!("    - Powered by ByteDance's Doubao LLM + others");
    println!("  Integrations: 200+ on Lark App Center");
    println!("              Salesforce, HubSpot, Workday, NetSuite, SAP");
    println!("              GitHub, GitLab, Jira");
    println!("              Zoom (alternative), Notion, Figma, Google Drive");
    println!("              Custom apps via Lark Open Platform (REST + webhooks)");
    println!("              Webhooks + Lark Bot Framework");
    println!("  Customers: 50M+ users on Lark + Feishu");
    println!("            Lark (international): NetEase, Soul (app), Rivian, Sokos, Lazada parts, ASEAN startups");
    println!("            Feishu (China-mainland): Xiaomi, ByteDance internal, OPPO, Vivo, Meituan");
    println!("            sweet spot: APAC + LATAM + EU mid-market SMB, especially companies seeking China alt");
    println!("            weak in: US enterprise (geopolitical concerns), US gov/defense (banned in some)");
    println!("  Critique: ByteDance ownership = US enterprise + government adoption blocked");
    println!("           perception of CCP data risk (despite separate Lark Cloud in Singapore)");
    println!("           feature breadth means UI density — learning curve for new users");
    println!("           AI features released slower internationally than in Feishu (China)");
    println!("           ecosystem (third-party apps) smaller than Slack or Microsoft 365");
    println!("           support for English+European hours sometimes lags Asia hours");
    println!("           rapid feature changes (no stable LTS — moves like a Chinese consumer app)");
    println!("  Differentiator: most-featured work super-app + extremely generous free tier (50 users, all features) + Lark Base bundled + Lark Minutes AI meeting transcription — the platform Asian unicorns + ByteDance-adjacent companies pick");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lark".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lark(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lark};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lark"), "lark");
        assert_eq!(basename(r"C:\bin\lark.exe"), "lark.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lark.exe"), "lark");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lark(&["--help".to_string()], "lark"), 0);
        assert_eq!(run_lark(&["-h".to_string()], "lark"), 0);
        let _ = run_lark(&["--version".to_string()], "lark");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lark(&[], "lark");
    }
}
