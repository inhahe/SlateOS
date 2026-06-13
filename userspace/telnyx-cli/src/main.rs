#![deny(clippy::all)]
//! telnyx-cli — Slate OS personality CLI for Telnyx, the owned-network CPaaS.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Telnyx — communications APIs running on our own global network.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       David Casem, Ian Reither, Chicago HQ, bootstrap-to-scale");
    println!("    network     The private IP backbone differentiator");
    println!("    apis        SMS, Voice, Verify, WhatsApp, Mission Control");
    println!("    inference   Telnyx Inference — LLM API on the same fabric");
    println!("    storage     Telnyx Storage (S3-compatible object storage)");
    println!("    pricing     Pay-as-you-go + volume tiers");
    println!("    customers   The high-volume A2P SMS sender profile");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("From SIP trunking to AI: we own the fabric end-to-end.");
}

fn print_version() {
    println!("telnyx-cli 0.1.0");
    println!("Telnyx, LLC — Chicago, Illinois. Founded 2009.");
}

fn cmd_about() {
    println!("Telnyx");
    println!();
    println!("FOUNDED");
    println!("  2009 in Chicago by David Casem and Ian Reither. The two had");
    println!("  spent the prior decade in the telecom + VoIP industry, watching");
    println!("  CPaaS pioneers like Twilio build elegant developer APIs on");
    println!("  top of third-party carrier networks. Their bet: own the network,");
    println!("  not just the API layer, and the gross margin + reliability +");
    println!("  feature-development velocity all compound.");
    println!();
    println!("BOOTSTRAPPED");
    println!("  Telnyx grew without major venture funding for over a decade.");
    println!("  Building out a private fiber-and-Tier-1-interconnect global");
    println!("  network funded by gross profit. ~700 employees as of 2024,");
    println!("  meaningful annual recurring revenue, no announced public-");
    println!("  market intent.");
    println!();
    println!("HEADQUARTERS");
    println!("  Chicago, Illinois. Major engineering presences in Dublin,");
    println!("  London, Amsterdam, Sao Paulo, Singapore. Operations + NOC");
    println!("  staff in PoP cities globally.");
    println!();
    println!("FUNDING");
    println!("  Series B announced 2021 ($30M), Pelion Venture Partners +");
    println!("  Energy Impact Partners. Still relatively undercapitalized");
    println!("  compared to peers (Twilio raised ~$615M pre-IPO; Sinch is");
    println!("  publicly traded). The bootstrap-leaning posture is");
    println!("  intentional — Telnyx prizes founder control.");
}

fn cmd_network() {
    println!("The Telnyx network differentiator");
    println!();
    println!("THE THESIS");
    println!("  Most CPaaS providers (Twilio, Vonage Nexmo, Plivo) buy");
    println!("  termination capacity from upstream carriers + run their");
    println!("  software at the edge. Telnyx instead built its own:");
    println!("    - IP backbone interconnecting global PoPs over private fiber");
    println!("    - Direct SS7 + SIGTRAN + SMPP interconnects with carriers");
    println!("    - Tier 1 IP transit at multiple internet exchanges");
    println!("    - Private MPLS for inter-PoP traffic");
    println!("    - Edge SBCs (Session Border Controllers) at every PoP");
    println!();
    println!("WHAT IT BUYS YOU");
    println!("  - Lower latency for media + signaling between PoPs");
    println!("  - Better delivery rates (direct carrier termination beats");
    println!("    cascaded aggregators)");
    println!("  - Margins not eaten by intermediary aggregators");
    println!("  - Faster product iteration — no need to wait on upstream");
    println!("    carriers to support new features");
    println!();
    println!("GLOBAL POPS");
    println!("  ~30+ PoPs across North America, Europe, Asia, South America,");
    println!("  Oceania. Each PoP runs edge compute capable of serving voice,");
    println!("  SMS, and now inference workloads (see below).");
    println!();
    println!("THE NETWORK MATTERS FOR AI")
    ;
    println!("  The same private backbone that carries voice + SMS now");
    println!("  carries tokens between Telnyx Inference customers and");
    println!("  GPU clusters at PoP-adjacent data centers, giving low,");
    println!("  predictable latency for AI workloads.");
}

fn cmd_apis() {
    println!("Telnyx APIs");
    println!();
    println!("MESSAGING (SMS/MMS)");
    println!("  POST /v2/messages with from + to + text. Alphanumeric");
    println!("  sender IDs supported in non-US destinations; toll-free, local,");
    println!("  shortcode, 10DLC for US A2P. Auto-segmentation, Unicode,");
    println!("  delivery receipts. Messaging Profile groups numbers + rules.");
    println!();
    println!("VOICE")
    ;
    println!("  Call Control API (REST + webhooks) or Call Commands inside");
    println!("  a TeXML (Telnyx XML, TwiML-compatible) flow. Inbound, outbound,");
    println!("  SIP trunking, IVR, recording, transcription. Programmable");
    println!("  Voice SDKs for web, iOS, Android.");
    println!();
    println!("VERIFY");
    println!("  Phone-number verification via SMS, voice, RCS, Flashcall");
    println!("  (auto-disconnected call where the caller ID is the OTP).");
    println!("  Per-success pricing similar to Vonage Verify.");
    println!();
    println!("WIRELESS")
    ;
    println!("  IoT eSIM provisioning + cellular connectivity. Buy a Telnyx");
    println!("  SIM (eSIM or physical), bind it to an account, get usage");
    println!("  via API. Useful for IoT projects (fleet, sensors, kiosks).");
    println!();
    println!("FAX API");
    println!("  Yes — programmable fax is still a meaningful market,");
    println!("  especially in US healthcare. T.38 + G.711 fallback,");
    println!("  inbound + outbound, PDF in/out via API.");
    println!();
    println!("MISSION CONTROL")
    ;
    println!("  The unified portal where all of the above are configured,");
    println!("  numbers purchased, A2P registrations submitted, traffic");
    println!("  monitored, and reports run.");
}

fn cmd_inference() {
    println!("Telnyx Inference — LLM API on the Telnyx network");
    println!();
    println!("WHAT IT IS");
    println!("  An OpenAI-compatible LLM inference API hosted on GPU clusters");
    println!("  at Telnyx PoPs. Models supported include Llama 3, Mixtral,");
    println!("  Mistral, Whisper, and a handful of others.");
    println!();
    println!("WHY TELNYX");
    println!("  The pitch is twofold: (1) low latency to Telnyx's voice +");
    println!("  SMS endpoints means an end-to-end voice-AI agent loop");
    println!("  (transcribe -> LLM -> TTS -> outbound voice) has predictable");
    println!("  sub-second latency, all inside one vendor; (2) data residency");
    println!("  + privacy posture for regulated workloads (call recordings");
    println!("  + transcripts not shipped to third-party LLM hyperscalers).");
    println!();
    println!("API SHAPE");
    println!("  POST /v2/inference/chat/completions  OpenAI-compatible");
    println!("  POST /v2/inference/embeddings        OpenAI-compatible");
    println!("  POST /v2/inference/audio/transcriptions  Whisper-compatible");
    println!("  Drop-in replacement for OpenAI clients pointed at the");
    println!("  Telnyx endpoint with a Telnyx API key.");
    println!();
    println!("WHO USES IT");
    println!("  Companies building voice AI agents (sales, support, telephony");
    println!("  bots) where call-leg cost + LLM inference cost + speech");
    println!("  cost benefit from being co-located on the same network.");
}

fn cmd_storage() {
    println!("Telnyx Storage");
    println!();
    println!("WHAT IT IS");
    println!("  S3-API-compatible object storage hosted in Telnyx PoPs.");
    println!("  Standard S3 SDKs work unchanged with a Telnyx endpoint.");
    println!();
    println!("USE CASES");
    println!("  - Call recordings storage (replaces S3 buckets that would");
    println!("    otherwise sit in AWS at egress cost when Telnyx voice");
    println!("    drops in audio)");
    println!("  - Fax PDF storage with HIPAA-compliant residency");
    println!("  - Inference inputs/outputs storage near GPU compute");
    println!("  - Backups + cold tier for customer apps");
    println!();
    println!("PRICING");
    println!("  Storage:    ~$0.020/GB/month");
    println!("  Egress:     FREE within Telnyx network (to voice/SMS/inference)");
    println!("              Modest per-GB charge for Internet egress");
    println!("  Requests:   ~$0.0004/1K PUTs, ~$0.00004/1K GETs");
    println!("  Lower than AWS S3 standard egress for any meaningful volume;");
    println!("  vastly lower for in-network use cases.");
    println!();
    println!("WHY IT EXISTS");
    println!("  Egress charges across cloud boundaries (AWS S3 -> Telnyx PoP)");
    println!("  burned customer margins. Building in-network storage lets");
    println!("  Telnyx undercut S3 + capture the storage spend that was");
    println!("  previously going to AWS.");
}

fn cmd_pricing() {
    println!("Telnyx pricing (approximate USD, 2024)");
    println!();
    println!("MESSAGING (US)");
    println!("  Local 10DLC outbound        $0.004/segment + 10DLC carrier fees");
    println!("  Toll-free outbound          $0.0055/segment");
    println!("  Shortcode outbound          $0.0040/segment + program fees");
    println!("  US inbound                  $0.0040/segment");
    println!("  Among the lowest in the industry due to direct interconnects.");
    println!();
    println!("VOICE (US)");
    println!("  Outbound to US local        $0.0080/min");
    println!("  Inbound to US numbers       $0.0050/min");
    println!("  Per-second billing on calls > 60s.");
    println!();
    println!("NUMBERS");
    println!("  US local                    $1.00/mo");
    println!("  US toll-free                $1.50/mo");
    println!("  UK / EU local               $1-3/mo");
    println!();
    println!("WIRELESS (SIM/eSIM)");
    println!("  $2/mo per active SIM + per-MB data tiered by region.");
    println!();
    println!("INFERENCE")
    ;
    println!("  Per million input tokens + per million output tokens.");
    println!("  Pricing competitive with OpenAI API for equivalent models;");
    println!("  major savings for high-volume voice-AI workloads vs.");
    println!("  cross-region OpenAI + S3 + telephony stack.");
}

fn cmd_customers() {
    println!("Selected Telnyx customers (by workload profile)");
    println!();
    println!("  Mailchimp        — high-volume marketing SMS senders");
    println!("  Carvana          — purchase-process notification SMS");
    println!("  ZipRecruiter     — recruiter outreach SMS");
    println!("  Greenhouse       — candidate notification flows");
    println!("  Discord          — number verification on signup");
    println!("  Wise (TransferWise) — transaction-confirmation SMS");
    println!("  StreamYard       — broadcaster SMS reminders");
    println!("  Microsoft Cloud Marketplace — listed CPaaS partner");
    println!("  Various hospital systems — programmable fax + HIPAA voice");
    println!("  Various AI voice startups — Inference + Voice combined");
    println!();
    println!("Sweet spot: developers + product teams doing 1M+ SMS/month or");
    println!("running voice + AI workloads who care about per-message cost,");
    println!("end-to-end latency, and a single-vendor stack from carrier to");
    println!("LLM. Less competitive in the developer-onboarding-experience");
    println!("dimension where Twilio's docs + Quest gamification still lead.");
}

fn run_telnyx(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "network" => { cmd_network(); 0 }
        "apis" => { cmd_apis(); 0 }
        "inference" => { cmd_inference(); 0 }
        "storage" => { cmd_storage(); 0 }
        "pricing" => { cmd_pricing(); 0 }
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
        .unwrap_or_else(|| "telnyx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_telnyx(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/telnyx"), "telnyx");
        assert_eq!(basename("C:\\Tools\\telnyx.exe"), "telnyx.exe");
        assert_eq!(basename("telnyx"), "telnyx");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("telnyx.exe"), "telnyx");
        assert_eq!(strip_ext("telnyx"), "telnyx");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_telnyx(&["help".to_string()], "telnyx"), 0);
        let _ = run_telnyx(&[], "telnyx");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_telnyx(&["nope".to_string()], "telnyx"), 2);
    }
}
