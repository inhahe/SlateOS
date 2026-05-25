#![deny(clippy::all)]
//! plivo-cli — OurOS personality CLI for Plivo, the value-CPaaS challenger.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Plivo — communications APIs that price like the SMS-bandwidth bill.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Venky Balasubramanian + Michael Ricordeau, YC W12");
    println!("    apis        SMS, MMS, Voice, Verify, WhatsApp, Premium SMS");
    println!("    contaque    Plivo's recent Contact Center push");
    println!("    sip         Zentrunk SIP trunking for VoIP carriers");
    println!("    pricing     The price-aggressive challenger pitch");
    println!("    networks    The Plivo SMS routing network and direct carrier ties");
    println!("    customers   IBM, Cisco, Adobe, Walmart Labs, Drift, others");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Twilio's API model, half the SMS price. The challenger-CPaaS.");
}

fn print_version() {
    println!("plivo-cli 0.1.0");
    println!("Plivo, Inc. — Austin, Texas (HQ) and Bengaluru. Founded 2011 (YC W12).");
}

fn cmd_about() {
    println!("Plivo");
    println!();
    println!("FOUNDED");
    println!("  2011 by Venky Balasubramanian and Michael Ricordeau. The");
    println!("  founders had built the open-source Plivo Framework on top of");
    println!("  FreeSWITCH starting around 2010, making it easier to write");
    println!("  voice applications without raw SIP work. They commercialized");
    println!("  the platform as a CPaaS through Y Combinator (Winter 2012)");
    println!("  alongside the API-economy wave that produced Twilio (a few");
    println!("  years earlier), Stripe (YC S09), and the Communications");
    println!("  Platform-as-a-Service category as a whole.");
    println!();
    println!("HEADQUARTERS");
    println!("  Austin, Texas. Engineering centers in Bengaluru (India) and");
    println!("  Sao Paulo (Brazil). ~200-300 employees.");
    println!();
    println!("POSITIONING");
    println!("  From the start, Plivo positioned itself as the price-aggressive");
    println!("  alternative to Twilio — same developer-friendly APIs, same");
    println!("  helper-library coverage, but lower per-message + per-minute");
    println!("  pricing driven by direct carrier interconnects and a smaller");
    println!("  organization. Targeted at high-volume SMS/voice senders");
    println!("  for whom margin matters more than every developer convenience.");
}

fn cmd_apis() {
    println!("Plivo APIs");
    println!();
    println!("SMS API");
    println!("  POST /v1/Account/<auth_id>/Message/  with src + dst + text.");
    println!("  Long-message segmentation auto-handled. Unicode + non-Unicode.");
    println!("  Delivery receipts via webhook. Inbound SMS via webhook on");
    println!("  the number's configured Application.");
    println!();
    println!("VOICE API");
    println!("  XML-based call flow ('PlivoXML'), structurally similar to");
    println!("  Twilio's TwiML. Verbs: Speak, Play, GetDigits, Record,");
    println!("  Dial (PSTN/SIP), Conference, Wait, Hangup, Redirect.");
    println!("  Outbound + inbound calls, IVR construction, conferencing.");
    println!();
    println!("VERIFY API");
    println!("  Hosted OTP flow for SMS + voice channels. Plivo manages the");
    println!("  code generation, delivery, retry logic, and verification.");
    println!("  Per-success or per-attempt pricing options.");
    println!();
    println!("WHATSAPP BUSINESS API");
    println!("  Officially sanctioned WhatsApp Business Solution Provider");
    println!("  (BSP). Same Message API shape; channel selected by routing");
    println!("  configuration on the phone number / template.");
    println!();
    println!("MMS API");
    println!("  US + Canada MMS for image, audio, video, vCard, PDF. ");
    println!("  Common use cases: marketing creative delivery, product");
    println!("  catalog images, identity-document collection.");
    println!();
    println!("PREMIUM SHORTCODE");
    println!("  Acquire dedicated US/UK shortcodes (5-6 digit) for marketing");
    println!("  + transactional volume. Carrier approval required; Plivo");
    println!("  manages the application process.");
}

fn cmd_contaque() {
    println!("Plivo CX (formerly Contaque) — contact center");
    println!();
    println!("WHAT IT IS");
    println!("  Plivo's contact-center-as-a-service offering, launched");
    println!("  initially in 2022 and expanded as Plivo CX in 2024.");
    println!("  Combines voice + SMS + WhatsApp + email in a unified agent");
    println!("  desktop. Targeted at SMB and mid-market customers underserved");
    println!("  by NICE / Genesys / Five9 enterprise contact centers.");
    println!();
    println!("FEATURES");
    println!("  - Omnichannel agent desktop");
    println!("  - Skills-based routing, queue management, IVR builder");
    println!("  - Recording + transcription + sentiment analysis");
    println!("  - Real-time + historical analytics dashboards");
    println!("  - CRM integrations: Salesforce, HubSpot, Zendesk, Freshworks");
    println!("  - Outbound dialer modes: power, predictive, progressive");
    println!();
    println!("WHY IT MATTERS");
    println!("  Plivo's existing CPaaS billing relationship with thousands of");
    println!("  developer-led customers gives a natural cross-sell path to");
    println!("  contact center as those customers grow. The same numbers,");
    println!("  same billing, same support; no need to integrate a separate");
    println!("  vendor for voice telephony underneath the contact center.");
}

fn cmd_sip() {
    println!("Zentrunk — Plivo's SIP trunking");
    println!();
    println!("WHAT IT IS");
    println!("  Carrier-grade SIP trunking that lets you bring your own PBX");
    println!("  or softphone and use Plivo as the SIP-to-PSTN bridge. Useful");
    println!("  when you have an existing on-prem Asterisk / FreeSWITCH /");
    println!("  3CX / Cisco / Avaya PBX and want to swap your incumbent CLEC");
    println!("  for cheaper, more flexible cloud-based termination.");
    println!();
    println!("FEATURES");
    println!("  - Encrypted SIP over TLS + sRTP");
    println!("  - Per-second billing (no per-minute rounding-up)");
    println!("  - E.164 + DID provisioning in ~80 countries");
    println!("  - Toll-free DIDs in US/Canada/UK");
    println!("  - Concurrent-call capacity scales elastically without quota");
    println!("    upgrades; you pay only for actual call-minutes used.");
    println!();
    println!("GEOGRAPHIC COVERAGE");
    println!("  Origin + termination supported in ~120 countries. Plivo runs");
    println!("  its own SIP gateways in US, Europe, Asia, plus reseller");
    println!("  relationships with regional Tier-1 carriers for last-mile");
    println!("  termination.");
}

fn cmd_pricing() {
    println!("Plivo pricing (approximate USD, 2024)");
    println!();
    println!("SMS (outbound, A2P)");
    println!("  US (local number)            $0.0055/segment");
    println!("  US (toll-free)               $0.0079/segment");
    println!("  US (shortcode)               $0.0050/segment + carrier fees");
    println!("  UK                           $0.038/segment");
    println!("  India                        $0.0060/segment + DLT fees");
    println!("  Compare Twilio US local: ~$0.0079/segment.");
    println!();
    println!("VOICE");
    println!("  US outbound                  $0.012/min");
    println!("  US inbound                   $0.0065/min");
    println!("  UK outbound                  $0.020/min");
    println!("  Per-second billing after first 60 seconds.");
    println!();
    println!("VIRTUAL NUMBERS");
    println!("  US local                     $0.80/month");
    println!("  US toll-free                 $1.00/month");
    println!("  UK / EU                      $1-3/month per country");
    println!();
    println!("VERIFY");
    println!("  Per-successful-verification  $0.05");
    println!("  Per-attempt option           $0.025 each");
    println!();
    println!("PRICING DIFFERENTIATOR");
    println!("  Plivo's headline pricing on US SMS is ~30% below Twilio. At");
    println!("  high SMS volumes (10M+/month), the per-message difference");
    println!("  funds whole engineering teams.");
}

fn cmd_networks() {
    println!("Plivo's routing network");
    println!();
    println!("DIRECT CARRIER INTERCONNECTS");
    println!("  Plivo operates SS7 + SIGTRAN + SMPP connections to major");
    println!("  US carriers (T-Mobile, AT&T, Verizon, US Cellular) and");
    println!("  international Tier-1 operators. Direct interconnects reduce");
    println!("  termination cost, improve delivery rates, and shorten the");
    println!("  latency between SMS submission and handset receipt.");
    println!();
    println!("INTERMEDIATE AGGREGATORS");
    println!("  For destinations where direct interconnect economics don't");
    println!("  pencil out, Plivo partners with regional aggregators (Sinch,");
    println!("  Tata, Twilio-on-Twilio, etc.). The Plivo platform routes");
    println!("  messages through the optimal path per destination.");
    println!();
    println!("LOOKUP + FRAUD CONTROL");
    println!("  Real-time number-type detection (mobile/landline/VoIP/wireless)");
    println!("  + carrier identification + ported-number lookups to prevent");
    println!("  pay-pumping, IRSF fraud, and rapid-fire spam patterns from");
    println!("  bleeding through to carriers.");
    println!();
    println!("DELIVERY RATE OPTIMIZATION");
    println!("  Plivo monitors per-route delivery rates and switches paths");
    println!("  in near-real-time when degraded routes appear. This is a");
    println!("  defining differentiator for high-volume A2P customers.");
}

fn cmd_customers() {
    println!("Selected Plivo customers");
    println!();
    println!("  IBM                — internal SMS + voice notification flows");
    println!("  Cisco              — selected Webex calling integrations");
    println!("  Adobe              — Marketo SMS delivery (high volume)");
    println!("  Walmart Labs       — operational notifications");
    println!("  Drift              — chat-to-SMS handoffs");
    println!("  Greenhouse         — recruiter SMS workflows");
    println!("  GoodRx             — prescription reminders");
    println!("  Yelp               — reservation reminder SMS");
    println!("  Instacart          — delivery alerts");
    println!("  Calm               — daily-reminder text outreach");
    println!();
    println!("Sweet spot: high-volume A2P SMS senders, healthcare appointment");
    println!("reminders, e-commerce transactional notifications, and product-");
    println!("led growth companies where margin per message is a meaningful");
    println!("line item in the P&L.");
}

fn run_plivo(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "apis" => { cmd_apis(); 0 }
        "contaque" => { cmd_contaque(); 0 }
        "sip" => { cmd_sip(); 0 }
        "pricing" => { cmd_pricing(); 0 }
        "networks" => { cmd_networks(); 0 }
        "customers" => { cmd_customers(); 0 }
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
        .unwrap_or_else(|| "plivo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_plivo(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/plivo"), "plivo");
        assert_eq!(basename("C:\\Tools\\plivo.exe"), "plivo.exe");
        assert_eq!(basename("plivo"), "plivo");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("plivo.exe"), "plivo");
        assert_eq!(strip_ext("plivo"), "plivo");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_plivo(&["help".to_string()], "plivo"), 0);
        assert_eq!(run_plivo(&[], "plivo"), 0);
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_plivo(&["nope".to_string()], "plivo"), 2);
    }
}
