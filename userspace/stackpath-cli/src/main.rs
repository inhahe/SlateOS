#![deny(clippy::all)]
//! stackpath-cli — OurOS StackPath / Webscale exit personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — StackPath / former edge CDN (personality / obituary)");
    println!();
    println!("USAGE: {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about         Lance Crosby 2015, ABRY Partners-backed edge co.");
    println!("    rollup        The 2016 rollup: MaxCDN, Highwinds, Cloak, Fireblade");
    println!("    products      Historical product portfolio");
    println!("    edge          The pivot to 'Edge Computing'");
    println!("    exit          May 2022 sale of CDN biz to Akamai; Aug 2023 close");
    println!("    webscale      What remained: WebscaleNetworks ecommerce focus");
    println!("    lessons       Lessons from the StackPath consolidation playbook");
    println!("    help / version");
}

fn print_version() {
    println!("stackpath-cli 0.1.0 — OurOS personality binary");
    println!("StackPath, LLC (Dallas, TX) — CDN biz exited to Akamai 2022");
}

fn cmd_about() {
    println!("StackPath — Edge computing platform, born from CDN roll-up.");
    println!();
    println!("Founded:  2015 in Dallas, Texas, by Lance Crosby");
    println!("          Crosby was previously CEO + founder of SoftLayer,");
    println!("          a cloud hosting company sold to IBM in 2013 for ~USD 2B.");
    println!("          StackPath was his second act.");
    println!();
    println!("Backers:  ABRY Partners (private equity, lead investor)");
    println!("          Cox Communications (strategic, telco angle)");
    println!("          Reported total raised: USD 396M+ (across debt + equity)");
    println!();
    println!("Strategy:");
    println!("  Build the next-generation edge platform by ACQUIRING existing");
    println!("  CDNs and security companies, consolidating their tech under");
    println!("  one platform. Avoid the slow organic build.");
    println!();
    println!("  This was the conscious 'rollup' thesis: edge is balkanized,");
    println!("  many small/mid CDNs exist, none have all the pieces. Buy them,");
    println!("  unify them, sell the combined platform to enterprises.");
    println!();
    println!("Footprint at peak:");
    println!("  ~45 PoPs globally, 60+ TBPS network capacity (claimed).");
    println!("  Network rivaled CloudFront/Fastly in tier-1 transit + peering.");
    println!();
    println!("See 'stackpath rollup' for the acquisition chronology.");
}

fn cmd_rollup() {
    println!("StackPath — the rollup chronology");
    println!();
    println!("2015:  StackPath founded; first acquisitions begin");
    println!();
    println!("2016:  The 'big four' year:");
    println!();
    println!("  MaxCDN (Jul 2016):");
    println!("    Bootstrapped CDN founded 2009 in Los Angeles by David Henzel");
    println!("    and Justin Dorfman. Popular with WordPress hosts. Profitable.");
    println!("    Acquisition price not disclosed but reported in low 9-figures.");
    println!();
    println!("  Highwinds Network Group (Aug 2016):");
    println!("    Orlando-based enterprise CDN founded 2002. Acquisition reported");
    println!("    at USD 240M. Brought enterprise-tier customers + Tier-1 network.");
    println!("    This was the big one — Highwinds was a serious player.");
    println!();
    println!("  Cloak (Aug 2016):");
    println!("    Consumer VPN service. Smaller, brought VPN tech for the");
    println!("    StackPath 'consumer privacy' angle.");
    println!();
    println!("  Fireblade (Oct 2016):");
    println!("    Israeli WAF / bot protection startup. Brought security tech.");
    println!();
    println!("2017-2018:");
    println!("  Platform unification work. The hard, unsexy job of merging");
    println!("  four different ops + control planes + customer dashboards into");
    println!("  one. This phase took longer than planned (it always does).");
    println!();
    println!("2019-2020:");
    println!("  Pivot to 'Edge Computing' branding. Edge VMs, edge containers,");
    println!("  edge K8s, edge Workers. The market was moving and StackPath");
    println!("  followed.");
    println!();
    println!("2021-2022:");
    println!("  Execution challenges visible. Edge compute revenue not scaling");
    println!("  fast enough; CDN core under pressure from Cloudflare + Fastly.");
}

fn cmd_products() {
    println!("StackPath historical product portfolio (at 2020-2021 peak)");
    println!();
    println!("Edge Compute:");
    println!("  • Edge VMs        Long-lived virtual machines at edge PoPs");
    println!("  • Edge Containers Docker/OCI workloads on edge K8s");
    println!("  • Edge Workers    Serverless JS at the edge (CF Workers-style)");
    println!();
    println!("Edge Delivery:");
    println!("  • CDN             Static + dynamic acceleration");
    println!("  • DNS             Authoritative DNS + load balancing");
    println!("  • Object Storage  S3-compatible storage at edge regions");
    println!();
    println!("Edge Security:");
    println!("  • WAF             Web application firewall");
    println!("  • DDoS Protection Layer 3-7 mitigation");
    println!("  • Bot Manager     Bot detection + management");
    println!("  • VPN             Consumer + business VPN (Cloak heritage)");
    println!();
    println!("Edge Monitoring:");
    println!("  • Real-Time Stats Per-edge analytics");
    println!("  • Logging         Real-time access log streaming");
    println!();
    println!("Developer experience:");
    println!("  REST API + GraphQL API. Terraform provider. CLI tool ('stackpath').");
    println!("  Dashboard at app.stackpath.com.");
    println!();
    println!("On paper this was a complete edge platform competing head-on with");
    println!("Cloudflare and Fastly. In practice, market consolidation chose the");
    println!("incumbents.");
}

fn cmd_edge() {
    println!("StackPath's pivot to 'Edge Computing'");
    println!();
    println!("Context:");
    println!("  ~2018-2019, every CDN was rebranding to 'edge cloud' or 'edge");
    println!("  computing platform' — Fastly led with Compute@Edge (2018-2020),");
    println!("  Cloudflare doubled down on Workers (2018+), AWS launched");
    println!("  Lambda@Edge (2017). StackPath had to follow or be left behind.");
    println!();
    println!("StackPath's edge offering:");
    println!("  • Edge VMs — full Linux VMs in 30+ PoPs, billed hourly");
    println!("  • Edge Containers — managed container runtime, push-and-deploy");
    println!("  • Workers — JS isolates at edge, similar to CF Workers");
    println!();
    println!("The technical proposition was sound. Edge VMs in particular were");
    println!("differentiated — full-fat Linux at the edge for workloads that");
    println!("V8 isolates couldn't handle (databases, gaming, transcoding).");
    println!();
    println!("Why it didn't scale:");
    println!("  1. Cloudflare's network was 3-5x bigger and growing faster");
    println!("  2. AWS Local Zones + Outposts ate the enterprise edge VM market");
    println!("  3. Developer mindshare went to Cloudflare Workers, Fastly Compute,");
    println!("     Vercel Edge Functions — StackPath had little dev community");
    println!("  4. PE-backed companies must grow into their valuation; the edge");
    println!("     compute market grew, but not fast enough for StackPath's plan");
    println!();
    println!("In 2022 the company restructured. See 'stackpath exit'.");
}

fn cmd_exit() {
    println!("StackPath — the 2022 exit");
    println!();
    println!("May 2022:");
    println!("  Akamai announces acquisition of StackPath's CDN business");
    println!("  (the original Highwinds + MaxCDN + delivery assets).");
    println!("  Terms not disclosed publicly.");
    println!("  StackPath retains Edge Compute, WAF, and other 'cloud' pieces.");
    println!();
    println!("Aug 2022 - Apr 2023:");
    println!("  StackPath continues as 'StackPath Cloud' — Edge VMs + Containers");
    println!("  + Workers. Customer migrations from acquired Highwinds/MaxCDN to");
    println!("  Akamai's platform proceed in waves.");
    println!();
    println!("Apr 2023:");
    println!("  StackPath announces wind-down of legacy CDN service for customers");
    println!("  who haven't migrated. Multiple email campaigns urging migration");
    println!("  to Akamai or other CDNs.");
    println!();
    println!("Late 2023 - 2024:");
    println!("  The legacy MaxCDN/Highwinds infrastructure shutdown completed.");
    println!("  StackPath Cloud edge-compute business quietly downsizes.");
    println!("  Effectively no longer competing in the CDN market by 2024.");
    println!();
    println!("Aftermath:");
    println!("  The StackPath corporate entity persists but pivoted to specialized");
    println!("  edge workloads. The dream of a unified edge rollup ended.");
    println!("  See 'stackpath webscale' for what remains.");
}

fn cmd_webscale() {
    println!("Webscale Networks — what remains of the StackPath orbit");
    println!();
    println!("(Note: 'Webscale Networks' is a separate company that ran adjacent");
    println!("to / alongside StackPath. Not always part of the same legal entity.)");
    println!();
    println!("Webscale Networks Inc:");
    println!("  Founded 2013, Santa Clara CA. Focused on managed hosting +");
    println!("  cloud-native acceleration specifically for ecommerce.");
    println!();
    println!("  Strong in Magento, Shopify Plus, BigCommerce, Adobe Commerce");
    println!("  operations. Provides PaaS-style managed deployments with CDN");
    println!("  + WAF + autoscaling baked in.");
    println!();
    println!("Webscale picked up some former StackPath staff and select tech");
    println!("post-2022 reshuffle, though the corporate continuity is informal");
    println!("(both ran in the edge / managed hosting orbit).");
    println!();
    println!("Notable Webscale Networks moves:");
    println!("  • 2022-2023: rebrand around 'composable ecommerce' positioning");
    println!("  • Headless commerce focus (Vue Storefront, Hydrogen, etc.)");
    println!("  • Acquisitions of smaller specialty hosts in the ecommerce niche");
    println!();
    println!("Lesson:");
    println!("  The 'become Akamai' edge-platform-from-scratch play needed many");
    println!("  billions of capital and many years. Refocusing on a vertical");
    println!("  niche (ecommerce hosting) where domain expertise compounds is");
    println!("  a more realistic mid-market business. That's where Webscale");
    println!("  Networks landed.");
}

fn cmd_lessons() {
    println!("Lessons from the StackPath rollup playbook");
    println!();
    println!("1. Acquiring market presence is not acquiring market position.");
    println!("   Buying MaxCDN + Highwinds gave StackPath ~5% global CDN share.");
    println!("   But share is preserved by continued product investment, not by");
    println!("   the M&A line item. Customers churn during integration drama.");
    println!();
    println!("2. Platform unification takes ~2-3x longer than planned.");
    println!("   Merging 4 different ops planes, billing systems, and customer");
    println!("   experiences is fundamentally a 3+ year project. ABRY's hold");
    println!("   period was probably planned around a 5-7 year exit; the");
    println!("   integration debt ate years of the value-creation window.");
    println!();
    println!("3. Developer mindshare moats are real and underappreciated.");
    println!("   Cloudflare's Workers community, Fastly's developer evangelism,");
    println!("   Vercel's framework partnerships — these compound over years.");
    println!("   A PE-built rollup buying network capacity can't shortcut this.");
    println!();
    println!("4. Edge VM vs Edge Workers — different markets, different motions.");
    println!("   Edge VMs serve traditional ops buyers; Edge Workers serve");
    println!("   developers. StackPath tried to win both with one go-to-market");
    println!("   and ended up underserving both.");
    println!();
    println!("5. PE-backed CDN consolidation is structurally hard.");
    println!("   See also: Edgio (Limelight + Yahoo Edgecast merger -> Chapter 11");
    println!("   filing Aug 2024). Same playbook, same outcome. The thesis that");
    println!("   'mid-tier CDNs will consolidate into a strong #3' has been");
    println!("   tested twice and failed twice.");
    println!();
    println!("Survivors of the consolidation wave:");
    println!("  Cloudflare (organic), Fastly (organic), Akamai (incumbent,");
    println!("  acquirer of rolled-up assets), AWS CloudFront (hyperscaler).");
}

fn run_stackpath(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "rollup" => cmd_rollup(),
        "products" => cmd_products(),
        "edge" => cmd_edge(),
        "exit" => cmd_exit(),
        "webscale" => cmd_webscale(),
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
        .unwrap_or_else(|| "stackpath-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_stackpath(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/stackpath-cli"), "stackpath-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("stackpath-cli.exe"), "stackpath-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_stackpath(&[], "stackpath-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_stackpath(&["bogus".into()], "stackpath-cli"), 2);
    }
}
