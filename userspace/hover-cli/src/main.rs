#![deny(clippy::all)]
//! hover-cli — Slate OS personality CLI for Hover, the Tucows-owned no-upsell registrar.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Hover — domains for the curious. By Tucows. No upsells.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       The Tucows lineage and Hover's anti-upsell launch");
    println!("    domains     Registration, valets, transfer concierge");
    println!("    email       Hover Email — bare-essentials hosted mail");
    println!("    parent      Tucows: holdings, OpenSRS, Ting, mobile");
    println!("    pricing     The transparent renewal-price philosophy");
    println!("    privacy     Free WHOIS privacy and no-data-selling stance");
    println!("    valet       Hover Valet — concierge transfer service");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("\"It's just domains. We just do them well.\"");
}

fn print_version() {
    println!("hover-cli 0.1.0");
    println!("Hover — operated by Tucows, Inc. Toronto, Ontario. Launched 2008.");
}

fn cmd_about() {
    println!("Hover");
    println!();
    println!("LAUNCHED");
    println!("  2008 by Tucows as a consumer-facing domain registrar with a");
    println!("  pointed positioning against GoDaddy: no upsells in the cart,");
    println!("  no aggressive ads, no surcharges. Brand voice was 'domains");
    println!("  for people who actually love the Internet' — a Mom-and-Pop");
    println!("  bookstore in a market dominated by big-box retailers.");
    println!();
    println!("HEADQUARTERS");
    println!("  Toronto, Canada (Tucows HQ). ~30 employees focused on Hover");
    println!("  (small team, shared infrastructure with Tucows-at-large).");
    println!();
    println!("BRAND PERSONALITY");
    println!("  Long-running 'Hover.com Show' podcast and developer-friendly");
    println!("  blog. Major sponsor of programming podcasts (notably The Talk");
    println!("  Show, Pen Addict, and the early Marco.org podcast pool) in");
    println!("  the 2010s, building developer mindshare.");
    println!();
    println!("WHO IT'S FOR");
    println!("  Developers, designers, indie creators, and bloggers who want");
    println!("  one or two domains for personal projects or small businesses,");
    println!("  managed through a clean UI without a cart upsell process.");
}

fn cmd_domains() {
    println!("Hover domain services");
    println!();
    println!("REGISTRATION");
    println!("  ~300 TLDs supported: all major gTLDs, most popular ccTLDs,");
    println!("  premium .com / .net / .org pricing close to wholesale.");
    println!("  No teaser-price tricks; the renewal price is the registration");
    println!("  price (with rare exceptions clearly labeled).");
    println!();
    println!("TRANSFERS");
    println!("  Inbound transfers from GoDaddy/Network Solutions/etc. are a");
    println!("  click-through process. Hover credits a free year onto the");
    println!("  domain when transferred in (industry standard, but Hover honors");
    println!("  it even on .uk and other ccTLDs where many registrars don't).");
    println!();
    println!("RECORDS");
    println!("  Standard DNS record management UI for A, AAAA, CNAME, MX, TXT,");
    println!("  SRV, CAA. URL forwarding (301/302), email forwarding, DNSSEC.");
    println!();
    println!("WILDCARDS");
    println!("  Wildcard records, ALIAS-style apex behavior for popular hosts");
    println!("  (Squarespace, Tumblr), and 'Connect with Google Workspace'");
    println!("  one-click MX setup.");
}

fn cmd_email() {
    println!("Hover Email");
    println!();
    println!("WHAT IT IS");
    println!("  Hosted email tied to a domain registered (or just managed)");
    println!("  through Hover. Built on the Open-Xchange platform under the");
    println!("  hood; reskinned as Hover Email with a focused, ad-free UI.");
    println!();
    println!("PLANS");
    println!("  Big Mailbox     $20/yr per mailbox  10GB storage, IMAP/POP/SMTP,");
    println!("                                     webmail, forwarding rules");
    println!("  Little Mailbox  $5/yr per mailbox   no storage; pure forward-only,");
    println!("                                     useful for catch-all aliases");
    println!();
    println!("FEATURES");
    println!("  - SPF / DKIM / DMARC handled automatically");
    println!("  - Mobile-friendly webmail at hover-email.com");
    println!("  - Aliases (multiple incoming addresses route to one mailbox)");
    println!("  - Auto-responders");
    println!("  - 24/7 support included in the price");
    println!();
    println!("POSITIONING");
    println!("  Targeted at people who want their own email on their own");
    println!("  domain without committing to a $6+/user/mo Google Workspace");
    println!("  subscription. Common for personal pages, small portfolio sites,");
    println!("  and solo consultants.");
}

fn cmd_parent() {
    println!("Tucows — Hover's parent company");
    println!();
    println!("CORPORATE STRUCTURE");
    println!("  Tucows, Inc. (NASDAQ: TCX) is a publicly-traded Canadian");
    println!("  Internet services company headquartered in Toronto. Founded");
    println!("  1993 as 'The Ultimate Collection of Winsock Software' — the");
    println!("  download portal for Windows shareware in the dial-up era.");
    println!();
    println!("BUSINESS SEGMENTS");
    println!("  - Domain Services (Tucows Domains)  ~$300M annual revenue:");
    println!("       OpenSRS (wholesale registrar serving ~10K reseller hosts");
    println!("       and SaaS platforms), Enom (acquired 2017), Hover (retail),");
    println!("       Ascio (acquired 2019, European corporate registrar).");
    println!();
    println!("  - Tucows Mobile (Wavelo + Ting Wireless)  ~$240M revenue:");
    println!("       MVNO + telecom SaaS platform sold to ISPs/MVNOs as the");
    println!("       'cloud-native BSS' for billing + provisioning. Wavelo");
    println!("       customers include the Ting brand + DISH Wireless.");
    println!();
    println!("  - Ting Internet (fiber)              ~$110M revenue:");
    println!("       Fiber-to-the-home in select US cities (Charlottesville,");
    println!("       Sandpoint, Holly Springs, Centennial). Capital-intensive");
    println!("       segment under scrutiny in 2024 cost-discipline efforts.");
    println!();
    println!("Tucows is one of the world's largest domain registrars by");
    println!("aggregate volume (~25M domains across OpenSRS+Enom+Hover+Ascio),");
    println!("though most of that volume is wholesale, not retail.");
}

fn cmd_pricing() {
    println!("Hover's transparent pricing");
    println!();
    println!("THE PHILOSOPHY");
    println!("  No teaser-price renewals. The price on the product page is");
    println!("  the price you'll pay in year two, year five, and year ten.");
    println!("  When registry pricing changes (Verisign raises .com prices,");
    println!("  ICANN restructures fees), Hover discloses the timing and");
    println!("  amount publicly months in advance.");
    println!();
    println!("REPRESENTATIVE PRICING (USD, 2024)");
    println!("  .com          $17.99/yr  (vs. GoDaddy first-year $11.99, renewal ~$22)");
    println!("  .net          $19.99/yr");
    println!("  .org          $19.99/yr");
    println!("  .io           $66.99/yr  (matches registry-driven uplift)");
    println!("  .dev          $19.99/yr");
    println!("  .app          $19.99/yr");
    println!("  .ai           ~$94/yr     (registry-driven; .ai is two-year minimum)");
    println!("  .me           $24.99/yr");
    println!();
    println!("WHY NOT CHEAPER?");
    println!("  Hover positions itself between Namecheap's promo-aggressive");
    println!("  pricing and GoDaddy's upsell-heavy model. The premium funds");
    println!("  fully-staffed Canadian-based support and the no-upsell pledge.");
}

fn cmd_privacy() {
    println!("Hover's privacy posture");
    println!();
    println!("FREE WHOIS PRIVACY");
    println!("  Free on every eligible domain since launch (2008) — Hover was");
    println!("  one of the first registrars to make this the default, years");
    println!("  before GDPR forced the industry to comply.");
    println!();
    println!("NO DATA SELLING");
    println!("  Hover's privacy policy disavows selling customer data to");
    println!("  advertisers or data brokers. The company has publicly opposed");
    println!("  ICANN policy proposals that would re-open WHOIS data, citing");
    println!("  the original 1980s mistake of including registrant home");
    println!("  addresses in public records.");
    println!();
    println!("TRANSPARENCY REPORTS");
    println!("  Tucows publishes annual transparency reports detailing law-");
    println!("  enforcement requests received across all its registrars,");
    println!("  including how many were granted, denied, or contested. One");
    println!("  of only a handful of registrars to publish such data.");
    println!();
    println!("REGISTRANT PROTECTIONS");
    println!("  Tucows operates the 'Tucows Information Officer' program —");
    println!("  an internal advocate role mandated to push back on government");
    println!("  takedown requests that exceed legal authority. Public posts");
    println!("  occasionally detail specific cases where requests were denied.");
}

fn cmd_valet() {
    println!("Hover Valet — concierge transfer service");
    println!();
    println!("THE PITCH");
    println!("  You have ~20 domains scattered across GoDaddy, Network Solutions,");
    println!("  Dotster, Crazy Domains, and three free-trial registrars from");
    println!("  2009 you can't remember the logins to. Hover Valet says:");
    println!("  hand us the spreadsheet and we'll do the transfers for you.");
    println!();
    println!("THE PROCESS");
    println!("  1. You email support a list of domains you want to transfer in.");
    println!("  2. Hover handles login recovery, unlock requests, EPP codes,");
    println!("     and post-transfer DNS rebuilds.");
    println!("  3. You approve charges; Hover does the registrar dance.");
    println!();
    println!("COST");
    println!("  Free for moderate quantities; flat fees on bulk transfers.");
    println!("  The cost is mostly the registration extension fee (1 year");
    println!("  added per transfer) plus a small service surcharge.");
    println!();
    println!("WHY IT MATTERS");
    println!("  Anyone who has managed dozens of domains across multiple");
    println!("  registrars knows the transfer process is the worst part of");
    println!("  any registrar's user experience — and the deliberate friction");
    println!("  that retains customers at upselling registrars. Valet exists");
    println!("  to short-circuit that friction.");
}

fn run_hover(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "domains" => { cmd_domains(); 0 }
        "email" => { cmd_email(); 0 }
        "parent" => { cmd_parent(); 0 }
        "pricing" => { cmd_pricing(); 0 }
        "privacy" => { cmd_privacy(); 0 }
        "valet" => { cmd_valet(); 0 }
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
        .unwrap_or_else(|| "hover".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_hover(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/hover"), "hover");
        assert_eq!(basename("C:\\Tools\\hover.exe"), "hover.exe");
        assert_eq!(basename("hover"), "hover");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("hover.exe"), "hover");
        assert_eq!(strip_ext("hover"), "hover");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_hover(&["help".to_string()], "hover"), 0);
        let _ = run_hover(&[], "hover");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_hover(&["nope".to_string()], "hover"), 2);
    }
}
