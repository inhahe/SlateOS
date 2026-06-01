#![deny(clippy::all)]
//! infobip-cli — personality CLI for Infobip, the Croatian global CPaaS giant.
//!
//! Founded 2006 by Silvio Kutic in Vodnjan, Croatia. Bootstrapped to scale
//! without external capital until a $200M One Equity Partners growth round
//! in 2020 (the largest VC round ever for a Croatian company at the time),
//! valuing Infobip above $1B and creating Croatia's first 'unicorn'. Famed
//! for direct operator integrations in places few competitors reach
//! (Africa, Middle East, parts of Asia). Acquired Shift conference (DevRel),
//! OpenMarket from Amdocs (US enterprise SMS) in 2021 for ~$300M, and Peerless
//! Network (US voice carrier) in 2024.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Infobip CPaaS personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about           Founder, Croatia, bootstrapped story");
    println!("    products        Moments, Conversations, Answers, People");
    println!("    channels        SMS, WhatsApp, Voice, Email, Viber, RCS, ...");
    println!("    reach           Direct operator footprint in 200+ countries");
    println!("    acquisitions    OpenMarket, Anam, Peerless Network");
    println!("    pricing         Per-message + per-conversation");
    println!("    customers       Selected named accounts");
    println!("    history         2006 founding to global scale");
    println!("    help            Show this help");
    println!("    version         Show version");
}

fn print_version() { println!("infobip-cli 0.1.0 (Vodnjan-to-the-world personality build)"); }

fn run_about() {
    println!("Infobip d.o.o.");
    println!("  Founded:     2006");
    println!("  Founder/CEO: Silvio Kutic");
    println!("  HQ:          Vodnjan, Istria, Croatia");
    println!("  Funding:     Bootstrapped through 2019.");
    println!("               $200M One Equity Partners growth, Jul 2020.");
    println!("               First Croatian unicorn (>$1B valuation).");
    println!("  Status:      Privately held, IPO long rumoured but deferred.");
    println!("  Headcount:   ~3,500 across 70+ offices globally.");
}

fn run_products() {
    println!("Product suite (own-brand, sold to enterprise marketing teams):");
    println!("  Moments         omnichannel marketing automation");
    println!("  Conversations   contact-center / agent inbox");
    println!("  Answers         chatbot builder and AI flows");
    println!("  People          customer data platform (CDP)");
    println!("  Signals         analytics + business intelligence");
    println!("  Communications  the underlying CPaaS API layer");
}

fn run_channels() {
    println!("Channels (one API, ~30+ messaging surfaces):");
    println!("  SMS         200+ countries via direct operator agreements");
    println!("  WhatsApp    BSP, template + session messaging");
    println!("  Voice       PSTN, SIP, programmable IVR, Voice Studio");
    println!("  Email       transactional + marketing");
    println!("  Viber       business messaging (huge in CEE + parts of Asia)");
    println!("  RCS        Google business messaging");
    println!("  Mobile App  push, in-app, geofencing");
    println!("  Live Chat   on-site web/app chat");
    println!("  Facebook    Messenger + Instagram DM");
    println!("  Telegram    bot + business platform");
    println!("  Apple Msgs  AMB enterprise channel");
    println!("  LINE        JP/TW reach");
    println!("  KakaoTalk   KR reach");
    println!("  WeChat      CN enterprise");
    println!("  Zalo        VN messaging");
}

fn run_reach() {
    println!("Operator footprint:");
    println!("  ~700 direct operator connections worldwide.");
    println!("  Particularly strong in CEE, Africa, Middle East, and");
    println!("  Southeast Asia — geographies where US-centric CPaaS");
    println!("  vendors typically rely on aggregator middlemen.");
    println!("  Result: better delivery rates and lower latency for");
    println!("  enterprise customers with global subscriber bases.");
}

fn run_acquisitions() {
    println!("Notable acquisitions:");
    println!("  OpenMarket       ~$300M  Nov 2020  US enterprise SMS, from Amdocs");
    println!("  Anam             undisc. 2018      SMS firewall / fraud protection");
    println!("  Dexatel          undisc. 2022      MENA messaging reach");
    println!("  Peerless Network undisc. 2024      US tier-1 voice carrier");
    println!("  Shift Conference (organic) DevRel/community brand");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Largely enterprise-sales-led, custom contracts.");
    println!("  Per-message and per-conversation tariffs by country/operator.");
    println!("  Application-layer products priced per contact or per seat.");
    println!("  Smaller self-serve tier exists but is not the focus.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  Daimler / Mercedes-Benz   connected-car notifications");
    println!("  Unilever                  global CPG campaigns");
    println!("  Costa Coffee              loyalty messaging");
    println!("  Burger King               local promotions");
    println!("  Uber                      driver/rider OTPs in EMEA");
    println!("  Microsoft                 partner / hyperscaler routing");
    println!("  ZDF / European broadcasters");
}

fn run_history() {
    println!("History highlights:");
    println!("  2006   Founded in Vodnjan, Croatia (population ~6,000).");
    println!("  ~2010  Builds direct operator agreements across CEE.");
    println!("  2014   Expands into Africa via direct telecom relationships.");
    println!("  2017   Adds OTT channels — Viber, WhatsApp, Facebook.");
    println!("  2020   One Equity Partners $200M growth round; unicorn status.");
    println!("  2020   Acquires OpenMarket from Amdocs (~$300M) for US scale.");
    println!("  2024   Acquires Peerless Network for owned US voice fabric.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "infobip-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "products" => run_products(),
        "channels" => run_channels(),
        "reach" => run_reach(),
        "acquisitions" => run_acquisitions(),
        "pricing" => run_pricing(),
        "customers" => run_customers(),
        "history" => run_history(),
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
        run_products();
        run_channels();
        run_reach();
        run_acquisitions();
        run_pricing();
        run_customers();
        run_history();
    }

    #[test]
    fn help_and_version() {
        print_help("infobip-cli");
        print_version();
    }
}
