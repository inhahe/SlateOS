#![deny(clippy::all)]
//! porkbun-cli — SlateOS personality CLI for Porkbun, the playful low-price registrar.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Porkbun — an oddly satisfying domain registrar.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Top Level Design + the pig in the trenchcoat");
    println!("    pricing     The .app / .dev / .com / .design cheap-renewals story");
    println!("    domains     Registration, transfers, and Glue records");
    println!("    api         The Porkbun REST API and dynamic DNS");
    println!("    perks       Free Whois privacy, free SSL, free email forwarding");
    println!("    knife       The Pork Knife domain-research tool");
    println!("    weird       Trade-mark cease-and-desist deflection, weird-design TLDs");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Snouts down, hooves up — registering since 2014.");
}

fn print_version() {
    println!("porkbun-cli 0.1.0");
    println!("Porkbun LLC — Portland, Oregon. Founded 2014.");
}

fn cmd_about() {
    println!("Porkbun");
    println!();
    println!("FOUNDED");
    println!("  2014 in Portland, Oregon, as a sister company to Top Level");
    println!("  Design (TLD) — the registry operator for .design, .ink, .wiki,");
    println!("  and originally co-operator of .design. Porkbun was launched as");
    println!("  a retail registrar to give TLD's portfolio direct distribution");
    println!("  and to be the kind of registrar developers like the founders");
    println!("  actually wanted to use.");
    println!();
    println!("HEADQUARTERS");
    println!("  Portland, OR. Tiny team. Privately held. No VC. No PE.");
    println!();
    println!("THE PIG");
    println!("  Porkbun's mascot is a pig in a trenchcoat named 'PorkBun'");
    println!("  (yes, capitalized that way in mascot canon). The pig appears");
    println!("  on confirmation screens, customer support emails, and the");
    println!("  occasional April Fools joke. The 'oddly satisfying' tagline");
    println!("  is unironic — they style the whole product around being a");
    println!("  pleasant, non-aggressive registrar experience.");
    println!();
    println!("SCALE");
    println!("  As of 2024: ~600K+ domains under management, growing through");
    println!("  word-of-mouth on Hacker News, Reddit /r/domains, and the");
    println!("  indie-hacker / side-project demographic. Profitable.");
}

fn cmd_pricing() {
    println!("Porkbun's aggressively low pricing");
    println!();
    println!("REPRESENTATIVE TLDS (USD, 2024 renewal prices)");
    println!("  .com          ~$10.37/yr   (among the cheapest US-based registrars)");
    println!("  .net          ~$13.40/yr");
    println!("  .org          ~$11.92/yr");
    println!("  .app          ~$13.41/yr   (was a flagship discount TLD)");
    println!("  .dev          ~$13.65/yr");
    println!("  .io           ~$36.94/yr");
    println!("  .ai           ~$76.69/yr   (registry-driven, 2-year minimum)");
    println!("  .design       ~$23/yr      (registry operator is their parent!)");
    println!("  .me           ~$15/yr");
    println!("  .xyz          ~$11.20/yr");
    println!();
    println!("RENEWAL TRANSPARENCY");
    println!("  The intro and renewal price are usually within $1-2 of each");
    println!("  other (unlike GoDaddy's typical 50%+ renewal markup).");
    println!("  Periodic transparency posts on porkbun.com/blog explain");
    println!("  when registry-side wholesale changes affect pricing.");
}

fn cmd_domains() {
    println!("Porkbun domain services");
    println!();
    println!("REGISTRATION");
    println!("  ~500 TLDs. All major gTLDs, most ccTLDs, every new gTLD that");
    println!("  Top Level Design operates (.design, .ink, .wiki, others) at");
    println!("  often-promotional pricing.");
    println!();
    println!("TRANSFER-IN");
    println!("  Transfer credits one year (industry standard). Concierge-style");
    println!("  support for bulk transfers; just email support with a list");
    println!("  and they'll help walk through the EPP-code dance.");
    println!();
    println!("DNS")
    ;
    println!("  Anycast nameservers (porkbun's own DNS clusters), DNSSEC,");
    println!("  GLUE records, ALIAS-like apex-A behavior for popular hosts.");
    println!("  Full record-type support including SRV, CAA, HTTPS, TLSA.");
    println!();
    println!("AUCTIONS");
    println!("  Hosts a small expired-domains auction. Not industry-leading");
    println!("  volume — Sedo / Afternic / Dropcatch dominate that segment.");
    println!("  Bulk-search and 'mock-name' generator help find creative names");
    println!("  without going to the aftermarket.");
}

fn cmd_api() {
    println!("Porkbun REST API");
    println!();
    println!("BASE URL");
    println!("  https://api.porkbun.com/api/json/v3/");
    println!();
    println!("AUTH");
    println!("  POST-body credentials: {{ apikey: ..., secretapikey: ... }}");
    println!("  Generate keys in the customer portal; revoke any time.");
    println!();
    println!("RESOURCES");
    println!("  /ping                                   sanity check");
    println!("  /pricing/get                            current TLD prices");
    println!("  /domain/listAll                         your domains");
    println!("  /domain/getNs/<domain>                  nameservers");
    println!("  /domain/updateNs/<domain>               set nameservers");
    println!("  /dns/retrieve/<domain>                  list DNS records");
    println!("  /dns/create/<domain>                    create a record");
    println!("  /dns/edit/<domain>/<id>                 edit a record");
    println!("  /dns/delete/<domain>/<id>               delete a record");
    println!("  /ssl/retrieve/<domain>                  retrieve cert + key");
    println!();
    println!("DYNAMIC DNS")
    ;
    println!("  Update an A/AAAA record from a script (cron + curl + your");
    println!("  current public IP). Many home-lab + self-hosted setups");
    println!("  use this to keep a porkbun-hosted hostname pointing at");
    println!("  a residential dynamic IP.");
}

fn cmd_perks() {
    println!("Porkbun's bundled perks (all free)");
    println!();
    println!("FREE WHOIS PRIVACY");
    println!("  Standard for all eligible TLDs; on by default. Not even");
    println!("  shown as a checkbox during registration — it just is.");
    println!();
    println!("FREE SSL (LET'S ENCRYPT)");
    println!("  Porkbun automatically requests + provisions a Let's Encrypt");
    println!("  certificate for every domain on Porkbun DNS. Cert + key");
    println!("  retrievable via /ssl/retrieve API. Useful for self-hosted");
    println!("  servers and indie SaaS apps that don't want to run certbot.");
    println!();
    println!("FREE EMAIL FORWARDING");
    println!("  Set up forward@yourdomain.com to send to a real Gmail/etc");
    println!("  inbox. Catch-all and per-address rules supported. No daily");
    println!("  forward limit (well, within reasonable spam-prevention rates).");
    println!();
    println!("FREE TRIVIAL HOSTING")
    ;
    println!("  Porkbun hosts a one-page redirect or simple static landing");
    println!("  page on every domain at no extra cost. Useful for parking");
    println!("  domains before you build the real site.");
    println!();
    println!("PRICING PHILOSOPHY");
    println!("  All of the above used to be paid add-ons at most registrars.");
    println!("  Porkbun moved them into the base price; the registrar charges");
    println!("  margin on registration, not on the obvious complementary goods.");
}

fn cmd_knife() {
    println!("Pork Knife — Porkbun's domain research tool");
    println!();
    println!("WHAT IT IS");
    println!("  An online domain-name research utility (porkbun.com/knife)");
    println!("  with bulk-availability checks, name-generator brainstorming,");
    println!("  and price comparison across TLDs.");
    println!();
    println!("FEATURES");
    println!("  - Multi-TLD search: type 'mybrand' and see availability +");
    println!("    pricing across .com / .net / .org / .io / .dev / .app / .xyz");
    println!("    and ~100 others in one shot.");
    println!("  - Bulk lookup: paste a CSV of 500 candidates and see results.");
    println!("  - Name suggestions: prefix/suffix synthesis + dictionary mashups.");
    println!("  - WHOIS lookup that respects modern redaction without throwing");
    println!("    nag messages.");
    println!();
    println!("WHY IT EXISTS");
    println!("  Other registrars' search UIs are intentionally cluttered to");
    println!("  upsell premium domains and add-on services. Pork Knife is the");
    println!("  unbundled tool a developer actually wants: name -> available -> price.");
}

fn cmd_weird() {
    println!("The Porkbun weirdness file");
    println!();
    println!("TLD CATALOG QUIRKS");
    println!("  Parent company Top Level Design runs .design, .ink, and .wiki,");
    println!("  which explains why Porkbun has unusually deep pricing on those.");
    println!("  Promotional cycles for these TLDs (often $1-3 first year)");
    println!("  drive a meaningful share of new registrations.");
    println!();
    println!("PUMA SE LITIGATION (2023)");
    println!("  Porkbun got sued by Puma SE (the athletic-shoe company) over");
    println!("  a registered customer-owned 'puma-related' domain. Porkbun");
    println!("  publicly fought back via blog post and customer-rights stance,");
    println!("  raising visibility of UDRP overreach issues. (Resolved.)");
    println!();
    println!("HOLIDAY DOMAINS");
    println!("  Porkbun runs ~year-round promotional weeks for niche TLDs:");
    println!("  'Pride Month -> .gay at $5', 'Winter -> .ski at $4', etc.");
    println!("  These create ongoing community engagement.");
    println!();
    println!("APRIL FOOLS");
    println!("  Annual joke products that aren't quite jokes: Porkbun mailed");
    println!("  real shipped t-shirts for the 2022 'porkbun.coffee' April");
    println!("  Fools coffee subscription; later turned the .coffee promo");
    println!("  into a recurring discount.");
    println!();
    println!("ETHOS");
    println!("  Porkbun has no Series A to satisfy. Every quirky decision");
    println!("  comes from a small team that has fun with the product.");
    println!("  The pig isn't a focus-group mascot; it's a vibe.");
}

fn run_porkbun(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "pricing" => { cmd_pricing(); 0 }
        "domains" => { cmd_domains(); 0 }
        "api" => { cmd_api(); 0 }
        "perks" => { cmd_perks(); 0 }
        "knife" => { cmd_knife(); 0 }
        "weird" => { cmd_weird(); 0 }
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
        .unwrap_or_else(|| "porkbun".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_porkbun(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/porkbun"), "porkbun");
        assert_eq!(basename("C:\\Tools\\porkbun.exe"), "porkbun.exe");
        assert_eq!(basename("porkbun"), "porkbun");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("porkbun.exe"), "porkbun");
        assert_eq!(strip_ext("porkbun"), "porkbun");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_porkbun(&["help".to_string()], "porkbun"), 0);
        let _ = run_porkbun(&[], "porkbun");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_porkbun(&["nope".to_string()], "porkbun"), 2);
    }
}
