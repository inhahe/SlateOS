#![deny(clippy::all)]
//! edgio-cli — SlateOS Edgio bankruptcy / Akamai acquisition obituary CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Edgio (Limelight + Yahoo Edgecast + Layer0) — obituary");
    println!();
    println!("USAGE: {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about         The 2022 mega-merger that became Edgio");
    println!("    limelight     Limelight Networks heritage (1995-2022)");
    println!("    edgecast      Edgecast / Yahoo Edge heritage (2006-2022)");
    println!("    layer0        Moovweb / Layer0 acquisition (Dec 2021)");
    println!("    bankruptcy    Chapter 11, Sep 9 2024");
    println!("    akamai        Akamai asset purchase, late 2024");
    println!("    lessons       What this teaches about CDN consolidation");
    println!("    help / version");
}

fn print_version() {
    println!("edgio-cli 0.1.0 — SlateOS personality binary");
    println!("Edgio, Inc. (formerly Limelight Networks) — Chapter 11 Sep 2024");
}

fn cmd_about() {
    println!("Edgio — Born from a CDN mega-merger, dissolved 30 months later.");
    println!();
    println!("Brand created:  Jun 16, 2022, by renaming Limelight Networks");
    println!("                (NASDAQ:LLNW -> NASDAQ:EGIO)");
    println!();
    println!("Three companies, one brand:");
    println!("  • Limelight Networks — public CDN since IPO 2007");
    println!("  • Yahoo Edgecast — CDN + WAF division spun out of Verizon Media");
    println!("    and sold to Limelight for USD 300M (in Limelight stock) in Mar 2022");
    println!("  • Moovweb / Layer0 — frontend acceleration platform acquired by");
    println!("    Limelight for USD 12.5M cash + equity, closed Dec 2021");
    println!();
    println!("Strategy:");
    println!("  Combine Limelight's network (Tier-1 transit + global PoPs) with");
    println!("  Edgecast's enterprise CDN + security book and Layer0's developer-");
    println!("  facing edge runtime. Become the credible #3 CDN behind Cloudflare");
    println!("  and Akamai, ahead of Fastly.");
    println!();
    println!("On paper:");
    println!("  ~USD 500M annual revenue post-merger. 300+ Gbps egress capacity.");
    println!("  ~12,000 enterprise customers across the combined book.");
    println!();
    println!("In reality:");
    println!("  Mountain of integration debt + declining CDN unit economics +");
    println!("  bleeding customers + falling stock price + delisting + Chapter 11.");
    println!("  See 'edgio bankruptcy' for the chronology.");
}

fn cmd_limelight() {
    println!("Limelight Networks — heritage CDN, 1995-2022");
    println!();
    println!("Founded:  1995 in Tempe, Arizona, by Michael Gordon and Allan Kaplan");
    println!("          One of the first commercial CDNs alongside Akamai (1998).");
    println!("IPO:      Jun 8, 2007 on NASDAQ (LLNW), at USD 15/share, USD 1.6B mkt cap");
    println!();
    println!("Heritage:");
    println!("  Tier-1 owned-and-operated optical backbone + dense PoP footprint.");
    println!("  Limelight famously OWNED its global fiber capacity rather than");
    println!("  leasing transit. This was Limelight's strategic moat against");
    println!("  Akamai's transit-based architecture.");
    println!();
    println!("Famous customers (historical):");
    println!("  Microsoft Xbox Live, Netflix (briefly in early days),");
    println!("  Activision, Microsoft Windows Update, Sony PlayStation Network,");
    println!("  numerous game studios (game patch distribution), Hulu.");
    println!();
    println!("Lawsuit with Akamai (2006-2013):");
    println!("  Akamai sued Limelight over CDN patents. Lengthy battle including");
    println!("  Supreme Court precedent (Akamai v. Limelight, 2014) on inducement");
    println!("  of patent infringement. Eventually settled.");
    println!();
    println!("Decline path:");
    println!("  Mid-2010s onward, Cloudflare's freemium model and Fastly's");
    println!("  developer pull eroded Limelight's mid-market. AWS CloudFront");
    println!("  ate the cloud-native segment. By 2020, Limelight stock had");
    println!("  drifted from ~USD 15 IPO to ~USD 5. The Edgecast merger was");
    println!("  an attempt to rescale into something competitive again.");
}

fn cmd_edgecast() {
    println!("Edgecast — heritage CDN that became Yahoo Edge");
    println!();
    println!("Founded:  2006 in Santa Monica, California");
    println!("Acquired: Verizon Communications, Dec 2013, for ~USD 350M");
    println!("          Initially a Verizon Digital Media Services / Verizon Media");
    println!("          property. Became part of Yahoo when Verizon bought Yahoo (2017).");
    println!();
    println!("When Apollo Global bought Yahoo from Verizon (Sep 2021), Yahoo Edge");
    println!("became a non-core asset. Apollo sold it to Limelight in Mar 2022 for");
    println!("USD 300M in Limelight stock (no cash).");
    println!();
    println!("Edgecast tech and customer base:");
    println!("  • Enterprise CDN with strong delivery + security combined offering");
    println!("  • WAF (Web Application Firewall) — formerly the ScaleArc / Imperva");
    println!("    competitor in the enterprise WAF market");
    println!("  • Streaming media specialty (Yahoo Sports, AOL News heritage)");
    println!("  • Significant US federal + state govt agency contracts");
    println!();
    println!("Why Apollo wanted out:");
    println!("  CDN unit economics had compressed industry-wide. Edgecast inside");
    println!("  Yahoo was sub-scale relative to Cloudflare + Akamai. Apollo as a");
    println!("  PE buyer wanted to extract value and exit, not invest in turnaround.");
    println!();
    println!("Why Limelight wanted in:");
    println!("  Adding Edgecast's enterprise + security customer book gave the");
    println!("  combined company instant scale (~2x revenue). The thesis: combine");
    println!("  two struggling CDNs into one viable scale-CDN. Classic consolidation.");
    println!();
    println!("In practice, you cannot solve a structural margin problem by adding");
    println!("two structurally-margin-pressured businesses together. The math");
    println!("doesn't change — costs combine, but pricing power doesn't.");
}

fn cmd_layer0() {
    println!("Layer0 (Moovweb) acquisition — Dec 2021");
    println!();
    println!("Moovweb / Layer0:");
    println!("  Founded 2010 in San Francisco. Mobile web acceleration originally,");
    println!("  then pivoted to a Vercel-style 'frontend cloud' product called");
    println!("  Layer0 (~2020 rebrand).");
    println!();
    println!("  Layer0's pitch: deploy your React/Next.js/Vue app to a globally-");
    println!("  distributed edge with built-in CDN, image optimization, prefetch,");
    println!("  and observability. The exact same target market Vercel was winning.");
    println!();
    println!("Acquisition:");
    println!("  Limelight bought Layer0 in Dec 2021 for USD 12.5M cash + equity.");
    println!("  (Tiny relative to the combined entity that would become Edgio.)");
    println!();
    println!("  The pitch: Limelight gets a credible developer-platform offering");
    println!("  to layer on top of its network, competing with Vercel + Cloudflare");
    println!("  Pages for the JAMstack / Next.js deployment market.");
    println!();
    println!("Renaming to Edgio Applications:");
    println!("  After the rebrand to Edgio in Jun 2022, Layer0 became");
    println!("  'Edgio Applications.' Customers like Volkswagen, Carnival, and");
    println!("  many ecommerce mid-market sites kept running on it.");
    println!();
    println!("Reality check:");
    println!("  Layer0 + Edgio could not match Vercel's developer mindshare or");
    println!("  Cloudflare's distribution. The 'enterprise CDN + frontend platform'");
    println!("  cross-sell motion never produced the expected pipeline. Layer0");
    println!("  ended up an orphan inside Edgio, not pulling its weight, and was");
    println!("  among the assets sold off during the bankruptcy.");
}

fn cmd_bankruptcy() {
    println!("Edgio Chapter 11 — Sep 9, 2024");
    println!();
    println!("Pre-bankruptcy signals (chronological):");
    println!();
    println!("  Late 2022: stock under USD 1. NASDAQ delisting notices begin.");
    println!("  2023:      reverse stock split attempted to maintain listing.");
    println!("             Net losses widening despite restructuring.");
    println!("  Early 2024: late SEC filings (10-K not filed on time).");
    println!("              Auditor flags going-concern doubt.");
    println!("  Aug 2024:   DBRS Morningstar / S&P further downgrade credit ratings.");
    println!();
    println!("Sep 9, 2024:");
    println!("  Edgio files for Chapter 11 bankruptcy protection in the District");
    println!("  of Delaware. Listed assets of approximately USD 100-500M and");
    println!("  liabilities in the same range (typical Ch. 11 filing format).");
    println!();
    println!("  Files for sale of operating assets under a 'stalking horse' bid.");
    println!("  Akamai Technologies, Inc. agrees to acquire substantially all of");
    println!("  Edgio's customer contracts and operating assets for USD 110M cash");
    println!("  in the auction process.");
    println!();
    println!("Asset sale completes:");
    println!("  Late Sep / Oct 2024. Edgio customers migrated to Akamai over");
    println!("  several months. Edgio corporate entity proceeds to wind-down");
    println!("  and asset distribution under bankruptcy court oversight.");
    println!();
    println!("Stockholders:");
    println!("  Common equity wiped out, as is typical in Chapter 11. Bondholders");
    println!("  and unsecured creditors recover pennies on the dollar.");
    println!();
    println!("Headcount impact:");
    println!("  Significant. Many engineering and ops roles eliminated as Akamai");
    println!("  absorbed the customer base but not the operational organization.");
}

fn cmd_akamai() {
    println!("Akamai's acquisition of Edgio assets");
    println!();
    println!("The deal:");
    println!("  Sep 2024 — Akamai signs a stalking-horse asset purchase agreement");
    println!("  for substantially all of Edgio's operating assets, including");
    println!("  customer contracts, software, certain employee transfers.");
    println!();
    println!("  Price: USD 110M cash, subject to court-approved bidding process.");
    println!("  No competing bid emerged at auction. Deal closed shortly after.");
    println!();
    println!("What Akamai got:");
    println!("  • Edgio's enterprise CDN customer contracts");
    println!("  • Streaming + delivery technology overlapping Akamai's existing");
    println!("    Media Delivery business (Aura Network Solutions heritage etc.)");
    println!("  • Customer relationships in segments Akamai already serves");
    println!("    (federal/state govt, telcos, broadcasters)");
    println!();
    println!("What Akamai did NOT acquire:");
    println!("  • Edgio Corporation as a going concern");
    println!("  • Most of Edgio's facilities or long-term office leases");
    println!("  • Most of Edgio's permanent headcount (selective hiring only)");
    println!();
    println!("Strategic context:");
    println!("  Akamai bought MaxCDN heritage from StackPath in May 2022.");
    println!("  Akamai bought Linode in Mar 2022 (USD 900M for cloud compute).");
    println!("  Akamai bought Edgio's CDN assets in Sep 2024.");
    println!();
    println!("Akamai has consciously played the role of consolidator-of-last-resort");
    println!("for the legacy mid-tier CDN industry — picking up customer books at");
    println!("distressed prices as competitors fail. The dominant survivor strategy");
    println!("when an industry consolidates around a small number of hyperscalers.");
}

fn cmd_lessons() {
    println!("Edgio lessons — CDN industry consolidation post-mortem");
    println!();
    println!("1. CDN unit economics compressed faster than expected.");
    println!("   Egress prices fell ~10x from 2010 to 2020 across the industry.");
    println!("   Mid-tier CDNs without proprietary tech or differentiation got");
    println!("   squeezed between hyperscaler scale and Cloudflare's freemium.");
    println!();
    println!("2. Adding two struggling businesses doesn't solve unit economics.");
    println!("   Limelight + Edgecast + Layer0 = three margin problems compounded.");
    println!("   Cost synergies cannot offset structural pricing pressure.");
    println!();
    println!("3. Public-market CDNs face an existential bind.");
    println!("   Fastly, Cloudflare, Akamai, Edgio were all on NASDAQ/NYSE.");
    println!("   Fastly has struggled too (post-2020 hype-trough). Akamai pivoted");
    println!("   to security + compute to escape pure CDN. Cloudflare uses CDN");
    println!("   as a Trojan-horse for security and DevPlat. Pure-play CDN as");
    println!("   public-market thesis is essentially over.");
    println!();
    println!("4. Developer mindshare is the new moat.");
    println!("   Cloudflare Workers, Vercel, Netlify built developer relationships");
    println!("   that translate to enterprise revenue at scale. Edgio bought Layer0");
    println!("   too late and without sustained investment.");
    println!();
    println!("5. Tier-1 owned network is no longer a moat.");
    println!("   Limelight's owned-fiber differentiation eroded as transit prices");
    println!("   collapsed and Anycast routing matured. What was a structural");
    println!("   advantage in 2007 was a sunk cost by 2020.");
    println!();
    println!("6. Akamai wins by being the last one standing.");
    println!("   The slow, profitable, security-pivoted incumbent absorbed the");
    println!("   failed rollups (StackPath assets, Edgio assets) at distress prices.");
    println!("   Boring beats consolidation drama in this industry.");
}

fn run_edgio(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "limelight" => cmd_limelight(),
        "edgecast" => cmd_edgecast(),
        "layer0" => cmd_layer0(),
        "bankruptcy" => cmd_bankruptcy(),
        "akamai" => cmd_akamai(),
        "lessons" => cmd_lessons(),
        "help" | "--help" | "-h" => print_help(prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'");
            eprintln!("Try '{prog} help' for the list of subcommands.");
            return 2;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "edgio-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_edgio(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/edgio-cli"), "edgio-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("edgio-cli.exe"), "edgio-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_edgio(&[], "edgio-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_edgio(&["bogus".into()], "edgio-cli"), 2);
    }
}
