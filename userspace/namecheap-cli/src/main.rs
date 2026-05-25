#![deny(clippy::all)]
//! namecheap-cli — OurOS personality CLI for Namecheap, the long-standing affordable registrar.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Namecheap — \"Cheap names. Big names. All names.\" since 2000.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Richard Kirkendall, Phoenix HQ, and 24 years of growth");
    println!("    domains     Registration, transfers, marketplace");
    println!("    privacy     WhoisGuard, the original free-privacy advocate");
    println!("    privateemail Private Email (formerly Hosted Email) suite");
    println!("    sslcerts    Comodo / Sectigo SSL reseller pioneer");
    println!("    hosting     Shared, VPS, dedicated, EasyWP managed WordPress");
    println!("    activism    Free expression and SOPA / PIPA history");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Cheap. Predictable. No GoDaddy upsells.");
}

fn print_version() {
    println!("namecheap-cli 0.1.0");
    println!("Namecheap, Inc. — Phoenix, Arizona. Founded 2000.");
}

fn cmd_about() {
    println!("Namecheap");
    println!();
    println!("FOUNDED");
    println!("  2000 by Richard Kirkendall, a Los Angeles entrepreneur who");
    println!("  registered the namecheap.com domain (a one-off play on cheap");
    println!("  domain names) and built a small reseller business through");
    println!("  eNom. The business grew through word-of-mouth in early forums");
    println!("  and on Slashdot during the 'GoDaddy advertising tsunami' era,");
    println!("  positioning Namecheap as the no-upsell, no-bullshit alternative.");
    println!();
    println!("INCORPORATION + GROWTH");
    println!("  Headquartered in Phoenix, Arizona. ICANN-accredited 2007.");
    println!("  Crossed 1M domains under management 2010, 3M by 2014, 7M by");
    println!("  2018, ~16M by 2024. ~1,500 employees globally with operations");
    println!("  centers in Kyiv, Sumy, and Tirana.");
    println!();
    println!("OWNERSHIP");
    println!("  Privately held by founder Richard Kirkendall. No VC funding,");
    println!("  no PE rollup, no public-market obligations.");
    println!();
    println!("UKRAINE CONNECTION");
    println!("  Significant engineering presence in Ukraine since the 2010s.");
    println!("  Donated $1M+ to Ukrainian humanitarian causes during 2022,");
    println!("  ceased operations in Russia/Belarus, and maintained payroll");
    println!("  for displaced Ukrainian employees through the invasion.");
}

fn cmd_domains() {
    println!("Namecheap domain services");
    println!();
    println!("REGISTRATION");
    println!("  500+ TLDs: .com, .net, .org, .io, .ai, .co, .me, .dev, .app,");
    println!("  .xyz, .online, .store, plus most ccTLDs. Sandbox-cheap intro");
    println!("  pricing on new TLDs and aggressive promo cycles.");
    println!();
    println!("MARKETPLACE");
    println!("  Resale of secondary-market premium domains. Sellers list");
    println!("  inventory at fixed or 'make offer' pricing; Namecheap brokers");
    println!("  the transfer + escrow.");
    println!();
    println!("BACKORDERS + EXPIRED");
    println!("  Backorder a soon-to-expire domain; Namecheap attempts to");
    println!("  capture it through their drop-catching network when it hits");
    println!("  the redemption period. Auction-based when multiple bidders");
    println!("  exist.");
    println!();
    println!("API");
    println!("  https://api.namecheap.com/xml.response — XML SOAP-ish");
    println!("  endpoint older than most current developers. New REST-style");
    println!("  API documented in beta as of 2024. IP allowlisting required.");
    println!();
    println!("RENEWAL HONESTY");
    println!("  First-year promo prices for new TLDs are common, but renewal");
    println!("  prices are listed plainly on the product page (Namecheap was");
    println!("  among the early registrars to surface renewal cost up-front).");
}

fn cmd_privacy() {
    println!("WhoisGuard — Namecheap's WHOIS privacy");
    println!();
    println!("THE STORY");
    println!("  WHOIS privacy used to be a $5-15/year add-on at most registrars");
    println!("  (and still is at GoDaddy). Namecheap was the first major");
    println!("  registrar to make WHOIS privacy free by default — initially");
    println!("  branded WhoisGuard, now just baked in.");
    println!();
    println!("POST-GDPR");
    println!("  GDPR (May 2018) forced most registrars to redact WHOIS by");
    println!("  default for EU-resident registrants. ICANN's Temporary");
    println!("  Specification extended this to most public WHOIS records.");
    println!("  Namecheap's pre-existing WhoisGuard infrastructure handled");
    println!("  the transition gracefully — for them, GDPR ratified what they");
    println!("  had been offering for years.");
    println!();
    println!("PROXY EMAILS");
    println!("  Registrants get a forwarding alias (e.g., x.y@whoisguard.com)");
    println!("  shown in WHOIS instead of their real address. Spam to the");
    println!("  alias is rate-limited, filtered, and can be disabled per-domain.");
}

fn cmd_privateemail() {
    println!("Private Email — hosted email");
    println!();
    println!("PLANS");
    println!("  Starter    $0.84/mo  3 mailboxes, 5GB each, basic webmail");
    println!("  Pro        $2.91/mo  unlimited mailboxes per domain, 30GB, calendar");
    println!("  Ultimate   $4.83/mo  70GB, advanced calendar + contacts + tasks");
    println!();
    println!("FEATURES");
    println!("  - IMAP / POP3 / SMTP with TLS");
    println!("  - SPF / DKIM / DMARC automated for Namecheap-managed DNS");
    println!("  - Webmail (Open-Xchange App Suite under the hood)");
    println!("  - Mobile apps for iOS + Android");
    println!("  - 30-day free trial on new domain registrations");
    println!();
    println!("POSITIONING");
    println!("  Lower cost than Google Workspace ($6/user/mo) or Microsoft 365");
    println!("  Business Basic ($6/user/mo) when all you need is an email");
    println!("  account on your custom domain — common for sole proprietors,");
    println!("  freelancers, and small NGOs.");
}

fn cmd_sslcerts() {
    println!("SSL certificates");
    println!();
    println!("RESELLER HISTORY");
    println!("  Namecheap was one of the first major Comodo (now Sectigo) SSL");
    println!("  resellers and pioneered the 'cheap DV cert' market in the");
    println!("  pre-Let's-Encrypt era. PositiveSSL at ~$8/year drove down");
    println!("  industry pricing for basic certs across the 2010s.");
    println!();
    println!("CURRENT TIERS");
    println!("  PositiveSSL              ~$5.99/yr   single-domain DV");
    println!("  PositiveSSL Wildcard     ~$71/yr     wildcard DV");
    println!("  EssentialSSL             ~$6.99/yr   alt single-domain DV");
    println!("  InstantSSL               ~$33/yr     single-domain OV");
    println!("  EV SSL                   ~$87/yr     extended validation");
    println!("  Multi-Domain (SAN)       ~$30+/yr    up to 250 SANs");
    println!();
    println!("LET'S ENCRYPT COEXISTENCE");
    println!("  Free Let's Encrypt undercut the cheap-DV market starting 2016.");
    println!("  Namecheap kept the reseller revenue by offering paid certs");
    println!("  for use cases LE doesn't address well: OV/EV identity vetting,");
    println!("  warranty backing, longer 'no-tooling' lifetimes, customer trust");
    println!("  badges, and certs for shared-hosting customers who can't run");
    println!("  certbot.");
}

fn cmd_hosting() {
    println!("Namecheap hosting products");
    println!();
    println!("SHARED");
    println!("  Stellar       ~$1.58/mo intro  cPanel, 3 sites, 20GB SSD");
    println!("  Stellar Plus  ~$2.68/mo intro  unmetered sites, 50GB SSD, AutoBackup");
    println!("  Stellar Bus.  ~$4.80/mo intro  cloud-redundant, 100GB SSD, premium support");
    println!();
    println!("EASYWP");
    println!("  Managed WordPress on Namecheap's own platform (not WP Engine /");
    println!("  Kinsta-tier). 50-200K monthly visits depending on plan. Auto-");
    println!("  updates, daily backups, staging, free SSL.");
    println!();
    println!("VPS");
    println!("  Pulsar ~$6.88/mo  2GB RAM / 40GB SSD / 1TB transfer");
    println!("  Quasar ~$12/mo    4GB RAM / 120GB SSD");
    println!("  Magnetar ~$30/mo  8GB RAM / 240GB SSD");
    println!();
    println!("DEDICATED");
    println!("  Xeon E-2236 / Ryzen 7 boxes in US-IAD + UK-LON data centers.");
    println!("  Bare-metal pricing from ~$60/mo intro, ~$120/mo renewal.");
}

fn cmd_activism() {
    println!("Namecheap's free-expression activism");
    println!();
    println!("SOPA / PIPA (2011-2012)");
    println!("  Namecheap was one of the loudest registrars opposing the Stop");
    println!("  Online Piracy Act + PROTECT IP Act. Founded the");
    println!("  'MoveYourDomainDay' campaign on December 29, 2011, encouraging");
    println!("  customers to transfer away from Go Daddy (who had publicly");
    println!("  supported SOPA at the time). The campaign captured tens of");
    println!("  thousands of transfers and was credited with forcing Go Daddy");
    println!("  to publicly reverse its position within 48 hours.");
    println!();
    println!("PRIVACY ADVOCACY");
    println!("  Long-running blog series + lobbying for ICANN to maintain");
    println!("  WHOIS privacy protections, GDPR-style data minimization, and");
    println!("  resistance to government-mandated registrant-identity schemes.");
    println!();
    println!("UKRAINE 2022+");
    println!("  Suspended service to Russian and Belarusian registrants in");
    println!("  March 2022 (one of the only registrars to do so). Donated");
    println!("  $1M+ to Ukrainian humanitarian causes; ran continued payroll");
    println!("  for displaced Ukrainian engineering staff.");
    println!();
    println!("Free expression bona fides are a core part of brand identity.");
}

fn run_namecheap(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "domains" => { cmd_domains(); 0 }
        "privacy" => { cmd_privacy(); 0 }
        "privateemail" => { cmd_privateemail(); 0 }
        "sslcerts" => { cmd_sslcerts(); 0 }
        "hosting" => { cmd_hosting(); 0 }
        "activism" => { cmd_activism(); 0 }
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
        .unwrap_or_else(|| "namecheap".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_namecheap(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/namecheap"), "namecheap");
        assert_eq!(basename("C:\\Tools\\namecheap.exe"), "namecheap.exe");
        assert_eq!(basename("namecheap"), "namecheap");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("namecheap.exe"), "namecheap");
        assert_eq!(strip_ext("namecheap"), "namecheap");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_namecheap(&["help".to_string()], "namecheap"), 0);
        assert_eq!(run_namecheap(&[], "namecheap"), 0);
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_namecheap(&["nope".to_string()], "namecheap"), 2);
    }
}
