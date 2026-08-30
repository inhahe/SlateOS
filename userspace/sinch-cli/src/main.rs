#![deny(clippy::all)]
//! sinch-cli — personality CLI for Sinch AB, the Swedish CPaaS public company.
//!
//! Sinch AB (publ) — Stockholm-headquartered, listed on Nasdaq Stockholm
//! (SINCH). Founded 2008 as Rebtel spin-off, formally Sinch since 2014.
//! Grew through an aggressive acquisition strategy: Inteliquent (US voice,
//! 2021, $1.14B), Pathwire/Mailgun + Mailjet (email, 2021, $1.9B), MessageMedia
//! (APAC SMS, 2021, $1.3B), Wavy (LATAM, 2020), SAP Digital Interconnect (2021).
//! One of very few CPaaS vendors with majority of revenue outside the US.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Sinch AB personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about           Company, listing, geography");
    println!("    products        Messaging, Voice, Email, Verification");
    println!("    acquisitions    Inteliquent, Pathwire, MessageMedia, Wavy, SAP DI");
    println!("    network         Tier-1 voice + direct operator connections");
    println!("    super-network   Sinch's term for its global reach fabric");
    println!("    pricing         Per-message, per-minute, per-verify");
    println!("    financials      Listing, scale, currency");
    println!("    customers       Selected named accounts");
    println!("    help            Show this help");
    println!("    version         Show version");
}

fn print_version() { println!("sinch-cli 0.1.0 (Super-Network personality build)"); }

fn run_about() {
    println!("Sinch AB (publ)");
    println!("  HQ:        Stockholm, Sweden");
    println!("  Listing:   Nasdaq Stockholm (SINCH)");
    println!("  Founded:   2008 (as Rebtel spin-off), Sinch brand 2014");
    println!("  Founders:  Andreas Bernstrom and Bjorn Zethraeus origins;");
    println!("             current CEO Laurinda Pang (from 2023).");
    println!("  Geography: Majority of revenue from outside the US — rare for");
    println!("             a public CPaaS. EMEA + APAC strong.");
    println!("  Headcount: ~3,500 across 60+ offices worldwide.");
}

fn run_products() {
    println!("Product portfolio:");
    println!("  Messaging      SMS, MMS, RCS, WhatsApp, Viber, Instagram,");
    println!("                 Facebook Messenger, KakaoTalk, Line, WeChat.");
    println!("  Voice          PSTN termination/origination via Inteliquent.");
    println!("                 Programmable SIP, conferencing, SBCs.");
    println!("  Email          Mailgun (transactional) + Mailjet (marketing).");
    println!("  Verification   SMS OTP, flash call, instant verification,");
    println!("                 silent network authentication (mobile carrier).");
    println!("  Chatlayer      Conversational AI / chatbot platform.");
    println!("  Contact Pro    Cloud contact center on top of voice + msg.");
}

fn run_acquisitions() {
    println!("Acquisition rollup (2018-2022, ~$5B deployed):");
    println!("  Vehicle           ~$1.14B   Inteliquent      Feb 2021  US voice");
    println!("  Pathwire          ~$1.9B    Mailgun+Mailjet  Nov 2021  Email");
    println!("  MessageMedia      ~$1.3B    APAC SMS         Jun 2021  SMS APAC");
    println!("  SAP Digital Intc  ~$0.225B  Carrier msg      Nov 2020");
    println!("  Wavy              ~$0.119B  LATAM            Feb 2020");
    println!("  ACL Mobile        ~$0.07B   India SMS        Feb 2021");
    println!("  TWW               ~$0.022B  Brazil SMS       Feb 2021");
    println!("  Chatlayer         (sm)      Belgium CAI      May 2021");
}

fn run_network() {
    println!("Voice network (via Inteliquent):");
    println!("  Tier-1 US voice carrier with ~70% of US enterprise voice traffic");
    println!("  reportedly touching its network at some point. Direct connects");
    println!("  to every major US carrier; STIR/SHAKEN attestation in place.");
}

fn run_super_network() {
    println!("'Super Network' — Sinch's branding for its global reach.");
    println!("  600+ direct operator connections for SMS termination.");
    println!("  Tier-1 voice peering in NA, EMEA, APAC, LATAM.");
    println!("  ~150 PoPs for email delivery (post-Pathwire integration).");
    println!("  Pitch: one contract, one API, billions of endpoints reachable.");
}

fn run_pricing() {
    println!("Pricing model (Sinch publishes country price lists):");
    println!("  SMS           per-message, by destination MCC/MNC.");
    println!("  Voice         per-minute, inbound vs outbound, by country.");
    println!("  Email         tiered monthly bands via Mailgun/Mailjet.");
    println!("  Verification  per successful verify, flash-call discount.");
    println!("Enterprise contracts dominate over self-serve.");
}

fn run_financials() {
    println!("Financials (public filings):");
    println!("  Revenue:       ~SEK 26-28B annualised (post-acquisitions).");
    println!("  Gross margin:  thin in voice/SMS, healthier in email + verify.");
    println!("  Listing:       Nasdaq Stockholm Large Cap (since 2020 move-up).");
    println!("  Currency:      Reports in SEK; revenue is multi-currency.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  Google         OTP and notifications");
    println!("  Microsoft      Teams + Azure Communication");
    println!("  Uber           rider/driver verification");
    println!("  Booking.com    booking notifications");
    println!("  Netflix        account verification");
    println!("  Visa           transaction alerts");
    println!("  Klarna         payment confirmations");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "sinch-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "products" => run_products(),
        "acquisitions" => run_acquisitions(),
        "network" => run_network(),
        "super-network" => run_super_network(),
        "pricing" => run_pricing(),
        "financials" => run_financials(),
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
        run_products();
        run_acquisitions();
        run_network();
        run_super_network();
        run_pricing();
        run_financials();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("sinch-cli");
        print_version();
    }
}
