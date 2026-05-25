#![deny(clippy::all)]
//! telesign-cli — personality CLI for TeleSign, the Marina-del-Rey identity
//! and fraud-prevention CPaaS owned by Belgian telco Proximus.
//!
//! Founded 2005 by Stacy Stubblefield and Ryan Disraeli in Marina del Rey, CA.
//! Pioneered phone-number-based identity scoring as a fraud and account-takeover
//! defence layer. Acquired by Belgacom (now Proximus) in 2017 for $230M. Tried
//! a SPAC merger with North Atlantic Acquisition Corp in 2021 (valuation
//! ~$1.3B) which was abandoned in 2022 due to market conditions.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — TeleSign identity + CPaaS personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, Marina del Rey, Proximus parent");
    println!("    products      Phone ID, Score, Verify, SMS, Voice");
    println!("    score         Phone-number reputation scoring");
    println!("    verify        SMS + voice OTP, silent network auth");
    println!("    intelligence  Data sources behind the risk model");
    println!("    proximus      Belgian telco parent and the abandoned SPAC");
    println!("    pricing       Per-lookup, per-verify, per-message");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("telesign-cli 0.1.0 (Proximus-era identity build)"); }

fn run_about() {
    println!("TeleSign Corporation");
    println!("  Founded:      2005");
    println!("  Founders:     Stacy Stubblefield, Ryan Disraeli");
    println!("  HQ:           Marina del Rey, California");
    println!("  Acquired:     by Belgacom (Proximus) in 2017 for $230M");
    println!("  Status:       Wholly-owned Proximus subsidiary.");
    println!("  Brand pitch:  Continuous Trust — phone-number-based identity");
    println!("                signals to fight fraud and account takeover.");
}

fn run_products() {
    println!("Product surface:");
    println!("  Phone ID         lookup: line type, carrier, country, MNP");
    println!("  Score            real-time risk score 0-1000");
    println!("  Verify           SMS OTP, voice OTP, push, silent");
    println!("  Messaging        SMS A2P + voice notifications");
    println!("  Voice Verify     callback-based voice OTP");
    println!("  AutoVerify       SMS Retriever API integration on Android");
    println!("  Intelligence     batch lookups for KYC and onboarding");
}

fn run_score() {
    println!("PhoneID Score — the flagship.");
    println!("  Input: a phone number.");
    println!("  Output: risk score 0-1000 + reason codes.");
    println!("  Signals: carrier velocity, recent porting, prepaid vs postpaid,");
    println!("           VoIP detection, association with known fraud patterns,");
    println!("           geographic anomalies, traffic-pumping markers.");
    println!("  Use case: gate signups + payouts on number reputation before");
    println!("            ever sending an OTP.");
}

fn run_verify() {
    println!("Verification stack:");
    println!("  SMS OTP             classic 6-digit code via SMS.");
    println!("  Voice OTP           text-to-speech dial-out fallback.");
    println!("  Silent Verify       mobile-network authentication, no UX.");
    println!("  Flash Call          incoming-call number is the OTP.");
    println!("  Push Verify         in-app push challenge.");
    println!("  SmartVerify         policy engine picks the best channel.");
}

fn run_intelligence() {
    println!("Intelligence sources behind the risk model:");
    println!("  Direct operator data via Proximus relationships");
    println!("  Aggregator data from hundreds of MNOs worldwide");
    println!("  Cross-customer behavioural signals (federated, not PII)");
    println!("  Public breach datasets cross-referenced by number");
    println!("  Velocity counters across the global TeleSign call/SMS network");
    println!("Result: claimed coverage of ~5 billion phone numbers, 200+ countries.");
}

fn run_proximus() {
    println!("Parent: Proximus Group (Brussels).");
    println!("  Belgian incumbent telco; Belgian state holds majority stake.");
    println!("  Houses TeleSign and BICS (international wholesale carrier).");
    println!("  Strategy: 'Global Communications' arm separate from Belgian retail.");
    println!("");
    println!("Abandoned SPAC: 2021 merger with North Atlantic Acquisition Corp");
    println!("  at ~$1.3B equity value was terminated in early 2022.");
    println!("  TeleSign remains private inside Proximus.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Phone ID lookup        per-call, tiered by volume.");
    println!("  Score                  per-score, sometimes bundled with verify.");
    println!("  Verify                 per successful verification.");
    println!("  SMS/Voice              per-message / per-minute, by country.");
    println!("  Enterprise contracts dominate; no public price list.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  Microsoft       account verification + fraud signal");
    println!("  TikTok          signup + login risk scoring");
    println!("  Salesforce      MFA verification");
    println!("  Uber            driver/rider onboarding");
    println!("  BBVA            banking onboarding fraud defence");
    println!("  Skype / Teams   account creation throttling");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "telesign-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "products" => run_products(),
        "score" => run_score(),
        "verify" => run_verify(),
        "intelligence" => run_intelligence(),
        "proximus" => run_proximus(),
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
        run_products();
        run_score();
        run_verify();
        run_intelligence();
        run_proximus();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("telesign-cli");
        print_version();
    }
}
