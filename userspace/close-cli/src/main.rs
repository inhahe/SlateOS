#![deny(clippy::all)]

//! close-cli — OurOS Close (inside-sales CRM with built-in calling/SMS/email)
//!
//! Single personality: `close`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_close(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: close [OPTIONS]");
        println!("Close (OurOS) — inside-sales CRM with native dialer/SMS/email");
        println!();
        println!("Options:");
        println!("  --startup              Startup $59/user/mo (3 users included)");
        println!("  --professional         Professional $109/user/mo");
        println!("  --enterprise           Enterprise $149/user/mo");
        println!("  --call-coaching        Call coaching mode (silent listen + whisper)");
        println!("  --power-dialer         Power Dialer (auto-dial through a list)");
        println!("  --predictive-dialer    Predictive Dialer (Enterprise)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Close 2024 (OurOS)"); return 0; }
    println!("Close 2024 (OurOS)");
    println!("  Vendor: Close.io / Elastic Inc. dba Close (San Francisco — fully remote)");
    println!("  Founders: Steli Efti + Anthony Nemitz + Phil Freo + Thomas Steinacher (2013)");
    println!("          Steli: famous sales evangelist, blog 'Close Blog', podcast 'The Startup Chat'");
    println!("          founded with revenue from ElasticSales (B2B sales-as-a-service)");
    println!("  Founded: 2013 — bootstrapped + profitable; no VC funding (deliberately)");
    println!("          remote-first since day one — distributed across 30+ countries");
    println!("          ~$50M+ ARR (estimated, private)");
    println!("  Pricing: Startup $59/user/mo (3 users included, 2,500 lead cap)");
    println!("          Professional $109/user/mo (Power Dialer, advanced reporting)");
    println!("          Enterprise $149/user/mo (Predictive Dialer, custom roles, custom activities)");
    println!("          annual billing only at these rates; monthly +20%");
    println!("  Core thesis: 'CRMs for managers, Close is built for actual reps'");
    println!("            opinionated against bloated UI — minimize clicks per call/email");
    println!("  Calling features (the killer feature):");
    println!("    - Built-in dialer (Twilio under the hood, but transparent)");
    println!("    - One-click call from any phone number in any field");
    println!("    - Recording + transcription + auto-logging to lead record");
    println!("    - Local presence (call from a number matching the lead's area code)");
    println!("    - Power Dialer — auto-dials through a list, you talk when connected");
    println!("    - Predictive Dialer (Enterprise) — multi-line auto-dial");
    println!("    - Call Coaching — managers silently listen + 'whisper' to rep mid-call");
    println!("    - Voicemail drop — pre-recorded VM with one click");
    println!("    - Call quality scoring (AI, beta)");
    println!("  Email features:");
    println!("    - 2-way sync (Gmail, Outlook, any IMAP)");
    println!("    - Email sequences with auto-stop on reply");
    println!("    - Bulk email sends (with throttling for deliverability)");
    println!("    - Inbox view inside the CRM (don't leave to check email)");
    println!("    - Email open + link click tracking");
    println!("  SMS features (native — most CRMs require add-on):");
    println!("    - Send/receive SMS from lead record");
    println!("    - Bulk SMS campaigns");
    println!("    - SMS sequences");
    println!("  Workflows: Smart Views (saved filters) + automated 'next steps'");
    println!("            Custom Activities (track demos, contract reviews, etc.)");
    println!("            Opportunities pipeline (multiple pipelines per workspace)");
    println!("  Integrations: 100+ Zapier connectors + native: Gmail, Outlook, Twilio, Slack,");
    println!("              Zoom, Calendly, HubSpot Marketing, Mailchimp, Intercom, Drift, Segment");
    println!("              public REST API + webhooks");
    println!("  Customers: high-volume inside sales teams (SaaS, real estate, professional services)");
    println!("            sweet spot: 3-100 rep teams cold-calling/emailing daily");
    println!("            ~4,000+ paying customers");
    println!("            FreshBooks, Backerkit, MakeMusic, Lemonade (early-stage)");
    println!("  Critique: not a fit for complex enterprise sales (long cycles, many stakeholders)");
    println!("           customization limited vs Salesforce — by design (opinionated UX)");
    println!("           pricing higher than Pipedrive at entry, harder sell for tiny teams");
    println!("  Differentiator: only CRM where calling is a first-class citizen baked into the core UX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "close".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_close(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_close};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/close"), "close");
        assert_eq!(basename(r"C:\bin\close.exe"), "close.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("close.exe"), "close");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_close(&["--help".to_string()], "close"), 0);
        assert_eq!(run_close(&["-h".to_string()], "close"), 0);
        assert_eq!(run_close(&["--version".to_string()], "close"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_close(&[], "close"), 0);
    }
}
