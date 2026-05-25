#![deny(clippy::all)]
//! vonage-cli — OurOS personality CLI for Vonage, now an Ericsson Communications Platform.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Vonage — communications APIs and contact center, now part of Ericsson.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Founders, the VoIP-to-CPaaS pivot, Nexmo + TokBox lineage");
    println!("    apis        Messages, Voice, Verify, Video, Network APIs");
    println!("    business    UCaaS, Contact Center, Conversations");
    println!("    ericsson    The $6.2B Ericsson acquisition (July 2022)");
    println!("    network     The Ericsson 'Network APIs' bet via Aduna");
    println!("    pricing     Pay-as-you-go pricing in USD");
    println!("    history     2001 VoIP launch, IPO, post-IPO acquisitions");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("From dialing your couch to telecom-grade APIs.");
}

fn print_version() {
    println!("vonage-cli 0.1.0");
    println!("Vonage (a wholly owned subsidiary of Ericsson) — Holmdel, New Jersey. Founded 2001.");
}

fn cmd_about() {
    println!("Vonage");
    println!();
    println!("FOUNDED");
    println!("  2001 by Jeff Pulver and Jeffrey Citron as MIN-X (later renamed");
    println!("  Vonage) in Edison, NJ. Started as a consumer VoIP service —");
    println!("  the 'plug-in our small box and use the Internet for your home");
    println!("  phone' brand from the 2003-2008 TV-ad era. IPO'd on NYSE in");
    println!("  May 2006 (one of the more contentious IPOs of the decade due");
    println!("  to a directed-share-plan glitch).");
    println!();
    println!("THE PIVOT");
    println!("  Throughout the 2010s, consumer VoIP became a commodity (Google");
    println!("  Voice, Skype, mobile-first usage). Vonage repositioned as an");
    println!("  enterprise communications platform via three large acquisitions:");
    println!("    - Vocalocity (2013) -> UCaaS for SMBs (Vonage Business)");
    println!("    - Nexmo (Jun 2016, $230M) -> CPaaS APIs (SMS, voice, verify)");
    println!("    - TokBox (Aug 2018, $35M) -> WebRTC video (OpenTok)");
    println!("    - NewVoiceMedia (Nov 2018, $350M) -> contact center");
    println!();
    println!("  By 2020 consumer voice was a small declining segment; APIs +");
    println!("  UCaaS + Contact Center were the growth engines that made");
    println!("  Vonage attractive to Ericsson.");
    println!();
    println!("HEADQUARTERS");
    println!("  Holmdel, New Jersey. Engineering offices in London (Nexmo");
    println!("  legacy), San Francisco (TokBox legacy), Tel Aviv, Bangalore.");
}

fn cmd_apis() {
    println!("Vonage Communications APIs (legacy Nexmo)");
    println!();
    println!("MESSAGES API");
    println!("  Unified API across SMS, MMS, WhatsApp Business, Viber Business,");
    println!("  Facebook Messenger, RCS. One request shape, channel auto-");
    println!("  routed by the destination number's capabilities. Webhooks");
    println!("  for inbound, status, delivery receipts.");
    println!();
    println!("VOICE API");
    println!("  Programmable voice calls + IVR with NCCO (Nexmo Call Control");
    println!("  Object) — JSON-defined call flows. Talk, stream, record,");
    println!("  connect, conversation, input collection (DTMF + speech).");
    println!("  Outbound + inbound calls, SIP termination, PSTN, in-app");
    println!("  voice via mobile + web SDKs.");
    println!();
    println!("VERIFY API");
    println!("  Phone-number verification (SMS + voice OTP, WhatsApp OTP,");
    println!("  Silent Auth via mobile network data). Per-success pricing");
    println!("  is the key differentiator — you pay only when the user");
    println!("  successfully verifies, not for failed attempts.");
    println!();
    println!("VIDEO API (TokBox / OpenTok)");
    println!("  WebRTC-based video sessions, recording, archives. Used by");
    println!("  telehealth platforms, online education, gaming voice chat.");
    println!("  Multi-party sessions, screen sharing, real-time transcription.");
    println!();
    println!("NUMBER INSIGHT API");
    println!("  Phone number validation, type detection (mobile/landline/");
    println!("  VOIP), carrier lookup, MNP-aware routing, fraud signals.");
    println!("  Used heavily in onboarding flows to filter known bad numbers.");
}

fn cmd_business() {
    println!("Vonage Business — UCaaS, Contact Center, Conversations");
    println!();
    println!("VONAGE BUSINESS COMMUNICATIONS (VBC)");
    println!("  Cloud PBX + softphone + chat + meetings + SMS. Direct");
    println!("  competitor to RingCentral, 8x8, Zoom Phone, Dialpad. Sold");
    println!("  per-user / per-month with included PSTN minutes.");
    println!();
    println!("VONAGE CONTACT CENTER (VCC)");
    println!("  Cloud contact center, formerly NewVoiceMedia. Native");
    println!("  Salesforce integration is the standout feature — a deep");
    println!("  embed in Service Cloud + Sales Cloud workflows. Predictive");
    println!("  dialer, omnichannel (voice + SMS + email + chat), WFM,");
    println!("  QA + speech analytics.");
    println!();
    println!("CONVERSATIONS API");
    println!("  A stateful conversation context spanning channels (SMS thread");
    println!("  -> voice call -> WhatsApp -> in-app chat) tied to one user");
    println!("  identity. Used by SaaS apps that need cross-channel session");
    println!("  state without building it themselves.");
    println!();
    println!("MEETINGS")
    ;
    println!("  Video meetings (TokBox-derived) embeddable in the UCaaS");
    println!("  product. Smaller in scale than Zoom but native to the same");
    println!("  Vonage account + identity.");
}

fn cmd_ericsson() {
    println!("The Ericsson acquisition (2022)");
    println!();
    println!("ANNOUNCED");
    println!("  November 22, 2021. Ericsson agreed to acquire Vonage for");
    println!("  approximately USD 6.2 billion in cash. Closed July 21, 2022.");
    println!("  ~$21/share, a ~28% premium over the pre-announcement close.");
    println!();
    println!("RATIONALE");
    println!("  Ericsson saw three layered strategic plays:");
    println!("    1. Add a software/SaaS revenue stream to balance hardware-");
    println!("       cyclical RAN business.");
    println!("    2. Acquire the Vonage developer + CPaaS distribution channel");
    println!("       to expose 5G network capabilities to enterprise developers.");
    println!("    3. Build the 'Network API' marketplace concept — exposing");
    println!("       telco capabilities (location, identity, SIM swap detection,");
    println!("       quality-on-demand) as developer-callable APIs.");
    println!();
    println!("THE GOODWILL WRITE-DOWN");
    println!("  In Q3 2023, Ericsson announced a SEK 32 billion (~$2.9B)");
    println!("  goodwill impairment on the Vonage acquisition, reflecting");
    println!("  slower-than-expected synergy realization. The acquisition");
    println!("  remained strategically central to Ericsson; the write-down");
    println!("  was a market-driven valuation reset, not a divestiture signal.");
    println!();
    println!("INTEGRATION");
    println!("  Vonage operates as a wholly-owned Ericsson subsidiary,");
    println!("  retaining its brand, customer base, and developer relations.");
    println!("  Engineering organization is increasingly integrated with");
    println!("  Ericsson Cloud Software + Services unit.");
}

fn cmd_network() {
    println!("Network APIs — the Ericsson + Vonage long bet");
    println!();
    println!("THE THESIS");
    println!("  Mobile carriers possess unique data + capabilities (real-time");
    println!("  location, SIM-swap events, quality-of-service guarantees,");
    println!("  device identity, anti-fraud signals) that they have");
    println!("  historically been unable to expose to third-party developers");
    println!("  due to inconsistent APIs across operators. Expose those");
    println!("  capabilities via a developer-friendly common API layer and");
    println!("  you create a new monetization channel for operators.");
    println!();
    println!("CAMARA + GSMA OPEN GATEWAY");
    println!("  Industry-wide initiative driven by GSMA's Open Gateway and");
    println!("  the CAMARA open-source project (Linux Foundation). Vonage is");
    println!("  a key implementer + reseller of these standardized APIs.");
    println!();
    println!("ADUNA");
    println!("  September 2024: Vonage + Ericsson, alongside global telco");
    println!("  operators (AT&T, Bharti Airtel, KDDI, Orange, Reliance Jio,");
    println!("  Singtel, Telefonica, T-Mobile, Telenor, Verizon), announced");
    println!("  Aduna — a joint venture to aggregate and monetize network");
    println!("  APIs at scale, providing a single integration point for");
    println!("  developers across multiple carrier networks.");
    println!();
    println!("AVAILABLE APIS (2024)");
    println!("  - SIM Swap detection (anti-account-takeover signal)");
    println!("  - Number Verification (cross-carrier silent auth)");
    println!("  - Quality-on-Demand (request 5G slice for app session)");
    println!("  - Device Location (carrier-derived, more accurate than IP");
    println!("    geo, with operator-controlled consent)");
    println!("  - Device Status (online/offline carrier-level)");
}

fn cmd_pricing() {
    println!("Vonage pricing (CPaaS APIs, approximate USD, 2024)");
    println!();
    println!("MESSAGES (SMS)");
    println!("  US outbound       $0.0079/segment");
    println!("  UK outbound       $0.038/segment");
    println!("  India outbound    $0.0072/segment + telco fees");
    println!("  Inbound (per #)   $1-3/month for a virtual number");
    println!();
    println!("VOICE");
    println!("  US outbound       $0.0135/min");
    println!("  US inbound        $0.0045/min");
    println!("  UK outbound       $0.022/min");
    println!("  Per-minute pricing varies sharply by destination carrier.");
    println!();
    println!("VERIFY");
    println!("  SMS verify        $0.05 per successful verification");
    println!("  Voice verify      $0.05-0.15 per success");
    println!("  Silent auth       $0.005-0.02 per check");
    println!("  Pay-per-success model is the key competitive position vs.");
    println!("  Twilio Verify (which charges per attempt).");
    println!();
    println!("VIDEO");
    println!("  Routed sessions   $0.00475/min per published stream");
    println!("  Volume discounts kick in above ~50K minutes/month.");
}

fn cmd_history() {
    println!("Vonage timeline");
    println!();
    println!("  2001  Founded as MIN-X by Jeff Pulver + Jeffrey Citron.");
    println!("  2003  TV-advertising explosion ('Whoo-hoo!' jingle).");
    println!("  2006  IPO on NYSE under VG, directed-share-plan controversy.");
    println!("  2008  Founder Jeffrey Citron steps back; Marc Lefar CEO.");
    println!("  2013  Acquires Vocalocity ($130M cash + stock) -> UCaaS pivot.");
    println!("  2016  Acquires Nexmo ($230M) -> CPaaS pivot begins.");
    println!("  2017  Acquires Icertis-VoIP-partner Tokenex (security).");
    println!("  2018  Acquires NewVoiceMedia ($350M) -> contact center.");
    println!("  2018  Acquires TokBox ($35M) -> video API (OpenTok).");
    println!("  2020  CEO change: Rory Read takes over from Alan Masarek.");
    println!("  2021  Ericsson agrees to acquire ($6.2B).");
    println!("  2022  Ericsson acquisition closes (July).");
    println!("  2023  Ericsson goodwill impairment (~SEK 32B / ~USD 2.9B).");
    println!("  2024  Aduna JV launched (Sep) with 12 global operators.");
}

fn run_vonage(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "apis" => { cmd_apis(); 0 }
        "business" => { cmd_business(); 0 }
        "ericsson" => { cmd_ericsson(); 0 }
        "network" => { cmd_network(); 0 }
        "pricing" => { cmd_pricing(); 0 }
        "history" => { cmd_history(); 0 }
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
        .unwrap_or_else(|| "vonage".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_vonage(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/vonage"), "vonage");
        assert_eq!(basename("C:\\Tools\\vonage.exe"), "vonage.exe");
        assert_eq!(basename("vonage"), "vonage");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("vonage.exe"), "vonage");
        assert_eq!(strip_ext("vonage"), "vonage");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_vonage(&["help".to_string()], "vonage"), 0);
        assert_eq!(run_vonage(&[], "vonage"), 0);
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_vonage(&["nope".to_string()], "vonage"), 2);
    }
}
