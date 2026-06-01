#![deny(clippy::all)]
//! liveagent-cli — personality CLI for LiveAgent, the bootstrapped
//! all-channel help desk from Slovakia.
//!
//! Built by Quality Unit s.r.o. (founded 2004 in Bratislava, Slovakia by
//! Viktor Zeman + Andrej Harsani), LiveAgent grew out of the company's
//! earlier products (PostAffiliatePro affiliate-management, LiveChatPro,
//! Tonido). The defining commercial claim: the most help-desk features at
//! the lowest price in the category — chat, ticketing, call centre, social,
//! KB, gamification, all in one bundle, at a price band well under
//! Zendesk + Freshdesk equivalents. Fully bootstrapped, profitable, with
//! large user counts especially in EU + LATAM. Slow-and-steady operator
//! mindset rather than venture-scale growth story.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — LiveAgent bootstrapped Slovak help-desk personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Quality Unit s.r.o. 2004 Bratislava; bootstrapped");
    println!("    bundle        All-channel bundle: chat + tickets + call + social + KB");
    println!("    ticketing     Email + form ticketing + automation");
    println!("    chat          Live chat widget + proactive invitations");
    println!("    callcenter    Built-in VoIP call-centre module (SIP-based)");
    println!("    gamification  Badges + levels + leaderboards for agents");
    println!("    pricing       Flat per-agent-per-month tiered pricing");
    println!("    customers     SMB EU + LATAM customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("liveagent-cli 0.1.0 (bundle-help-desk personality build)"); }

fn run_about() {
    println!("LiveAgent (Quality Unit, LLC; product of Quality Unit s.r.o.).");
    println!("  Parent founded: 2004, Bratislava, Slovakia.");
    println!("  Founders:   Viktor Zeman + Andrej Harsani.");
    println!("  Other Quality Unit products: PostAffiliatePro (affiliate management),");
    println!("              LiveChatPro (precursor), Tonido.");
    println!("  Status:     bootstrapped, profitable.");
    println!("  Scale:      ~150,000 user companies historically claimed across product line.");
    println!("  Position:   maximum feature breadth at SMB-friendly pricing — explicit");
    println!("              'best-value all-in-one' positioning vs. Zendesk + Freshdesk.");
}

fn run_bundle() {
    println!("All-channel bundle.");
    println!("  Single LiveAgent SaaS tenant covers:");
    println!("    Email ticketing");
    println!("    Live chat widget");
    println!("    Call centre (VoIP / SIP)");
    println!("    Social: Facebook + Instagram + Twitter + WhatsApp + Viber");
    println!("    Customer-facing knowledge base + forum");
    println!("    Internal chat + customer-portal");
    println!("    Gamification module");
    println!("  The pitch: rather than paying Zendesk Suite + an add-on call centre +");
    println!("  a separate live-chat tool + a separate KB tool, get everything in one product.");
    println!("  Feature surface is genuinely large, though depth varies by module.");
}

fn run_ticketing() {
    println!("Ticketing.");
    println!("  Email-to-ticket via mailbox connection (IMAP / POP / forwarding / native");
    println!("  Gmail + Microsoft 365 OAuth).");
    println!("  Contact form widgets that create tickets directly.");
    println!("  Hybrid ticket stream: chats + calls + social messages all become tickets");
    println!("  in the same queue.");
    println!("  Automation rules: trigger + condition + action, plus time-based SLA rules.");
    println!("  Departments + tags + custom fields for routing.");
    println!("  Multi-brand: multiple branded portals on one tenant.");
}

fn run_chat() {
    println!("Live chat widget.");
    println!("  Configurable on-page widget with proactive invite rules (page X visited,");
    println!("  cart value over $Y, time on page over Z seconds).");
    println!("  'Real-time typing view' (controversial feature): agent can see what the");
    println!("  customer is typing before they press send — speeds reply preparation.");
    println!("  Pre-chat surveys + post-chat CSAT.");
    println!("  Mobile-app chat support + offline-message capture.");
    println!("  Co-browse + screen-share extensions.");
}

fn run_callcenter() {
    println!("Built-in VoIP call centre.");
    println!("  SIP-based: connect your existing telco trunk or use a LiveAgent-provided number.");
    println!("  IVR: configurable menu trees, queues, on-hold music, callbacks.");
    println!("  Call recording with retention policies + compliance considerations.");
    println!("  Voicemail-to-ticket conversion.");
    println!("  Call transfer + warm-transfer + supervisor whisper-monitoring.");
    println!("  Unusual to find a real telephony module bundled at this price band —");
    println!("  most competitors require a separate call-centre product (CallHub, Aircall, etc.).");
}

fn run_gamification() {
    println!("Gamification module.");
    println!("  Agent badges + experience-point levels + per-team leaderboards.");
    println!("  Configurable XP rules: 'first response under 2 minutes' = 10 XP,");
    println!("  'resolution with 5-star CSAT' = 25 XP, etc.");
    println!("  Distinctive carry-over from the company's earlier focus on motivating");
    println!("  affiliate marketers in PostAffiliatePro.");
    println!("  Customers either love it (BPO + outsourced-support teams) or ignore it");
    println!("  (more professionalised in-house support orgs) — niche differentiator either way.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free:     limited-usage free tier (single agent, basic ticketing + chat).");
    println!("  Small:    ~$15 per agent per month (ticket only).");
    println!("  Medium:   ~$29 per agent per month (ticket + chat).");
    println!("  Large:    ~$49 per agent per month (ticket + chat + call + social).");
    println!("  Enterprise: ~$69 per agent per month (all features + dedicated support).");
    println!("  Significantly below Zendesk / Freshdesk for equivalent feature scope.");
    println!("  No per-conversation or per-ticket fees on most plans.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: SMBs across EU + LATAM + Eastern Europe + India, 5-100 employees.");
    println!("  Heavy presence in regions where 'global' SaaS vendors are perceived as expensive;");
    println!("  LiveAgent's USD/EUR pricing converts more reasonably than Zendesk's.");
    println!("  Industries: e-commerce, telcos + ISPs, regional travel + hospitality,");
    println!("  utility companies, small BPOs / outsourced-support shops.");
    println!("  Common origin: customer searched for cheap Zendesk alternative + found");
    println!("  LiveAgent on a feature-comparison page or affiliate referral.");
    println!("  Affiliate program is unusually active given the parent company's");
    println!("  PostAffiliatePro DNA.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "liveagent-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "bundle" => run_bundle(),
        "ticketing" => run_ticketing(),
        "chat" => run_chat(),
        "callcenter" => run_callcenter(),
        "gamification" => run_gamification(),
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
        run_bundle();
        run_ticketing();
        run_chat();
        run_callcenter();
        run_gamification();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("liveagent-cli");
        print_version();
    }
}
