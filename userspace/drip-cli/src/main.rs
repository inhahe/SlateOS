#![deny(clippy::all)]
//! drip-cli — personality CLI for Drip, the ecommerce-CRM email + SMS
//! platform.
//!
//! Founded 2013 by Rob Walling (serial bootstrapper, "MicroConf" + "Startups
//! For The Rest Of Us" podcast). Originally a simple drip-campaign tool
//! pitched at SaaS and indie founders. Acquired by Leadpages in 2016 and
//! then operated as a standalone ecommerce-CRM property under that parent.
//! Repositioned 2018-2019 as "Ecommerce CRM" — narrowed product focus to
//! Shopify, BigCommerce, WooCommerce store owners with deep store-data
//! integration, transactional event ingest, and revenue attribution.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Drip ecommerce-CRM email + SMS personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Walling 2013, Leadpages acquired 2016, ecom pivot");
    println!("    workflows     Visual automations with events + conditions");
    println!("    ecom          Shopify/Woo/BigCommerce deep integrations");
    println!("    segments      Behaviour + purchase-history filters");
    println!("    attribution   Revenue per workflow + per email");
    println!("    sms           Email + SMS in the same workflows");
    println!("    pricing       Per-contact tiers, ecom-pitched");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("drip-cli 0.1.0 (ecommerce-CRM personality build)"); }

fn run_about() {
    println!("Drip (Drip Global, Inc.).");
    println!("  Founded:    2013, Minneapolis area.");
    println!("  Founder:    Rob Walling — serial bootstrapper, MicroConf founder,");
    println!("              'Startups For The Rest Of Us' podcast.");
    println!("  Acquired:   2016 by Leadpages, kept its own brand.");
    println!("  Pivot:      Repositioned 2018-2019 as 'Ecommerce CRM' —");
    println!("              dropped SaaS-founder framing, doubled down on ecom.");
    println!("  Today:      Run as part of Redbrick group (since 2020) /");
    println!("              independent ecom-focused product.");
}

fn run_workflows() {
    println!("Workflows — visual automations.");
    println!("  Canvas of nodes: triggers, actions, decisions, goals, exits.");
    println!("  Triggers: order placed, cart abandoned, page viewed, custom event,");
    println!("            tag changed, date/birthday, form submitted.");
    println!("  Decisions: tag, custom field, segment, last order date,");
    println!("             lifetime revenue band.");
    println!("  Goals: 'reached purchase' moves the contact to the success branch.");
    println!("  Suitable for multi-week post-purchase + winback flows.");
}

fn run_ecom() {
    println!("Ecommerce integrations — Drip's main wedge.");
    println!("  Shopify         deep: products, orders, customers, refunds, sub-status.");
    println!("  WooCommerce     official plugin syncs orders + customers + products.");
    println!("  BigCommerce     similar depth to Shopify.");
    println!("  Magento + Square + custom REST also supported.");
    println!("  Browse + cart events captured via a JS snippet on the storefront.");
    println!("  Catalog sync: products + variants + inventory + prices in Drip.");
}

fn run_segments() {
    println!("Segments — behaviour + commerce filters.");
    println!("  Spent more than X in last 90 days, viewed product P, abandoned");
    println!("  cart containing variant V, ordered from collection C, etc.");
    println!("  Predictive segments: likely-to-purchase, likely-to-churn,");
    println!("  high-LTV — surface ranked lists.");
    println!("  Real-time updates: segments recompute as new events come in.");
}

fn run_attribution() {
    println!("Revenue attribution — first-class metric.");
    println!("  Per-email and per-workflow revenue dashboards.");
    println!("  Attribution window configurable (24h, 7d, 30d).");
    println!("  Compare to control segments to measure incremental lift.");
    println!("  Pitch vs general-purpose ESPs: ecom owners see GMV not opens.");
}

fn run_sms() {
    println!("SMS in the same workflows.");
    println!("  Email + SMS nodes in the same automation canvas.");
    println!("  Carrier coverage: US + CA primary, expanding internationally.");
    println!("  Pricing: per-segment SMS, separate from contact tier.");
    println!("  TCPA/GDPR consent collection baked into Drip forms + checkout.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Per active subscribers. Indicative:");
    println!("    2,500 subs   ~$39/mo.");
    println!("    5,000 subs   ~$89/mo.");
    println!("    10,000 subs  ~$154/mo.");
    println!("  SMS: per-message, separate add-on at all tiers.");
    println!("  Unlimited sends, unlimited workflows, all features on every tier.");
    println!("  14-day free trial.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Independent ecommerce brands on Shopify + Woo, especially in");
    println!("  apparel, beauty, supplements, niche-DTC categories.");
    println!("  Course creators using ecom checkout for digital products.");
    println!("  Public case studies on getdrip.com show typical revenue lift.");
    println!("  Klaviyo's most direct mid-market competitor in the segment.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "drip-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "workflows" => run_workflows(),
        "ecom" => run_ecom(),
        "segments" => run_segments(),
        "attribution" => run_attribution(),
        "sms" => run_sms(),
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
        run_workflows();
        run_ecom();
        run_segments();
        run_attribution();
        run_sms();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("drip-cli");
        print_version();
    }
}
