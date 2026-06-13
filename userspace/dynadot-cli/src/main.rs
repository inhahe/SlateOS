#![deny(clippy::all)]
//! dynadot-cli — SlateOS personality CLI for Dynadot, the domainer-focused registrar.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Dynadot — the domain investor's registrar.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Todd Han, San Mateo HQ, the domainer focus");
    println!("    auctions    Dynadot Auctions and aftermarket marketplace");
    println!("    bulkops     Bulk renew, bulk register, bulk move");
    println!("    api         The Dynadot REST/XML API");
    println!("    builder     Website Builder for parked domains");
    println!("    pricing     Wholesale-ish pricing for active investors");
    println!("    forum       The Dynadot Forum and domain investor community");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("\"Run by domainers, for domainers.\"");
}

fn print_version() {
    println!("dynadot-cli 0.1.0");
    println!("Dynadot, LLC — San Mateo, California. Founded 2002.");
}

fn cmd_about() {
    println!("Dynadot");
    println!();
    println!("FOUNDED");
    println!("  2002 by Todd Han in San Mateo, California. Han had previously");
    println!("  founded Boku.com (unrelated to today's Boku payments) and");
    println!("  pivoted to building a registrar after seeing how poorly existing");
    println!("  registrars served the burgeoning domain-investor market. The");
    println!("  thesis: build a registrar that domainers (people who buy and");
    println!("  sell domain names for a living) would actually want to use.");
    println!();
    println!("HEADQUARTERS");
    println!("  San Mateo, California. Privately held, owner-operated.");
    println!("  ~100 employees. Engineering offices in California + China.");
    println!();
    println!("MARKET POSITIONING");
    println!("  Dynadot's competitive edge in the early 2010s was the bulk");
    println!("  feature set: APIs, mass registration, fast UIs for portfolios");
    println!("  of hundreds or thousands of domains. Today the customer base");
    println!("  is split between high-volume investors and developers who");
    println!("  appreciate the clean UI + competitive pricing.");
    println!();
    println!("SCALE");
    println!("  ~6M domains under management. Among the top-15 ICANN registrars");
    println!("  by volume. Significantly smaller than GoDaddy/Tucows/Namecheap");
    println!("  in customer count but with much higher domain-per-customer.");
}

fn cmd_auctions() {
    println!("Dynadot Auctions");
    println!();
    println!("WHAT IT IS");
    println!("  An aftermarket marketplace for buying and selling premium and");
    println!("  expiring domains. Two main streams:");
    println!("    1. Closeouts — expiring domains being dropped by Dynadot");
    println!("       customers. Fixed-price first-come basis.");
    println!("    2. Marketplace — user-listed inventory with seller pricing.");
    println!();
    println!("BACKORDER + EXPIRING-AUCTION PIPELINE");
    println!("  When a Dynadot-registered domain enters the redemption grace");
    println!("  period (not renewed by current owner), it goes into Dynadot's");
    println!("  expiring auction stream. Backorders allow non-Dynadot domains");
    println!("  too, via partner drop-catching services.");
    println!();
    println!("ESCROW + TRANSFERS");
    println!("  Dynadot operates as escrow agent for marketplace transactions.");
    println!("  In-network buyer/seller transfers (both at Dynadot) are");
    println!("  near-instant; out-of-network requires push to a registrar");
    println!("  account at the buyer's chosen registrar.");
    println!();
    println!("COMMISSIONS");
    println!("  ~10% seller commission on marketplace sales. Closeouts and");
    println!("  expiring auctions are direct Dynadot inventory, not commissioned.");
}

fn cmd_bulkops() {
    println!("Bulk operations for domain investors");
    println!();
    println!("BULK SEARCH");
    println!("  Paste a list of up to 5,000 names; Dynadot returns availability");
    println!("  + pricing for each across TLDs you choose. Useful for");
    println!("  evaluating large keyword + dictionary expansions.");
    println!();
    println!("BULK REGISTER");
    println!("  Add 100s of domains to cart, checkout in one transaction.");
    println!("  Useful when a new TLD launches in landrush phase or when");
    println!("  you've finished a brainstorm session.");
    println!();
    println!("BULK RENEW");
    println!("  Renew 10s-1000s of domains at once. Most domainer portfolios");
    println!("  have hundreds of renewals due in a given month; doing this");
    println!("  one-by-one in a typical registrar UI is excruciating.");
    println!();
    println!("BULK MODIFY DNS / WHOIS / NS");
    println!("  Update DNS records, registrant contact details, nameservers,");
    println!("  privacy settings — across many domains at once.");
    println!();
    println!("BULK PUSH / TRANSFER");
    println!("  In-network 'push' between Dynadot accounts is free + instant.");
    println!("  Useful when reorganizing a portfolio across multiple buyer");
    println!("  shells or selling chunks of inventory between investors.");
}

fn cmd_api() {
    println!("Dynadot API");
    println!();
    println!("BASE URL");
    println!("  https://api.dynadot.com/api3.xml      XML response");
    println!("  https://api.dynadot.com/api3.json     JSON response");
    println!();
    println!("AUTH");
    println!("  ?key=<api-key>&command=<command>&...args...");
    println!("  IP allowlist + per-command rate limiting.");
    println!();
    println!("COMMANDS");
    println!("  search                       check availability");
    println!("  register                     register a domain");
    println!("  bulk_search                  bulk availability");
    println!("  bulk_register                bulk registration");
    println!("  domain_info                  status, contacts, NS, expiry");
    println!("  set_ns                       set nameservers");
    println!("  set_dns                      set DNS records (A/CNAME/MX/TXT/etc.)");
    println!("  set_forwarding               URL forwarding");
    println!("  renew_domain                 renew a domain");
    println!("  transfer_domain              start an inbound transfer");
    println!("  push_domain                  push between Dynadot accounts");
    println!("  get_contacts/set_contacts    registrant contact management");
    println!();
    println!("THIRD-PARTY INTEGRATIONS");
    println!("  Used by aftermarket aggregators (NameBio, NameJet, Estibot,");
    println!("  GoDaddy Auctions price tools) for inventory tracking, sale-");
    println!("  comparable data, and bulk-monitoring of customer portfolios.");
}

fn cmd_builder() {
    println!("Dynadot Website Builder");
    println!();
    println!("WHAT IT IS");
    println!("  A simple drag-drop site builder included with Dynadot accounts.");
    println!("  Designed for the use case 'I own this domain, I want a one-");
    println!("  page lander on it without spinning up hosting.'");
    println!();
    println!("PARKED-DOMAIN MONETIZATION");
    println!("  For domainers with hundreds of unused domains, Dynadot");
    println!("  provides a free parking page that displays type-in ads.");
    println!("  Revenue share is paid back to the domain owner. Not a major");
    println!("  revenue stream individually but at portfolio scale (1000s of");
    println!("  domains) it can offset registration costs.");
    println!();
    println!("FOR-SALE LANDERS");
    println!("  A different parking template for domains explicitly listed");
    println!("  for sale. Includes 'Make Offer' contact form + tracking.");
    println!("  Most major aftermarket brokers (Sedo, Afternic, Dan, Dynadot");
    println!("  Marketplace) point sold domains through these landers");
    println!("  pre-purchase.");
    println!();
    println!("WHY IT'S NOT WIX");
    println!("  Dynadot's builder isn't a Wix/Squarespace competitor and");
    println!("  isn't trying to be. It's a utility for domain owners who");
    println!("  need a presence on a domain without investing in real hosting.");
}

fn cmd_pricing() {
    println!("Dynadot pricing (as of 2024)");
    println!();
    println!("RETAIL TLDS (representative)");
    println!("  .com          $10.99/yr    intro and renewal essentially equal");
    println!("  .net          $11.99/yr");
    println!("  .org          $12.99/yr");
    println!("  .io           $34.99/yr");
    println!("  .ai           ~$70/yr (2-year minimum)");
    println!("  .app          $13.99/yr");
    println!("  .dev          $14.99/yr");
    println!("  .me           $19.99/yr");
    println!("  .xyz          $10.99/yr");
    println!();
    println!("DOMAINER VOLUME TIERS");
    println!("  Standard       0+ domains       retail pricing");
    println!("  Bronze         50+ domains      $0.20-0.50 off most TLDs");
    println!("  Silver         100+ domains     ~$0.50-1 off");
    println!("  Gold           500+ domains     wholesale-ish pricing");
    println!("  Platinum       2,000+ domains   approaches registry cost");
    println!("  Diamond        5,000+ domains   special account team");
    println!();
    println!("FREE WHOIS PRIVACY");
    println!("  Included on all eligible domains. Free SSL via integrations.");
    println!("  Email forwarding free; full mailbox hosting paid.");
}

fn cmd_forum() {
    println!("The Dynadot Forum");
    println!();
    println!("WHAT IT IS");
    println!("  forum.dynadot.com — one of the longest-running domain investor");
    println!("  forums on the Internet (older than NamePros' current incarnation).");
    println!("  Active threads on auction strategy, drop-catching, premium-name");
    println!("  appraisal, TLD launch hauls, and registry policy changes.");
    println!();
    println!("WHY IT WORKS");
    println!("  Dynadot staff (including senior product folks) participate in");
    println!("  threads, post feature announcements, respond to bug reports,");
    println!("  and engage with the customer base directly. The forum doubles");
    println!("  as a public roadmap discussion + support channel.");
    println!();
    println!("PROMINENT ALUMNI");
    println!("  Major domain investors (some active publicly, many anonymous)");
    println!("  who have made the forum their default discussion home for 15+");
    println!("  years. The institutional knowledge accumulated in the forum");
    println!("  is arguably as much an asset as Dynadot's software itself.");
    println!();
    println!("CULTURAL ROLE");
    println!("  Where domainer-specific feature requests (improved bulk-search,");
    println!("  faster API, more aggressive auction-bidding capabilities)");
    println!("  surface, get debated, and frequently get built.");
}

fn run_dynadot(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "auctions" => { cmd_auctions(); 0 }
        "bulkops" => { cmd_bulkops(); 0 }
        "api" => { cmd_api(); 0 }
        "builder" => { cmd_builder(); 0 }
        "pricing" => { cmd_pricing(); 0 }
        "forum" => { cmd_forum(); 0 }
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
        .unwrap_or_else(|| "dynadot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_dynadot(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/dynadot"), "dynadot");
        assert_eq!(basename("C:\\Tools\\dynadot.exe"), "dynadot.exe");
        assert_eq!(basename("dynadot"), "dynadot");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("dynadot.exe"), "dynadot");
        assert_eq!(strip_ext("dynadot"), "dynadot");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_dynadot(&["help".to_string()], "dynadot"), 0);
        let _ = run_dynadot(&[], "dynadot");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_dynadot(&["nope".to_string()], "dynadot"), 2);
    }
}
