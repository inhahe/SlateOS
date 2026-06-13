#![deny(clippy::all)]
//! godaddy-cli — SlateOS personality CLI for GoDaddy, the world's largest domain registrar.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("GoDaddy — the world's largest registrar. NYSE: GDDY.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Bob Parsons, the IPO, KKR/Silver Lake");
    println!("    domains     Domain registration + auctions + appraisals");
    println!("    aftermarket Afternic + Dan.com acquisition (April 2022)");
    println!("    products    Hosting, Microsoft 365 reseller, websites + commerce");
    println!("    payments    GoDaddy Payments and Poynt acquisition (Jan 2020)");
    println!("    api         The GoDaddy Domains API");
    println!("    finance     Public company, revenue, segment mix");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Domains, hosting, websites, payments — for ~21 million customers worldwide.");
}

fn print_version() {
    println!("godaddy-cli 0.1.0");
    println!("GoDaddy Inc. — Tempe, Arizona. Founded 1997 as Jomax Technologies. NYSE: GDDY.");
}

fn cmd_about() {
    println!("GoDaddy");
    println!();
    println!("FOUNDED");
    println!("  1997 by Bob Parsons in Baltimore, originally as Jomax");
    println!("  Technologies. Renamed 'Go Daddy' in 1999 after Parsons");
    println!("  reportedly suggested 'Big Daddy' at a team meeting and a");
    println!("  colleague said 'how about Go Daddy?' (his recollection). The");
    println!("  company built a domain registrar business through aggressive");
    println!("  TV advertising — especially the Super Bowl spots (2005-2013)");
    println!("  that turned 'Go Daddy Girl' Danica Patrick into the brand face.");
    println!();
    println!("HEADQUARTERS");
    println!("  Tempe, Arizona, plus offices in Sunnyvale, Seattle, Cambridge MA,");
    println!("  Cambridge UK, Cologne Germany, Belgrade, Hyderabad, Singapore.");
    println!("  ~7,000 employees globally as of 2024.");
    println!();
    println!("PRIVATE EQUITY ERA (2011-2015)");
    println!("  In 2011, Parsons sold a majority stake to KKR + Silver Lake +");
    println!("  Technology Crossover Ventures for ~$2.25B, an industry-defining");
    println!("  PE buyout of an Internet infrastructure business. The PE owners");
    println!("  modernized operations and pruned the controversial advertising");
    println!("  approach (Indianapolis 500 and Super Bowl spots wound down).");
    println!();
    println!("PUBLIC LISTING");
    println!("  IPO on NYSE in April 2015 under ticker GDDY at $20/share,");
    println!("  raising $460M and valuing the company at ~$4.5B. As of");
    println!("  2024, market cap fluctuates in the $20-25B range.");
}

fn cmd_domains() {
    println!("GoDaddy domain services");
    println!();
    println!("SCALE");
    println!("  ~84 million domains under management (largest in the world by");
    println!("  a wide margin; next-closest is Tucows ~25M). ~21M customers");
    println!("  in 180+ countries.");
    println!();
    println!("BREADTH");
    println!("  ~500 TLDs supported: gTLDs, ccTLDs, new gTLDs, IDN");
    println!("  internationalized names. Domain backorders, drop-catching,");
    println!("  and a dedicated domain-search experience with AI-suggested");
    println!("  alternatives if your first choice is taken.");
    println!();
    println!("APPRAISALS");
    println!("  GoValue — an ML-driven appraisal tool that estimates the");
    println!("  resale value of a domain (length, dictionary words, TLD,");
    println!("  comparable sales). Available to domain owners free; used by");
    println!("  Afternic to anchor listings.");
    println!();
    println!("AUCTIONS")
    ;
    println!("  GoDaddy Auctions hosts ~10K expiring + premium auctions per day.");
    println!("  Includes 'Closeout' bargain bin and members-only premium access.");
    println!();
    println!("RENEWAL PRICING CRITIQUE");
    println!("  GoDaddy's renewal prices are typically $5-12/year above the");
    println!("  registry wholesale cost on .com — significantly higher than");
    println!("  registrars like Namecheap, Porkbun, or Cloudflare. Customers");
    println!("  often switch out at renewal time.");
}

fn cmd_aftermarket() {
    println!("Afternic + Dan.com — GoDaddy's aftermarket dominance");
    println!();
    println!("AFTERNIC");
    println!("  Acquired by GoDaddy in October 2013 (as part of the broader");
    println!("  NameFind / Dot To Lot / Afternic asset bundle). Afternic");
    println!("  operates the world's largest domain-name aftermarket exchange,");
    println!("  syndicating premium-domain listings to 100+ partner registrars'");
    println!("  search results. Sellers list once; the listing appears wherever");
    println!("  a buyer searches across the partner network.");
    println!();
    println!("DAN.COM");
    println!("  Acquired April 2022 for an undisclosed sum (likely $90-150M");
    println!("  range based on reporting). Dan (founded 2013 by Reza Sardeha");
    println!("  in Amsterdam) operated an aftermarket marketplace with a");
    println!("  reputation for sleek UX and seller-friendly tools. GoDaddy");
    println!("  initially ran the brands in parallel; Dan.com began transitioning");
    println!("  to Afternic in mid-2024 as part of consolidation.");
    println!();
    println!("WHY THIS MATTERS");
    println!("  Combined, Afternic + Dan handle the lion's share of all");
    println!("  premium domain transactions. The list-once-distribute-everywhere");
    println!("  model essentially makes Afternic the de-facto exchange for");
    println!("  domain aftermarket liquidity. Sellers paying ~15-20% commissions");
    println!("  fund a meaningful chunk of GoDaddy's recurring revenue.");
}

fn cmd_products() {
    println!("GoDaddy product portfolio");
    println!();
    println!("WEBSITES + COMMERCE");
    println!("  Websites + Marketing (formerly GoCentral) — drag-drop site");
    println!("  builder competing with Wix/Squarespace. Plans $9.99-29.99/mo");
    println!("  with built-in commerce (Stripe/Square integration), email");
    println!("  marketing (Mad Mimi roots), appointments + scheduling.");
    println!();
    println!("MANAGED WORDPRESS");
    println!("  WordPress hosting tuned for performance. The mt-Media-Temple");
    println!("  acquisition (2013) brought MT Grid hosting expertise; the");
    println!("  Sucuri acquisition (2017) brought WP security tooling.");
    println!();
    println!("EMAIL + PRODUCTIVITY");
    println!("  Microsoft 365 reseller (one of the largest in the world).");
    println!("  Auto-provisions mail + domain DNS records together. Plans");
    println!("  $5.99/user/mo to $11.99/user/mo.");
    println!();
    println!("CPANEL/PLESK HOSTING");
    println!("  Shared hosting (Linux + Windows), VPS, dedicated. Legacy");
    println!("  segment with declining growth; cross-sells to higher-ARPU");
    println!("  products is the strategic focus.");
    println!();
    println!("CLOUD DOMAINS")
    ;
    println!("  In April 2023 GoDaddy acquired Google Domains' customer base");
    println!("  from Google for an undisclosed sum, adding ~10M domains and");
    println!("  significantly broadening the developer-friendly registrar's");
    println!("  inventory.");
}

fn cmd_payments() {
    println!("GoDaddy Payments");
    println!();
    println!("POYNT ACQUISITION (January 2020)");
    println!("  GoDaddy acquired Poynt for ~$365M cash from Osama Bedier (ex-");
    println!("  Google Wallet, ex-PayPal). Poynt made smart payment terminals");
    println!("  with an open-app platform — essentially small commerce-focused");
    println!("  hardware + a payment-processor stack.");
    println!();
    println!("THE RESULT");
    println!("  GoDaddy Payments launched in 2021 as an in-house payment-");
    println!("  processor stack offered to GoDaddy customers. Pricing:");
    println!("  2.3% + $0.00 in-person (no per-transaction fee, atypical for");
    println!("  the industry), 2.9% + $0.30 online — competitive with Square,");
    println!("  Stripe, and PayPal.");
    println!();
    println!("HARDWARE");
    println!("  GoDaddy POS (Poynt 5 + Poynt Smart Terminal) for in-person");
    println!("  retail. Sold as part of the Websites + Commerce bundles.");
    println!();
    println!("STRATEGIC RATIONALE");
    println!("  Cross-sell into the existing customer base of small-business");
    println!("  domain + hosting buyers, capturing the payment-processing");
    println!("  spread that previously went to Square / Stripe / PayPal");
    println!("  via third-party integrations.");
}

fn cmd_api() {
    println!("GoDaddy Domains API");
    println!();
    println!("BASE URL");
    println!("  https://api.godaddy.com/v1/");
    println!("  OTE (sandbox): https://api.ote-godaddy.com/v1/");
    println!();
    println!("AUTH");
    println!("  sso-key <key>:<secret> header. API keys generated in the");
    println!("  customer account dashboard; tiered by daily-call limits.");
    println!();
    println!("RESOURCES");
    println!("  /domains, /domains/available, /domains/suggest, /domains/<name>,");
    println!("  /domains/<name>/records, /domains/<name>/contacts,");
    println!("  /domains/agreements, /shoppers, /orders, /aftermarket/listings,");
    println!("  /subscriptions, /certificates.");
    println!();
    println!("TYPICAL USES");
    println!("  - Bulk-check availability for premium-name research");
    println!("  - Automate DNS-record updates for service-discovery use cases");
    println!("  - Provision domains as part of customer-onboarding flows");
    println!("  - Sync expirations + renewals into business systems");
    println!();
    println!("DEVELOPER FRICTION");
    println!("  The API is functional but minimal compared to Cloudflare's");
    println!("  Registrar API or DNSimple's. Webhooks are limited; DNSSEC");
    println!("  + DS-record automation is partial. Common workaround: drive");
    println!("  GoDaddy via the API for orders, then transfer to Cloudflare/");
    println!("  Route 53 for the actual DNS hosting.");
}

fn cmd_finance() {
    println!("GoDaddy — financial snapshot (FY2023)");
    println!();
    println!("REVENUE");
    println!("  ~$4.25 billion in FY2023, up ~4% YoY in a soft year for");
    println!("  consumer + SMB tech. Long-term CAGR ~10% since IPO.");
    println!();
    println!("SEGMENTS (approximate mix)");
    println!("  Core Platform        ~76%  (domains + hosting + email)");
    println!("  Applications + Comm  ~24%  (Websites+Marketing, Payments,");
    println!("                              Professional Services)");
    println!();
    println!("KEY METRICS");
    println!("  ~21 million customers");
    println!("  ~84 million domains under management");
    println!("  ARPU ~$200/year");
    println!("  Customer count growth ~1-3% per year; ARPU growth ~4-7%");
    println!("  (the levers are cross-sell + price increases, not customer");
    println!("  acquisition).");
    println!();
    println!("MARKET CAP");
    println!("  Fluctuates ~$20-25B. ~$300M+ annual buyback authorizations.");
    println!("  S&P 500 component since 2024.");
    println!();
    println!("THE BUYBACK MACHINE");
    println!("  GoDaddy generates ~$1.4B of unlevered free cash flow per year");
    println!("  and returns much of it via share buybacks. Critics argue this");
    println!("  prioritizes shareholder returns over product investment;");
    println!("  bulls argue it reflects a mature, durable business with");
    println!("  limited high-IRR reinvestment opportunities.");
}

fn run_godaddy(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "domains" => { cmd_domains(); 0 }
        "aftermarket" => { cmd_aftermarket(); 0 }
        "products" => { cmd_products(); 0 }
        "payments" => { cmd_payments(); 0 }
        "api" => { cmd_api(); 0 }
        "finance" => { cmd_finance(); 0 }
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
        .unwrap_or_else(|| "godaddy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_godaddy(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/godaddy"), "godaddy");
        assert_eq!(basename("C:\\Tools\\godaddy.exe"), "godaddy.exe");
        assert_eq!(basename("godaddy"), "godaddy");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("godaddy.exe"), "godaddy");
        assert_eq!(strip_ext("godaddy"), "godaddy");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_godaddy(&["help".to_string()], "godaddy"), 0);
        let _ = run_godaddy(&[], "godaddy");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_godaddy(&["nope".to_string()], "godaddy"), 2);
    }
}
