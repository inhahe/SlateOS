#![deny(clippy::all)]

//! sugarcrm-cli — OurOS SugarCRM (originally open-source CRM, now AI-driven enterprise)
//!
//! Single personality: `sugar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sugar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sugar [OPTIONS]");
        println!("SugarCRM (OurOS) — enterprise CRM with AI predictions");
        println!();
        println!("Options:");
        println!("  --sell                 Sugar Sell (Sales — pipeline + forecasting)");
        println!("  --serve                Sugar Serve (Customer Service — case mgmt)");
        println!("  --market               Sugar Market (Marketing automation, ex-Salesfusion)");
        println!("  --enterprise           Sugar Enterprise (on-prem deployment option)");
        println!("  --hint                 Sugar Hint (contact enrichment, ex-Collabspot)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SugarCRM 2024 (OurOS)"); return 0; }
    println!("SugarCRM 2024 (OurOS)");
    println!("  Vendor: SugarCRM Inc. (Cupertino, CA — private)");
    println!("  Founders: Clint Oram, John Roberts, Jacob Taylor (2004)");
    println!("          built as open-source alternative to Salesforce — Sugar Community Edition");
    println!("          GPLv3 commercial open core model in early 2000s");
    println!("  Founded: 2004, IPO planned 2014 but withdrew → bought by Accel-KKR Aug 2018");
    println!("  Ownership: Accel-KKR (PE) since 2018 — ~$300M+ ARR estimated");
    println!("            CEO: Craig Charlton (joined 2019, ex-Epicor)");
    println!("  Acquisitions under Accel-KKR:");
    println!("    - Salesfusion (marketing automation) Apr 2019 → Sugar Market");
    println!("    - Collabspot (productivity tools) Aug 2019 → Sugar Hint enrichment");
    println!("    - Loyaltyworks (loyalty programs) Dec 2020");
    println!("    - Node (predictive AI) Mar 2020 → embedded as 'SugarPredict'");
    println!("    - Augmento.ai (BI + analytics) Mar 2022 → embedded as 'SugarLive' insights");
    println!("  Strategy: 'time-aware' CRM — uses historical data + AI to predict outcomes");
    println!("           pivoted hard from open-source roots toward AI-first enterprise positioning");
    println!("  Pricing: Essentials $49/user/mo (3-user min)");
    println!("          Professional $80/user/mo");
    println!("          Advanced $135/user/mo (3-year contract typical)");
    println!("          Premier custom (enterprise)");
    println!("          all SKUs require annual contracts");
    println!("  Sugar Sell features:");
    println!("    - Account/Contact/Opportunity/Lead standard CRM model");
    println!("    - Forecasting with confidence intervals (SugarPredict AI)");
    println!("    - Deal scoring + likelihood-to-close predictions");
    println!("    - Renewal Console (Advanced+) — recurring revenue mgmt");
    println!("    - Geo-mapping of leads/accounts (built-in)");
    println!("  Sugar Serve features:");
    println!("    - Case management with SLA timers");
    println!("    - Self-service portal");
    println!("    - SugarBPM (business process automation)");
    println!("    - Knowledge base + customer journey timeline");
    println!("  Sugar Market features:");
    println!("    - Email campaigns + landing pages + forms");
    println!("    - Lead scoring + nurture journeys");
    println!("    - Marketing analytics with multi-touch attribution");
    println!("  Distinctive points:");
    println!("    - Still offers on-premise deployment (rare among modern CRMs)");
    println!("    - Sugar Community Edition (GPL) discontinued 2018, but Sugar still ships installer for on-prem");
    println!("    - Heavy investment in process automation (SugarBPM workflows)");
    println!("    - 'No-touch information management' — auto-fill records from email/calendar");
    println!("  Integrations: 100+ marketplace apps");
    println!("              QuickBooks, Slack, Office 365, Gmail, DocuSign, Mailchimp, RingCentral");
    println!("              SugarOutfitters partner ecosystem (legacy from open-source days)");
    println!("  Customers: mid-market enterprises (mostly 200-5000 employees)");
    println!("            biotech, manufacturing, financial services strong verticals");
    println!("            ~2 million users at ~6,500 companies worldwide");
    println!("  Critique: brand momentum faded post-IPO-withdrawal");
    println!("           AI features lag Salesforce Einstein in maturity");
    println!("           UI feels enterprise-heavy — not pretty");
    println!("           the 'open source CRM' legacy is mostly gone");
    println!("  Differentiator: only major CRM still offering true on-prem + heavy process automation focus");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sugar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sugar(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sugar};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sugarcrm"), "sugarcrm");
        assert_eq!(basename(r"C:\bin\sugarcrm.exe"), "sugarcrm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sugarcrm.exe"), "sugarcrm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sugar(&["--help".to_string()], "sugarcrm"), 0);
        assert_eq!(run_sugar(&["-h".to_string()], "sugarcrm"), 0);
        let _ = run_sugar(&["--version".to_string()], "sugarcrm");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sugar(&[], "sugarcrm");
    }
}
