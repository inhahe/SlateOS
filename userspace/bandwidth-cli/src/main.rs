#![deny(clippy::all)]
//! bandwidth-cli — personality CLI for Bandwidth Inc., the Raleigh-NC CPaaS
//! that actually owns its own nationwide IP voice network.
//!
//! Founded 1999 by Henry Kaestner and David Morken in Raleigh, NC. IPO'd
//! Nov 2017 on NASDAQ as BAND. Unique in the CPaaS space for being a
//! CLEC-licensed carrier in all 50 US states with its own IP voice network,
//! direct PSTN interconnections, and direct-to-carrier SMS routing. Twilio
//! ran on Bandwidth's network for years before Twilio built its own.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Bandwidth Inc. personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about       Founders, listing, Raleigh roots");
    println!("    network     The owned IP voice network — CLEC in 50 states");
    println!("    apis        Voice, Messaging, Emergency Services, Numbers");
    println!("    duet        Bandwidth Duet — Teams Phone integration");
    println!("    maestro     Application orchestration layer");
    println!("    e911        Dynamic E911 + Kari's Law compliance");
    println!("    customers   Google, Microsoft, Zoom, RingCentral, Cisco");
    println!("    history     Twilio-era anchor tenant, IPO, growth");
    println!("    help        Show this help");
    println!("    version     Show version");
}

fn print_version() { println!("bandwidth-cli 0.1.0 (BAND-on-NASDAQ personality build)"); }

fn run_about() {
    println!("Bandwidth Inc. (NASDAQ: BAND)");
    println!("  Founded:    1999");
    println!("  Founders:   Henry Kaestner, David Morken");
    println!("  HQ:         Raleigh, North Carolina");
    println!("  IPO:        Nov 10 2017, IPO price $20, NASDAQ:BAND");
    println!("  Subsidiary: 2bandwidth.com origin -> spun out Republic Wireless");
    println!("              (sold to DISH 2021); core Bandwidth remained.");
    println!("  Ethos:      'Whole-Person' wellness program, faith-rooted culture.");
}

fn run_network() {
    println!("The owned IP voice network is the differentiator.");
    println!("  CLEC license in all 50 US states.");
    println!("  Direct PSTN interconnections with every major US carrier.");
    println!("  Own SIP softswitches and SBCs across multiple data centers.");
    println!("  Direct-routed SMS to all major US wireless carriers.");
    println!("  Result: no middleman markup, full call-quality control,");
    println!("          STIR/SHAKEN attestation as the originating carrier,");
    println!("          dynamic E911 with first-responder routing.");
}

fn run_apis() {
    println!("API surface:");
    println!("  Voice API           outbound calls, IVR, transcription,");
    println!("                      conferencing, machine detection.");
    println!("  Messaging API       SMS, MMS, group messaging, toll-free,");
    println!("                      short codes, 10DLC compliance.");
    println!("  Emergency Services  E911 provisioning, dynamic location.");
    println!("  Numbers API         search, purchase, port-in, port-out,");
    println!("                      local + toll-free + international.");
    println!("  Multi-Channel       WhatsApp, RCS, MMS group, video links.");
}

fn run_duet() {
    println!("Bandwidth Duet for Microsoft Teams.");
    println!("  Direct Routing PSTN connectivity into Microsoft Teams Phone.");
    println!("  Bandwidth provides numbers, calling minutes, emergency services");
    println!("  while Teams provides the client experience.");
    println!("  Alternative to Microsoft Calling Plans — usually cheaper and");
    println!("  with more porting/E911 flexibility for enterprise.");
}

fn run_maestro() {
    println!("Bandwidth Maestro — no/low-code orchestration.");
    println!("  Visual workflow designer for voice + messaging apps.");
    println!("  Pre-built building blocks: transcribe, dial, conference, SMS.");
    println!("  Targets teams that want CPaaS power without writing code.");
}

fn run_e911() {
    println!("E911 / RAY BAUM's Act / Kari's Law compliance.");
    println!("  Dynamic location updates as endpoints move (softphones).");
    println!("  Direct trunks to Public Safety Answering Points (PSAPs).");
    println!("  Bandwidth is one of very few CPaaS vendors to operate its");
    println!("  own E911 routing and is a preferred provider for UCaaS");
    println!("  vendors needing US compliance.");
}

fn run_customers() {
    println!("Selected customers (UCaaS / hyperscaler-grade):");
    println!("  Google           Google Voice underlying PSTN");
    println!("  Microsoft        Teams Operator Connect partner");
    println!("  Zoom             Zoom Phone international + US numbers");
    println!("  RingCentral      number provisioning + voice");
    println!("  Cisco            Webex Calling");
    println!("  Pinger / Textfree consumer SMS app");
    println!("  Republic Wireless WiFi-first MVNO (former subsidiary)");
}

fn run_history() {
    println!("History highlights:");
    println!("  1999    Founded as a CLEC reseller in Raleigh.");
    println!("  ~2007   Twilio launches on Bandwidth's network as anchor tenant.");
    println!("  2014    Spins out Republic Wireless as separate consumer brand.");
    println!("  2017    IPO at $20 on NASDAQ.");
    println!("  2020    COVID-era voice + messaging surge.");
    println!("  2021    Sells Republic Wireless to DISH; focuses on enterprise.");
    println!("  2022    Microsoft Teams Operator Connect launch partner.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "bandwidth-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "network" => run_network(),
        "apis" => run_apis(),
        "duet" => run_duet(),
        "maestro" => run_maestro(),
        "e911" => run_e911(),
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
        run_network();
        run_apis();
        run_duet();
        run_maestro();
        run_e911();
        run_customers();
        run_history();
    }

    #[test]
    fn help_and_version() {
        print_help("bandwidth-cli");
        print_version();
    }
}
