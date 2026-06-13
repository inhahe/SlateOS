#![deny(clippy::all)]
//! easydns-cli — SlateOS personality CLI for easyDNS, the Canadian civil-liberties registrar.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("easyDNS — DNS hosting + registrar with a free-speech backbone.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Mark Jeftovic and the 1998 founding");
    println!("    dns         DNS hosting, anycast PoPs, secondary, DDoS");
    println!("    domains     Domain registration with the Canadian privacy angle");
    println!("    civil       Civil-liberties stance and famous takedown refusals");
    println!("    api         REST API for DNS + domain automation");
    println!("    backupmx    Backup MX, email forwarding, mailbag");
    println!("    plans       Tiered DNS hosting plans");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Defending the open Internet — one zone at a time.");
}

fn print_version() {
    println!("easydns-cli 0.1.0");
    println!("easyDNS Technologies, Inc. — Toronto, Canada. Founded 1998.");
}

fn cmd_about() {
    println!("easyDNS");
    println!();
    println!("FOUNDED");
    println!("  1998 in Toronto, Canada, by Mark Jeftovic + Colin Viebrock as");
    println!("  one of the world's first commercial managed-DNS services. The");
    println!("  founders were among the first to commercialize 'authoritative");
    println!("  DNS as a service' — selling zone hosting separately from");
    println!("  registrar and hosting bundles.");
    println!();
    println!("HEADQUARTERS");
    println!("  Toronto, Ontario. Privately held, owner-operated. Small team");
    println!("  (~20-30 employees) with disproportionate cultural footprint");
    println!("  via Mark's frequent public commentary on Internet policy.");
    println!();
    println!("PHILOSOPHY");
    println!("  easyDNS positions itself as a values-driven Internet");
    println!("  infrastructure company: committed to free expression, due");
    println!("  process, customer privacy, and resistance to overreach by");
    println!("  governments + corporate complainants. Mark Jeftovic blogs at");
    println!("  axisofeasy.com / easydns.com/blog about cryptocurrency,");
    println!("  censorship, DNS policy, and emerging-tech politics.");
}

fn cmd_dns() {
    println!("easyDNS hosting platform");
    println!();
    println!("ANYCAST NETWORK");
    println!("  ~20 globally-distributed PoPs. BGP anycast announcements");
    println!("  for nameserver IPs; queries hit the nearest responder.");
    println!("  DDoS protection sized for sustained multi-hundred-Gbps events.");
    println!();
    println!("RECORD TYPES");
    println!("  Full RFC support: A, AAAA, CNAME, MX, NS, PTR, SOA, SPF, TXT,");
    println!("  SRV, CAA, NAPTR, TLSA, HTTPS, SVCB. DNSSEC supported on");
    println!("  eligible TLDs with automated DS-record submission to registries");
    println!("  for domains also registered through easyDNS.");
    println!();
    println!("SECONDARY DNS");
    println!("  Operate easyDNS as a secondary (slave) to your on-prem");
    println!("  BIND/NSD/PowerDNS primary, with TSIG-signed AXFR/IXFR");
    println!("  transfers. A long-standing use case for enterprise customers.");
    println!();
    println!("DYNAMIC DNS");
    println!("  Per-record DNS API + DDNS-style update endpoints for home-lab");
    println!("  and remote-worker setups. Common with the original-easyDNS-");
    println!("  customer demographic of network-savvy individuals.");
    println!();
    println!("GLOBAL TRAFFIC DIRECTOR (GTD)");
    println!("  Geo-routing + failover + weighted-round-robin for premium plans.");
    println!("  HTTP/HTTPS/TCP/UDP/Ping monitors drive automatic failover.");
}

fn cmd_domains() {
    println!("easyDNS domain registration");
    println!();
    println!("ACCREDITATION");
    println!("  ICANN-accredited registrar. Authorized to sell .com / .net /");
    println!("  .org / .info and ~400 other gTLDs + new gTLDs. Registry-");
    println!("  accredited or partner-accredited for ~100 ccTLDs.");
    println!();
    println!("CANADIAN POSTURE");
    println!("  As a Canadian company, easyDNS is bound by Canadian law and");
    println!("  CIRA's policies on .ca registrations. This includes stricter");
    println!("  privacy defaults than US registrars, judicial review");
    println!("  requirements for content-related takedowns, and PIPEDA");
    println!("  privacy obligations on customer data handling.");
    println!();
    println!("PRIVACY DEFAULTS");
    println!("  WHOIS privacy free on all eligible TLDs. .ca registrations");
    println!("  by default redact registrant information per CIRA policy");
    println!("  unless you're a business owner who chooses to publish.");
    println!();
    println!("LIFETIME PRICING TIER")
    ;
    println!("  easyDNS offers (in selected promo periods) extended-pricing");
    println!("  options where renewals are locked at registration price for");
    println!("  the life of a corporate plan — a hedge against future");
    println!("  registry pricing increases (e.g., Verisign's regular .com price hikes).");
}

fn cmd_civil() {
    println!("easyDNS's civil-liberties stance");
    println!();
    println!("FAMOUS REFUSAL: WIKILEAKS DOMAINS (2010)");
    println!("  After EveryDNS dropped Wikileaks under US government pressure");
    println!("  in December 2010, easyDNS provided emergency DNS service for");
    println!("  some Wikileaks domains. Mark Jeftovic wrote a much-cited essay");
    println!("  arguing that DNS providers should not act as ad-hoc content");
    println!("  arbiters and should require due-process orders.");
    println!();
    println!("ROJADIRECTA (2011)");
    println!("  US Homeland Security seized Spanish sports-streaming domains");
    println!("  via Verisign during the SOPA-era ICE seizures. easyDNS used");
    println!("  the incident to publicly criticize the policy framework that");
    println!("  let one US agency seize gTLDs operated under foreign-owned");
    println!("  Spanish judicial findings of legality.");
    println!();
    println!("UNDERCOVER MARKETING + DMCA ABUSE");
    println!("  easyDNS has refused multiple takedown demands when the");
    println!("  underlying complaint appears to be reputation management,");
    println!("  trademark bullying, or competitive sabotage rather than a");
    println!("  good-faith infringement claim. Customers pay a premium to");
    println!("  know their registrar will fight rather than fold.");
    println!();
    println!("DRP / UDRP DEFENSE NETWORK");
    println!("  easyDNS partners with domain-defense lawyers and the Internet");
    println!("  Commerce Association to defend customers facing UDRP /");
    println!("  cybersquatting complaints, raising the cost of frivolous filings.");
    println!();
    println!("BITCOIN-DENOMINATED PAYMENT (since 2014)");
    println!("  One of the first registrars to accept Bitcoin. Reflects the");
    println!("  cypherpunk-adjacent values of the founder + customer base.");
}

fn cmd_api() {
    println!("easyDNS REST API");
    println!();
    println!("BASE URL");
    println!("  https://rest.easydns.net/");
    println!();
    println!("AUTH");
    println!("  HTTP Basic with token+key generated in the customer portal.");
    println!();
    println!("RESOURCES");
    println!("  /domains                     list/manage your domains");
    println!("  /domain/<domain>             domain details");
    println!("  /zones/records/all/<domain>  list zone records");
    println!("  /zones/records/add/<domain>  create a record");
    println!("  /zones/records/modify/<domain>/<id>");
    println!("  /zones/records/delete/<domain>/<id>");
    println!("  /domain/registrant/<domain>  WHOIS contact");
    println!("  /domain/nameservers/<domain> set nameservers");
    println!();
    println!("DDNS ENDPOINT");
    println!("  GET https://api.cp.easydns.com/dyn/generic.php?hostname=...&myip=...");
    println!("  HTTP Basic auth. Used by home-lab / dynamic-IP setups. The");
    println!("  endpoint shape mirrors the original DynDNS protocol so any");
    println!("  client supporting DynDNS works with easyDNS unchanged.");
}

fn cmd_backupmx() {
    println!("easyDNS Mailbag + Backup MX");
    println!();
    println!("BACKUP MX");
    println!("  easyDNS's MX servers accept inbound mail for your domain when");
    println!("  your primary mail server is unreachable, then forward when");
    println!("  the primary comes back. Useful for self-hosted mail servers");
    println!("  to ride out brief outages without bouncing senders.");
    println!();
    println!("MAILBAG");
    println!("  Hosted mail forwarding service. Set up");
    println!("  any@yourdomain.com -> realme@gmail.com forwarders with");
    println!("  catch-all and per-rule logic. Optional small mailbox");
    println!("  retention (10-50MB) for users with no real mailbox.");
    println!();
    println!("FULL EMAIL HOSTING");
    println!("  For customers wanting real mailboxes, easyDNS offers an");
    println!("  email-hosting bolt-on built on Open-Xchange. IMAP/POP/SMTP");
    println!("  + webmail, mobile-friendly. Cheaper than Google Workspace");
    println!("  for the single-mailbox-on-own-domain use case.");
    println!();
    println!("DKIM / SPF / DMARC HELPERS");
    println!("  easyDNS's record templates pre-fill SPF / DKIM / DMARC");
    println!("  records correctly for the most common ESPs (Mailgun,");
    println!("  Sendgrid, Postmark) when you wire up forwarding/MX.");
}

fn cmd_plans() {
    println!("easyDNS plans (as of 2024, USD)");
    println!();
    println!("DNS HOSTING TIERS");
    println!("  Standard      $9.95/yr/domain");
    println!("                Basic anycast hosting, standard record types,");
    println!("                IPv6, free DNSSEC, free 24/7 support");
    println!();
    println!("  Pro           $34.95/yr/domain");
    println!("                Adds GTD geo-routing, monitoring, advanced API,");
    println!("                higher query allowances, premium support priority");
    println!();
    println!("  Premium       $69.95/yr/domain");
    println!("                Adds API automation, additional GTM policies,");
    println!("                vanity NS, advanced reporting, dedicated SLA");
    println!();
    println!("  Enterprise    Custom");
    println!("                Custom anycast capacity, vanity-NS branding,");
    println!("                contractual SLA, dedicated CSM");
    println!();
    println!("DOMAIN REGISTRATION");
    println!("  Charged at industry-comparable rates with a slight premium");
    println!("  over Porkbun/Cloudflare ($15-20/yr for .com). The premium");
    println!("  funds the values-driven posture and the registrar-defended-");
    println!("  by-lawyers promise.");
}

fn run_easydns(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "dns" => { cmd_dns(); 0 }
        "domains" => { cmd_domains(); 0 }
        "civil" => { cmd_civil(); 0 }
        "api" => { cmd_api(); 0 }
        "backupmx" => { cmd_backupmx(); 0 }
        "plans" => { cmd_plans(); 0 }
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
        .unwrap_or_else(|| "easydns".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_easydns(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/easydns"), "easydns");
        assert_eq!(basename("C:\\Tools\\easydns.exe"), "easydns.exe");
        assert_eq!(basename("easydns"), "easydns");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("easydns.exe"), "easydns");
        assert_eq!(strip_ext("easydns"), "easydns");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_easydns(&["help".to_string()], "easydns"), 0);
        let _ = run_easydns(&[], "easydns");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_easydns(&["nope".to_string()], "easydns"), 2);
    }
}
