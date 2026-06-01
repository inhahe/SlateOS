#![deny(clippy::all)]
//! messagebird-cli — personality CLI for MessageBird / Bird, the Amsterdam CPaaS.
//!
//! Founded 2011 by Robert Vis in Amsterdam, the Netherlands. Bootstrapped to
//! profitability before raising a $60M Series A from Accel and Atomico in 2017
//! at a reported >$700M valuation, followed by a $200M round in 2020 valuing
//! the company near $3B. Long known for European SMS reach and a developer
//! API rivalling Twilio in EMEA, MessageBird rebranded to "Bird" in 2023 as
//! part of a pivot toward omnichannel customer-engagement (CRM + marketing
//! automation) on top of its messaging fabric. Acquired SparkPost (email,
//! 2021) and Pusher (realtime, 2020).

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — MessageBird / Bird CPaaS personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about        Company history, founder, rebrand to Bird");
    println!("    channels     SMS, WhatsApp, Voice, Email, RCS, Telegram, Line");
    println!("    flow         Flow Builder — visual no-code orchestration");
    println!("    inbox        Unified omnichannel inbox for agents");
    println!("    crm          MessageBird CRM and marketing automation (post-2023)");
    println!("    acquisitions SparkPost, Pusher, 24sessions, Hull");
    println!("    pricing      Per-message and per-minute pricing model notes");
    println!("    customers    Uber, Heineken, SAP, Hugo Boss, Deliveroo");
    println!("    help         Show this help");
    println!("    version      Show version");
}

fn print_version() { println!("messagebird-cli 0.1.0 (Bird-era personality build)"); }

fn run_about() {
    println!("MessageBird / Bird");
    println!("  Founded:        2011 in Amsterdam, Netherlands");
    println!("  Founder/CEO:    Robert Vis");
    println!("  Rebrand:        MessageBird -> Bird (Feb 2023)");
    println!("  Funding:        Bootstrapped to profitability before VC.");
    println!("                  $60M Series A (Accel, Atomico) 2017.");
    println!("                  $200M Series C 2020 at ~$3B valuation.");
    println!("  Thesis:         Make every customer conversation programmable,");
    println!("                  across every channel a customer actually uses.");
    println!("  Why the rebrand: Vis argued CPaaS-as-pipes was commoditising;");
    println!("                  the real value sits in the applications built");
    println!("                  on top — CRM, marketing automation, inbox.");
}

fn run_channels() {
    println!("Channels (all behind a unified API):");
    println!("  SMS              global A2P, ~240 countries");
    println!("  WhatsApp         official BSP, template + session messaging");
    println!("  Voice            programmable PSTN + SIP trunking");
    println!("  Email            via SparkPost acquisition");
    println!("  RCS              Google-RCS business messaging");
    println!("  Telegram         bot platform integration");
    println!("  Line             JP/TW market reach");
    println!("  WeChat           CN enterprise messaging");
    println!("  Facebook Msgr    Meta Business Messaging");
    println!("  Apple Msgs Biz   AMB enterprise channel");
}

fn run_flow() {
    println!("Flow Builder — visual workflow engine.");
    println!("  Drag-and-drop nodes:  trigger, condition, send, wait,");
    println!("                        HTTP request, fork, AI completion.");
    println!("  Triggers:  inbound SMS/WA, webhook, schedule, form submit.");
    println!("  Use cases: 2FA, OTP, appointment reminders, drip campaigns,");
    println!("             escalation to human agents on keyword match.");
}

fn run_inbox() {
    println!("Inbox — unified agent workspace.");
    println!("  Threads from every channel merged into single conversation.");
    println!("  Routing rules, SLA timers, canned replies, AI suggestions.");
    println!("  Positioned against Intercom and Zendesk.");
}

fn run_crm() {
    println!("Bird CRM (the rebrand pivot).");
    println!("  Customer profiles aggregated across all channels + email.");
    println!("  Segmentation -> targeted broadcasts.");
    println!("  Journeys (multi-step campaigns) sit on Flow Builder.");
    println!("  Sells against Klaviyo and Braze on the marketing side,");
    println!("  against Twilio Segment + Engage on the data side.");
}

fn run_acquisitions() {
    println!("Notable acquisitions:");
    println!("  Pusher       (2020) — realtime channels / WebSockets.");
    println!("  SparkPost    (2021) — transactional email at scale.");
    println!("  24sessions   (2021) — video calling for support.");
    println!("  Hull         (2021) — customer data platform.");
    println!("These rolled up under the Bird brand in 2023.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  SMS         per message, by destination country.");
    println!("  WhatsApp    per conversation (Meta-defined 24h windows).");
    println!("  Voice       per minute, inbound and outbound separately.");
    println!("  Email       per email, tiered volumes via SparkPost.");
    println!("  CRM         per contact / per seat for the application layer.");
    println!("Pricing is published per-country on the Bird site.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  Uber          driver/rider notifications");
    println!("  Heineken      promotional campaigns");
    println!("  SAP           enterprise notifications");
    println!("  Hugo Boss     retail CRM");
    println!("  Deliveroo     order updates");
    println!("  Lufthansa     travel notifications");
    println!("  Domino's      order tracking");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "messagebird-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "channels" => run_channels(),
        "flow" => run_flow(),
        "inbox" => run_inbox(),
        "crm" => run_crm(),
        "acquisitions" => run_acquisitions(),
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
        run_channels();
        run_flow();
        run_inbox();
        run_crm();
        run_acquisitions();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("messagebird-cli");
        print_version();
    }
}
