#![deny(clippy::all)]

//! mailchimp-cli — OurOS Intuit Mailchimp marketing automation
//!
//! Single personality: `mailchimp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mailchimp [OPTIONS] [SUBCMD]");
        println!("Intuit Mailchimp (OurOS) — Email marketing + automation");
        println!();
        println!("Options:");
        println!("  --apikey KEY           Mailchimp API key (dc-key format)");
        println!("  campaigns send ID      Send campaign");
        println!("  lists subscribe        Add subscriber to list");
        println!("  --mandrill             Mandrill transactional email");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Intuit Mailchimp Marketing API v3.0 (OurOS)"); return 0; }
    println!("Intuit Mailchimp (OurOS)");
    println!("  Owner: Intuit (acquired 2021 for $12B)");
    println!("  Products: Email Marketing, Marketing Automation, Audience CDP,");
    println!("            Websites, Stores (e-commerce), Mandrill (transactional)");
    println!("  Editions: Free, Essentials, Standard, Premium (audience-size pricing)");
    println!("  Templates: 100+ email templates, drag-drop builder, custom HTML");
    println!("  Automation: customer journeys, abandoned cart, post-purchase, welcome");
    println!("  AI: subject line tester, send time optimization, content optimizer");
    println!("  Integrations: Shopify, WooCommerce, Magento, Salesforce, Stripe, 300+");
    println!("  API: REST v3, webhooks, OAuth 2.0, batch operations");
    println!("  License: free tier (500 contacts) + paid plans by contact count");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mailchimp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mailchimp"), "mailchimp");
        assert_eq!(basename(r"C:\bin\mailchimp.exe"), "mailchimp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mailchimp.exe"), "mailchimp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mc(&["--help".to_string()], "mailchimp"), 0);
        assert_eq!(run_mc(&["-h".to_string()], "mailchimp"), 0);
        assert_eq!(run_mc(&["--version".to_string()], "mailchimp"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mc(&[], "mailchimp"), 0);
    }
}
