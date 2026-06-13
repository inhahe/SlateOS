#![deny(clippy::all)]
//! dnsimple-cli — Slate OS personality CLI for DNSimple, the developer-friendly DNS + domain registrar.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("DNSimple — the simple-and-honest registrar + DNS provider.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Anthony Eden and the developer-first registrar bet");
    println!("    one         ONE-record — DNSimple's automated-presets feature");
    println!("    api         The DNSimple REST API and Terraform provider");
    println!("    domains     Domain registration, transfers, EPP");
    println!("    services    One-click integrations for SaaS apps");
    println!("    pricing     Per-account flat pricing, registrar pass-through");
    println!("    privacy     The owner-led, profit-funded, no-VC stance");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("DNS for developers, by developers, since 2010.");
}

fn print_version() {
    println!("dnsimple-cli 0.1.0");
    println!("DNSimple Corporation — Boston/Florence Italy. Founded 2010.");
}

fn cmd_about() {
    println!("DNSimple");
    println!();
    println!("FOUNDED");
    println!("  2010 by Anthony Eden, an American living in Florence, Italy.");
    println!("  Anthony had been a developer at Engine Yard and previously at");
    println!("  Mozy/EMC; he was frustrated with the registrar experience");
    println!("  (GoDaddy upsells, complex DNS interfaces, no APIs) and decided");
    println!("  to build a clean alternative as a side project. Within a few");
    println!("  years it became his full-time business.");
    println!();
    println!("HEADQUARTERS");
    println!("  Florence, Italy (founder) + Boston, MA (US ops). Fully remote,");
    println!("  ~20 employees across the US, EU, and APAC. Profitable, no VC.");
    println!();
    println!("AUTHORITATIVE INFRASTRUCTURE");
    println!("  Anycast network with PoPs across the US (NYC, Ashburn, San Jose),");
    println!("  Europe (London, Amsterdam, Frankfurt), Asia (Singapore, Tokyo),");
    println!("  Australia (Sydney), and South America (Sao Paulo). DNSSEC-");
    println!("  signed zones supported. 100% query uptime SLA.");
    println!();
    println!("ACCREDITATION");
    println!("  ICANN-accredited registrar (since 2017); previously operated");
    println!("  via partner-registrars Enom and OpenSRS. Today sells .com / .net");
    println!("  / .org / .io / .dev / .app and ~400 other TLDs at near-cost.");
}

fn cmd_one() {
    println!("ONE-record — DNSimple's automated presets");
    println!();
    println!("WHAT IT IS");
    println!("  A managed bundle of DNS records for popular SaaS services.");
    println!("  Instead of looking up 'how do I connect example.com to");
    println!("  Heroku?' you tell DNSimple 'Apply Heroku preset' and it sets");
    println!("  the correct ALIAS/CNAME/A records at the apex and www");
    println!("  automatically. If Heroku changes their endpoints, DNSimple");
    println!("  updates the records for you.");
    println!();
    println!("APEX ALIAS");
    println!("  DNSimple was an early adopter of ALIAS records (sometimes");
    println!("  called ANAME) — A-record-like behavior at the zone apex that");
    println!("  resolves a CNAME-target on the fly. Enables 'example.com");
    println!("  -> Heroku/Fastly/Netlify' without delegating the whole");
    println!("  domain to the provider's nameservers.");
    println!();
    println!("INTEGRATIONS WITH SAAS");
    println!("  Built-in services: Google Workspace, Microsoft 365, Heroku,");
    println!("  Fastly, Netlify, Vercel, Render, GitHub Pages, AWS, Cloudflare,");
    println!("  Mailgun, SendGrid, Squarespace, Shopify, Webflow, Tumblr, and");
    println!("  dozens more. Each is a one-click 'Add to my domain' action.");
}

fn cmd_api() {
    println!("DNSimple API");
    println!();
    println!("BASE URL");
    println!("  https://api.dnsimple.com/v2/");
    println!("  Sandbox: https://api.sandbox.dnsimple.com/v2/");
    println!();
    println!("AUTH");
    println!("  Bearer tokens (HTTP API access token from account settings).");
    println!("  OAuth 2.0 for third-party app integrations.");
    println!();
    println!("RESOURCES");
    println!("  /accounts, /domains, /zones, /zones/<id>/records,");
    println!("  /registrar/domains/<name>/check, /registrar/domains/<name>/register,");
    println!("  /registrar/domains/<name>/transfer, /webhooks, /collaborators,");
    println!("  /services, /vanity, /templates, /tlds, /contacts.");
    println!();
    println!("OFFICIAL LIBRARIES");
    println!("  Ruby (dnsimple-ruby), Go (dnsimple-go), Node.js (dnsimple-node),");
    println!("  PHP, Python, Elixir, Java, .NET. Community Rust bindings exist.");
    println!();
    println!("TERRAFORM");
    println!("  The dnsimple/dnsimple Terraform provider supports zones,");
    println!("  records, services, and registrar resources. Popular for");
    println!("  Infrastructure-as-Code shops who want their entire DNS in git.");
}

fn cmd_domains() {
    println!("Domain registration via DNSimple");
    println!();
    println!("REGISTRATION");
    println!("  ICANN-accredited registrar (since 2017). 400+ TLDs supported.");
    println!("  WHOIS privacy is free on every eligible TLD. Auto-renewal");
    println!("  defaults on; can be configured per domain.");
    println!();
    println!("EPP / TRANSFERS");
    println!("  Domain transfers in/out are first-class API operations.");
    println!("  Authentication via EPP/transfer code. Bulk transfers via API.");
    println!();
    println!("DNSSEC");
    println!("  Automated DS-record submission to registries for TLDs that");
    println!("  support it (.com, .net, .org, .io, .dev, .app, and ~100 others).");
    println!();
    println!("PREMIUM DOMAINS");
    println!("  Premium-priced inventory (registry-controlled) is displayed");
    println!("  with the actual registry price. No covered fees, no upsells.");
    println!();
    println!("CCTLDS");
    println!("  Supports ~100 ccTLDs including .uk, .de, .nl, .es, .it, .au,");
    println!("  .nz, .in, .co, .me, .ca, .br. Some have local-presence");
    println!("  requirements handled by DNSimple's contacts service.");
}

fn cmd_services() {
    println!("DNSimple Services (one-click DNS integrations)");
    println!();
    println!("PURPOSE");
    println!("  Eliminate the most common DNS configuration mistake: 'I copied");
    println!("  the records from the help docs but it still doesn't work.'");
    println!("  Each Service in DNSimple's library is a curated set of records");
    println!("  maintained by DNSimple alongside the upstream SaaS partner.");
    println!();
    println!("EXAMPLES (~100 services)");
    println!("  Google Workspace        SPF + DKIM + DMARC + MX + verification");
    println!("  Microsoft 365           MX + TXT + autodiscover + SPF");
    println!("  Heroku                  ALIAS @ + CNAME www");
    println!("  Vercel                  ALIAS @ + CNAME www + verification");
    println!("  Netlify                 ALIAS @ + CNAME www");
    println!("  Cloudflare              NS delegation (full-DNS handoff)");
    println!("  Mailgun                 MX + SPF + DKIM + tracking CNAMEs");
    println!("  Shopify                 A + CNAME for shops.myshopify.com");
    println!("  GitHub Pages            A x4 + AAAA x4 + CNAME www");
    println!("  Webflow                 A + CNAME + verification");
    println!();
    println!("KEEPING CURRENT");
    println!("  When a SaaS changes endpoints (e.g., Heroku's apex address),");
    println!("  DNSimple updates the Service definition and (with permission)");
    println!("  applies the change to all affected zones automatically.");
}

fn cmd_pricing() {
    println!("DNSimple pricing (as of 2024)");
    println!();
    println!("ACCOUNT TIERS (flat monthly, includes a number of domains)");
    println!("  Personal      $5/mo   1 domain managed (DNS + registration)");
    println!("  Professional  $25/mo  3 zones, 10 domains, all features");
    println!("  Business      $65/mo  10 zones, 25 domains, sub-accounts");
    println!("  Master        $200/mo 25 zones, 75 domains, audit logs, SSO");
    println!();
    println!("DOMAIN REGISTRATIONS");
    println!("  Pass-through to registry pricing — DNSimple does NOT mark up");
    println!("  domain registrations beyond a small accreditation fee.");
    println!("  Examples:  .com  $10/yr   .org  $13/yr   .io  $59/yr");
    println!("             .dev  $14/yr   .app  $16/yr   .ai   $79/yr");
    println!();
    println!("FREE FOREVER");
    println!("  WHOIS privacy, DNSSEC, transfer-in, custom nameservers,");
    println!("  basic SSL/TLS service integration, all API endpoints.");
}

fn cmd_privacy() {
    println!("DNSimple's owner-led, profit-funded stance");
    println!();
    println!("NO VC, NO EXIT PRESSURE");
    println!("  Anthony Eden owns and runs the company. There are no");
    println!("  venture investors, no preferred stock, no liquidation");
    println!("  preferences, no exit obligations. The company has been");
    println!("  profitable since shortly after launch and stayed profitable");
    println!("  through every cyclical downturn.");
    println!();
    println!("CALM-COMPANY ETHOS");
    println!("  Asynchronous, fully remote. Public blog (blog.dnsimple.com)");
    println!("  documents engineering, ops, and people decisions. The company");
    println!("  publishes its values, salary methodology, and policies openly.");
    println!();
    println!("PRIVACY POSTURE");
    println!("  DNSimple does not sell user data, run ad networks, or build");
    println!("  retargeting profiles. WHOIS privacy is free; GDPR-compliant");
    println!("  by default; no dark-pattern upsells in the registrar flow.");
    println!();
    println!("WHY IT MATTERS");
    println!("  In a market dominated by GoDaddy's upsell barrage, Namecheap's");
    println!("  bait-and-switch renewal pricing, and the Endurance / Newfold");
    println!("  rollup of EIG-acquired brands, DNSimple is one of the few");
    println!("  registrars where the price you see is the price you pay.");
}

fn run_dnsimple(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "one" => { cmd_one(); 0 }
        "api" => { cmd_api(); 0 }
        "domains" => { cmd_domains(); 0 }
        "services" => { cmd_services(); 0 }
        "pricing" => { cmd_pricing(); 0 }
        "privacy" => { cmd_privacy(); 0 }
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
        .unwrap_or_else(|| "dnsimple".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_dnsimple(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/dnsimple"), "dnsimple");
        assert_eq!(basename("C:\\Tools\\dnsimple.exe"), "dnsimple.exe");
        assert_eq!(basename("dnsimple"), "dnsimple");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("dnsimple.exe"), "dnsimple");
        assert_eq!(strip_ext("dnsimple"), "dnsimple");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_dnsimple(&["help".to_string()], "dnsimple"), 0);
        let _ = run_dnsimple(&[], "dnsimple");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_dnsimple(&["nope".to_string()], "dnsimple"), 2);
    }
}
