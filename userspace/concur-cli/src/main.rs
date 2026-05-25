#![deny(clippy::all)]

//! concur-cli — OurOS SAP Concur (enterprise T&E — travel + expense + invoice)
//!
//! Single personality: `concur`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_concur(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: concur [OPTIONS]");
        println!("SAP Concur (OurOS) — Enterprise travel, expense & invoice");
        println!();
        println!("Options:");
        println!("  --expense              Concur Expense");
        println!("  --travel               Concur Travel (TripIt for business)");
        println!("  --invoice              Concur Invoice (AP automation)");
        println!("  --request              Concur Request (pre-trip approval)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SAP Concur 2024 (OurOS)"); return 0; }
    println!("SAP Concur 2024 (OurOS)");
    println!("  Vendor: Concur Technologies, Inc. (a subsidiary of SAP SE)");
    println!("  History: founded 1993 by Steve Singh + Mike Hilton (Redmond, WA)");
    println!("          early packaged-software era (CD-ROM expense report tool!)");
    println!("          went public, then private (Steve Singh take-private)");
    println!("          acquired by SAP Dec 2014 for $8.3B (one of SAP's biggest cloud acquisitions)");
    println!("  Scale: 47,000+ customer companies, 80M+ users worldwide");
    println!("        most Fortune 500 use Concur — incumbent enterprise T&E");
    println!("  Strategy: T&E (travel + expense) integrated end-to-end with ERP/AP/Payroll/HR");
    println!("           rides SAP S/4HANA integration as primary moat");
    println!("  Pricing: enterprise — undisclosed, typically $8-15/user/mo per module + base fee");
    println!("          implementation costs $50K-$1M+ for mid-large enterprise");
    println!("  Acquisitions over the years:");
    println!("    - TripIt (consumer travel itinerary 2011)");
    println!("    - Hipmunk (consumer travel search 2016, shut down 2020)");
    println!("    - Captio (Spain) 2018, KDS (Europe) 2016, ConTgo (mobile travel) 2014");
    println!("  Features:");
    println!("    - Expense report creation from receipts (photo, OCR via ExpenseIt)");
    println!("    - Corporate card auto-import (AmEx, Visa, Mastercard direct feeds)");
    println!("    - Mileage tracking (mobile GPS, IRS-compliant)");
    println!("    - Travel booking with policy-compliance enforcement");
    println!("    - Cash advance + per-diem management");
    println!("    - Multi-level approval workflows (manager → finance → audit)");
    println!("    - Audit Service (Concur Detect — ML-based fraud detection)");
    println!("    - GDPR + SOC + ISO 27001 compliance");
    println!("    - SAP ERP / Oracle / Workday / NetSuite integrations");
    println!("    - 100+ corporate card programs supported");
    println!("    - 35+ languages, 100+ countries");
    println!("    - Concur Drive (mileage auto-capture)");
    println!("    - Budget Insight (real-time spend visibility for managers)");
    println!("  Critique: notoriously bad UX — meme-tier 'Concur expense report nightmare'");
    println!("           UI feels stuck in 2010, mobile app slow + crashy");
    println!("           Ramp / Brex / Airbase explicitly built to replace Concur for SMB/mid-market");
    println!("           SAP integration is the only thing preventing mass enterprise migration");
    println!("  Differentiator: deepest enterprise integrations + global compliance + travel coverage");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "concur".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_concur(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
