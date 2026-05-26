#![deny(clippy::all)]
//! symantec-cli — personality CLI for Symantec, the once-dominant
//! enterprise-security + consumer-antivirus brand whose enterprise
//! business was acquired by Broadcom in 2019 + whose consumer
//! business spun off as NortonLifeLock + later merged with Avast to
//! become Gen Digital.
//!
//! Founded 1982 in Mountain View, California by Gary Hendrix as a
//! natural-language-processing software company. Pivoted to consumer
//! utilities + antivirus through the late-1980s acquisitions of Peter
//! Norton Computing (1990, the Norton brand) and later Symantec
//! AntiVirus. Through the 1990s + 2000s Symantec became the largest
//! pure-play security software company in the world. The November 2019
//! Broadcom acquisition of the Symantec enterprise business for \$10.7B
//! split the company in two: enterprise + on-prem products became
//! 'Symantec by Broadcom' inside Broadcom Software Group; the consumer
//! brand (Norton + LifeLock) spun out as NortonLifeLock + later merged
//! with Avast in 2022 to form Gen Digital (NASDAQ:GEN).

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Symantec (the enterprise-security pioneer) personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Hendrix 1982 Mountain View; once world's largest pure-security");
    println!("    history       NLP origin -> Norton acq 1990 -> Veritas merger -> Broadcom split");
    println!("    broadcom      Broadcom acquires enterprise business 2019 \\$10.7B");
    println!("    norton        Norton consumer business -> NortonLifeLock -> Gen Digital 2022");
    println!("    endpoint      Symantec Endpoint Protection + Endpoint Security Complete");
    println!("    dlp           Symantec DLP — the enterprise data-loss-prevention category leader");
    println!("    veritas       Veritas Software 2005 merger + 2016 demerger");
    println!("    customers     Fortune 500 + government + the long-tail of legacy SEP installs");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("symantec-cli 0.1.0 (enterprise-security-pioneer personality build)"); }

fn run_about() {
    println!("Symantec / Broadcom Symantec Enterprise.");
    println!("  Founded:    1982, Mountain View, California by Gary Hendrix.");
    println!("  Original product: natural-language Q&A interface for databases (Q&A).");
    println!("  Pivot:      consumer utilities + antivirus via Peter Norton Computing acq 1990.");
    println!("  Peak:       ~2008 — world's largest pure-play security software company,");
    println!("              ~\\$6B annual revenue, Fortune 500 enterprise-IT staple.");
    println!("  Stock:      NASDAQ:SYMC public 1989-2019.");
    println!("  Split (2019): Broadcom acquires the enterprise business for \\$10.7B;");
    println!("                consumer Norton + LifeLock business spins off as NortonLifeLock.");
    println!("  Today:      Symantec brand survives inside Broadcom Software Group as");
    println!("              'Symantec Enterprise Cloud' alongside Carbon Black + VMware.");
}

fn run_history() {
    println!("Compressed corporate history.");
    println!("  1982:  Gary Hendrix founds Symantec in Mountain View — NLP software origin.");
    println!("  1984:  Q&A natural-language database query product ships.");
    println!("  1989:  IPO on NASDAQ:SYMC.");
    println!("  1990:  acquires Peter Norton Computing — Norton consumer brand enters portfolio.");
    println!("  1990s: rolls up Central Point, Fifth Generation, Delrina, IBM ANYTIME, Quarterdeck.");
    println!("  2005:  \\$13.5B merger with Veritas Software (storage management).");
    println!("  2016:  Veritas demerged + sold to Carlyle for \\$8B; Symantec becomes pure security.");
    println!("  2016:  Symantec acquires Blue Coat Systems for \\$4.65B (web + cloud security).");
    println!("  2017:  acquires LifeLock for \\$2.3B (identity-theft protection).");
    println!("  2019:  Broadcom buys Symantec enterprise for \\$10.7B; consumer half spun off.");
    println!("  2022:  NortonLifeLock + Avast merge to form Gen Digital (NASDAQ:GEN).");
}

fn run_broadcom() {
    println!("The Broadcom enterprise acquisition (2019).");
    println!("  November 2019: Broadcom buys Symantec's enterprise business for \\$10.7B cash.");
    println!("  Deal architect: Hock Tan + the post-CA Technologies (Broadcom acq 2018) playbook");
    println!("  of large pure-software roll-ups inside the Broadcom Software Group.");
    println!("  Strategy: massively focus the business on the top ~2,000 strategic customers;");
    println!("  dramatically thin out the long tail of smaller accounts (channel pruning,");
    println!("  product retirements, price increases) — classic Broadcom enterprise playbook.");
    println!("  Resulting Symantec Enterprise inside Broadcom: SEP + DLP + ProxySG + CASB +");
    println!("  Symantec Endpoint Security Complete + the Web Security Service + SSL Visibility.");
    println!("  Pattern repeated later with the VMware + Carbon Black + Tanzu acquisitions.");
}

fn run_norton() {
    println!("Norton consumer business -> NortonLifeLock -> Gen Digital.");
    println!("  Norton Antivirus: introduced in the 1990s after the Peter Norton acquisition,");
    println!("                    a household-name consumer antivirus brand for decades.");
    println!("  Norton 360: bundle of antivirus + VPN + password manager + cloud backup.");
    println!("  LifeLock: identity-theft monitoring + restoration, acquired 2017 for \\$2.3B.");
    println!("  NortonLifeLock: 2019 spin-off post-Broadcom-deal, NASDAQ:NLOK.");
    println!("  Gen Digital: 2022 merger of NortonLifeLock + Avast (Czech security vendor) into");
    println!("  one publicly-traded consumer-security holding (NASDAQ:GEN). Brands continued:");
    println!("  Norton + Avast + AVG + LifeLock + Avira + CCleaner + ReputationDefender.");
    println!("  ~500M+ users worldwide across the family of brands.");
}

fn run_endpoint() {
    println!("Symantec Endpoint Protection + Endpoint Security Complete.");
    println!("  SEP (Symantec Endpoint Protection): the long-running enterprise antivirus +");
    println!("  endpoint protection product since the early 2000s, descended from Norton AV +");
    println!("  Sygate (acq 2005) + Whole Security (acq 2005) + several other endpoint brands.");
    println!("  Symantec Endpoint Security Complete: the modern cloud-delivered EDR + EPP +");
    println!("  device-control + application-control bundle aimed at displacing legacy SEP.");
    println!("  Adaptive Protection, Behavioral Analysis, Active Directory Defense, Threat");
    println!("  Hunter Service Layer subscription.");
    println!("  Competitive position: holds large legacy install base, faces hard pressure from");
    println!("  CrowdStrike + SentinelOne + Microsoft Defender for Endpoint on net-new business.");
}

fn run_dlp() {
    println!("Symantec DLP — Data Loss Prevention.");
    println!("  Symantec DLP is widely regarded as the category-defining + market-leading");
    println!("  enterprise data-loss-prevention product, originally the Vontu acquisition (2007).");
    println!("  Coverage: endpoint DLP, network DLP, storage DLP, email DLP, cloud + CASB DLP.");
    println!("  Detection: fingerprinting, exact-data-match, vector machine learning, dictionaries.");
    println!("  Discovery: scan endpoints + file shares + SharePoint + cloud storage for sensitive");
    println!("  data at rest; classify + remediate.");
    println!("  Information-Centric Encryption + Tagging: data-protection-by-classification flow.");
    println!("  Common in financial services + healthcare + government compliance programmes.");
    println!("  Inside Broadcom: continues as the enterprise DLP standard for the strategic top accounts.");
}

fn run_veritas() {
    println!("Veritas Software — the 2005 merger + 2016 demerger.");
    println!("  2005:  Symantec + Veritas Software merge in a \\$13.5B all-stock deal — the");
    println!("         largest software merger to that date. Combined entity intended to cover");
    println!("         security (Symantec) + storage + availability (Veritas) for the enterprise.");
    println!("  Outcome: cross-sell never materialised at the level the deal premise required;");
    println!("           operational + cultural integration was difficult through the late 2000s.");
    println!("  2016:  Symantec demerges Veritas, selling to a Carlyle Group-led consortium for");
    println!("         ~\\$8B. Veritas continues as a private-equity-owned storage + backup vendor.");
    println!("  Symantec post-2016: refocuses on pure security via Blue Coat acq + LifeLock acq.");
    println!("  A canonical case study in the 'mega-merger of two adjacent enterprise categories'");
    println!("  pattern that the software industry has tried + mostly abandoned since.");
}

fn run_customers() {
    println!("Customer profile.");
    println!("  Sweet spot (today): the ~2,000 strategic Broadcom Software Group accounts —");
    println!("  Fortune 500 + Global 2000 enterprises + governments running long-tenured SEP +");
    println!("  DLP + ProxySG + Web Security Service deployments.");
    println!("  Industries: financial services, government + defence, healthcare, utilities,");
    println!("  oil + gas, telecoms, large manufacturing — anywhere a multi-decade Symantec");
    println!("  enterprise relationship survived the Broadcom strategic-account rationalisation.");
    println!("  Geographic: heavy US + EU + APAC + LATAM enterprise; long-tail SMB segment is");
    println!("  no longer a Broadcom Symantec focus.");
    println!("  Norton consumer: ~500M+ users globally inside Gen Digital's brand family.");
    println!("  Anti-segment (today): net-new cloud-first SMB + mid-market (gone to CrowdStrike,");
    println!("  SentinelOne, Microsoft Defender, Sophos, ESET, Bitdefender).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "symantec-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "history" => run_history(),
        "broadcom" => run_broadcom(),
        "norton" => run_norton(),
        "endpoint" => run_endpoint(),
        "dlp" => run_dlp(),
        "veritas" => run_veritas(),
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
        run_history();
        run_broadcom();
        run_norton();
        run_endpoint();
        run_dlp();
        run_veritas();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("symantec-cli");
        print_version();
    }
}
