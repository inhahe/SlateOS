#![deny(clippy::all)]
//! moosend-cli — personality CLI for Moosend, the Greek email marketing
//! platform now owned by Sitecore.
//!
//! Founded 2011 in Athens, Greece by Yiannis Psarras and Panos Melissaropoulos.
//! Moosend was bootstrapped + scrappy for the first eight years, building a
//! reputation as one of the cheapest credible ESPs in the price-sensitive
//! SMB tier — particularly in EMEA + APAC where pricing in dollars actually
//! mattered. Acquired by Sitecore in March 2021 to bolt an email marketing
//! capability onto Sitecore's digital experience platform. Has continued
//! to operate as a standalone product (moosend.com) under Sitecore ownership.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Moosend Greek SMB-affordable email personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Psarras+Melissaropoulos 2011 Athens; Sitecore 2021");
    println!("    campaigns     Email campaign editor + templates");
    println!("    automations   Recipe library + visual automations");
    println!("    landingpages  Hosted landing + subscription forms");
    println!("    audience      List management + segmentation + GDPR tools");
    println!("    sitecore      Integration into Sitecore's DXP stack");
    println!("    pricing       Aggressively cheap per-subscriber tiers");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("moosend-cli 0.1.0 (Greek-affordability personality build)"); }

fn run_about() {
    println!("Moosend (Moosend Ltd, now a Sitecore brand).");
    println!("  Founded:    2011, Athens, Greece.");
    println!("  Founders:   Yiannis Psarras, Panos Melissaropoulos.");
    println!("  Funding:    Bootstrapped through 2018; small later rounds.");
    println!("  Acquired:   March 2021 by Sitecore (terms undisclosed).");
    println!("  Posture:    SMB-affordable, EMEA + APAC strong base.");
    println!("  Operations: Continues as a standalone product post-acquisition.");
}

fn run_campaigns() {
    println!("Campaigns — drag-drop email editor.");
    println!("  Block-based editor with conditional content blocks.");
    println!("  Hundreds of pre-built responsive templates.");
    println!("  A/B testing: subject line, sender name, content variants.");
    println!("  Send-time optimisation per recipient based on past opens.");
    println!("  Spam-test preview against major inbox providers before send.");
}

fn run_automations() {
    println!("Automations — recipe library.");
    println!("  Pre-built 'recipes': welcome series, anniversary, cart-abandon,");
    println!("  re-engagement, post-purchase, lead nurture, birthday.");
    println!("  Each recipe is a starting workflow editable in canvas view.");
    println!("  Triggers: subscribed, opened, clicked, custom field changed,");
    println!("            site action via tracking pixel.");
    println!("  Filters + branches by tag, custom field, behaviour.");
}

fn run_landingpages() {
    println!("Landing pages + forms.");
    println!("  Drag-drop landing page builder hosted at *.moosend.page.");
    println!("  Subscription form builder: inline, popup, floating bar.");
    println!("  Form fields can write directly to custom fields on the contact.");
    println!("  Conversion analytics per form / per landing page.");
    println!("  GDPR-compliant double-opt-in option for EU senders.");
}

fn run_audience() {
    println!("Audience management.");
    println!("  Lists + segments + tags coexist; subscriber is the unit.");
    println!("  Static + dynamic segments (real-time recomputed).");
    println!("  Custom fields: text, number, date, multiselect.");
    println!("  GDPR consent capture + audit trail per contact.");
    println!("  CSV import + REST API + Zapier ingest paths.");
}

fn run_sitecore() {
    println!("Sitecore integration.");
    println!("  Moosend now sits in the Sitecore DXP marketing portfolio.");
    println!("  Sitecore CDP can sync segments to Moosend for sends.");
    println!("  Sitecore Personalize events can trigger Moosend automations.");
    println!("  Bidirectional contact + behaviour sync via Sitecore Connect.");
    println!("  Standalone Moosend.com remains available for SMB customers.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Pro            per-subscriber tier from ~$9/mo for 500 contacts.");
    println!("  Plus           adds landing pages + transactional emails.");
    println!("  Enterprise     dedicated IP, SSO, custom reporting, account team.");
    println!("  Free trial available; SMB-friendly pricing curve.");
    println!("  No charge for forms, landing pages, or list import.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  TEDx, WWF Greece, Vogue Greece, Domino's Greece, Gucci Beauty,");
    println!("  Sephora regional teams, Pizza Hut UK regional, several mid-market");
    println!("  EMEA retail brands.");
    println!("  Long tail of price-sensitive SMBs in Greece, India, Brazil, MENA.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "moosend-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "campaigns" => run_campaigns(),
        "automations" => run_automations(),
        "landingpages" => run_landingpages(),
        "audience" => run_audience(),
        "sitecore" => run_sitecore(),
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
        run_campaigns();
        run_automations();
        run_landingpages();
        run_audience();
        run_sitecore();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("moosend-cli");
        print_version();
    }
}
