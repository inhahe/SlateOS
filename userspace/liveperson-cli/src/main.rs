#![deny(clippy::all)]
//! liveperson-cli — personality CLI for LivePerson, the original live-chat
//! company turned conversational-AI vendor.
//!
//! Founded 1995 in New York by Robert LoCascio (CEO 1995-2022) — one of
//! the very first companies to put live chat on websites, predating the
//! entire SaaS era. Listed on Nasdaq:LPSN since 2000. For most of its
//! life the largest enterprise live-chat vendor, with deep telco, retail
//! and bank deployments. Pivoted hard to "Conversational AI" / "Conversational
//! Cloud" from 2017 on — bot-builder, NLU intent engine, agent assist, multi-channel
//! messaging. LoCascio departed Dec 2022 after a turbulent stretch (activist
//! investors, leadership shakeups). The company has been a turnaround story
//! since under new CEO John Sabino.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — LivePerson Conversational Cloud personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Robert LoCascio 1995 NYC; Nasdaq:LPSN 2000");
    println!("    livechat      The original 1995 product line still going");
    println!("    convai        Conversational Cloud + intent engine + bot studio");
    println!("    voice         Voice + IVR-replacement + speech-AI extensions");
    println!("    channels      WhatsApp, Apple Messages for Business, RCS, SMS, web chat");
    println!("    turnaround    2022-2024 leadership transition + restructuring");
    println!("    pricing       Enterprise contract pricing per conversation or seat");
    println!("    customers     Tier-1 telco + bank + retail customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("liveperson-cli 0.1.0 (conversational-cloud personality build)"); }

fn run_about() {
    println!("LivePerson, Inc. (Nasdaq:LPSN).");
    println!("  Founded:    1995, New York City.");
    println!("  Founder:    Robert LoCascio (CEO 1995-2022).");
    println!("  Listing:    Nasdaq:LPSN since 2000 IPO at $8/share.");
    println!("  Peak market cap: ~$5B 2021 at the conversational-AI hype peak;");
    println!("  significantly below that since 2022 reset.");
    println!("  Headcount:  ~1,200 (post-restructuring).");
    println!("  Core asset: 30 years of conversational data + enterprise integrations.");
    println!("  Current CEO: John Sabino (joined late 2023 from VillageMD).");
}

fn run_livechat() {
    println!("LiveEngage / live chat (the original 1995 product).");
    println!("  Web chat widget on hundreds of name-brand sites; many users have");
    println!("  unknowingly used it for decades.");
    println!("  Agent desktop with conversation queueing, transfer, supervisor monitoring.");
    println!("  Pre-chat surveys, post-chat CSAT, transcript export.");
    println!("  Co-browse + screen-share for assisted-checkout flows.");
    println!("  Survived the entire wave of newer competitors (Olark, Intercom, Drift,");
    println!("  Tidio) by being entrenched in regulated industries with hard procurement.");
}

fn run_convai() {
    println!("Conversational Cloud + AI.");
    println!("  Bot Studio: visual conversation-flow + intent designer.");
    println!("  Proprietary NLU engine trained on the company's accumulated conversation");
    println!("  corpus across customers (with opt-in + privacy controls).");
    println!("  Agent Assist: suggests responses, summarises long threads, drafts replies.");
    println!("  Voice + text combined: same intent model serves both channels.");
    println!("  Generative AI integration with OpenAI + in-house LLMs (LP Generative AI");
    println!("  framework launched 2023).");
    println!("  This is the bet that took the company from 'live chat' to 'conversational AI'.");
}

fn run_voice() {
    println!("Voice + IVR-replacement.");
    println!("  Acquired VoiceBase 2020 for speech recognition + analytics.");
    println!("  Voice AI: handles phone-channel conversations with the same intent model.");
    println!("  IVR-disambiguation use case: route a customer call to a chat session");
    println!("  ('we'll text you so you don't have to wait on hold') — major telco use case.");
    println!("  Real-time agent coaching during voice calls (whisper-mode suggestions).");
    println!("  Compliance: redaction of card numbers, SSNs, etc. in real time.");
}

fn run_channels() {
    println!("Channel coverage.");
    println!("  WhatsApp Business Platform (large carrier partner since early days).");
    println!("  Apple Messages for Business (one of the launch partners).");
    println!("  RCS (Rich Communication Services) for Google + carrier-backed messaging.");
    println!("  SMS via carrier-grade aggregator relationships.");
    println!("  Facebook Messenger + Instagram DM.");
    println!("  Web chat + in-app chat widgets.");
    println!("  Voice + IVR via VoiceBase-derived stack.");
    println!("  Email is supported but not the focus — LivePerson is a chat-first platform.");
}

fn run_turnaround() {
    println!("2022-2024 leadership transition + restructuring.");
    println!("  Late 2022: LoCascio departed amid board pressure and activist-investor");
    println!("  involvement (Vector Capital, Starboard Value publicly noted).");
    println!("  Multiple interim leadership phases through 2023.");
    println!("  Late 2023: John Sabino appointed CEO; restructuring focus on cash-flow");
    println!("  positivity + winding down underperforming product lines.");
    println!("  Acquired companies WildHealth + Kasamba spun off / divested.");
    println!("  Multiple rounds of staff reductions (>20%% headcount cuts).");
    println!("  Stock has been highly volatile; remains a turnaround thesis.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Enterprise contract pricing, not published self-serve.");
    println!("  Two main billing units:");
    println!("    per-conversation (Monthly Active Conversations / MACs) — chat + bot");
    println!("    per-seat (agent licences for the live-agent product).");
    println!("  Voice + Voice AI priced separately by minute / by conversation.");
    println!("  Generative-AI usage typically broken out as add-on.");
    println!("  Multi-year contracts the norm; deals typically six-to-seven-figure ACV.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Tier-1 telcos: T-Mobile, Vodafone, Telstra, BT — large historical accounts.");
    println!("  Major banks: HSBC, Bank of America (selective), several US regionals.");
    println!("  Big retail: The Home Depot, IKEA (selective), Macy's, Estée Lauder.");
    println!("  Airlines + travel: Delta, United (selectively), JetBlue.");
    println!("  Insurance + healthcare payers.");
    println!("  Typical: heavily-regulated industries with strict procurement that have");
    println!("  carried the LivePerson contract for 5-15 years and gradually layered on");
    println!("  the AI / messaging modules. Lower SMB/mid-market presence vs Zendesk + Intercom.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "liveperson-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "livechat" => run_livechat(),
        "convai" => run_convai(),
        "voice" => run_voice(),
        "channels" => run_channels(),
        "turnaround" => run_turnaround(),
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
        run_livechat();
        run_convai();
        run_voice();
        run_channels();
        run_turnaround();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("liveperson-cli");
        print_version();
    }
}
