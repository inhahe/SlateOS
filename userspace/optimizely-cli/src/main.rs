#![deny(clippy::all)]
//! optimizely-cli — personality CLI for Optimizely, the A/B testing pioneer
//! turned digital-experience-platform (DXP).
//!
//! Founded 2010 in San Francisco by Dan Siroker and Pete Koomen, both ex-Google.
//! Pioneered visual A/B testing on the web (Optimizely Classic), grew to a
//! reported $100M+ ARR before pivoting. Acquired Episerver in 2020 and folded
//! into a unified "Optimizely" brand combining CMS, commerce, marketing
//! automation, and experimentation. Privately owned by Insight Partners.
//! Migrated from "Optimizely Classic" to the developer-API "Full Stack" /
//! "Feature Experimentation" product.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Optimizely DXP personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Siroker + Koomen, Episerver merger, Insight");
    println!("    classic       The original visual A/B testing era");
    println!("    fullstack     Feature Experimentation API product");
    println!("    web           Web Experimentation (modern visual tool)");
    println!("    cms           Content Cloud (ex-Episerver)");
    println!("    commerce      Configured Commerce (ex-InsiteCommerce)");
    println!("    one           One Optimizely Suite — the DXP pitch");
    println!("    stats         Stats Engine — sequential testing");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("optimizely-cli 0.1.0 (DXP-era personality build)"); }

fn run_about() {
    println!("Optimizely, Inc.");
    println!("  Founded:      2010, San Francisco");
    println!("  Founders:     Dan Siroker (ex-Google PM, Obama '08 analytics),");
    println!("                Pete Koomen (ex-Google).");
    println!("  Key pivot:    2020 acquired Episerver (Swedish CMS giant);");
    println!("                Episerver itself rebranded as Optimizely.");
    println!("  Owner:        Insight Partners (private equity).");
    println!("  Positioning:  Digital Experience Platform (DXP) competing");
    println!("                against Adobe Experience Cloud and Sitecore.");
}

fn run_classic() {
    println!("Optimizely Classic (sunset 2019).");
    println!("  Snippet-based JavaScript that paints test variations in-browser.");
    println!("  Visual editor: WYSIWYG change-the-headline-color, no code.");
    println!("  Hugely popular with marketing teams, painful for product engineers.");
    println!("  Sunset in favor of the API-first Full Stack / Web Experimentation.");
}

fn run_fullstack() {
    println!("Full Stack / Feature Experimentation.");
    println!("  Server-side and mobile SDKs for engineers.");
    println!("  Feature flags + experiments via the same API.");
    println!("  Datafile model: SDK polls/streams compiled config JSON.");
    println!("  Events ingest via the Event API for metric computation.");
    println!("  Stats Engine handles peeking, sequential analysis.");
}

fn run_web() {
    println!("Web Experimentation (modern).");
    println!("  Successor to Classic for marketing teams.");
    println!("  Visual editor on top of the Full Stack SDK.");
    println!("  Snippet runs synchronously to avoid flicker.");
    println!("  Audience targeting, multi-page funnels, personalisation.");
}

fn run_cms() {
    println!("Content Cloud (ex-Episerver CMS).");
    println!(".NET-based content platform popular with enterprise marketers");
    println!("in Europe + Asia. SaaS-hosted on Azure. Content composition,");
    println!("personalisation, AI-driven content recommendations. Integrates");
    println!("natively with Web Experimentation for content-targeted tests.");
}

fn run_commerce() {
    println!("Configured Commerce (ex-InsiteCommerce, acquired by Episerver 2020).");
    println!("B2B-focused commerce platform with quote workflows, contract");
    println!("pricing, account hierarchies. Integrated with the CMS and");
    println!("Experimentation for unified buyer experience.");
}

fn run_one() {
    println!("One Optimizely Suite — the DXP pitch.");
    println!("  Content Cloud (CMS) + Commerce + Web/Feature Experimentation");
    println!("  + Content Marketing Platform (CMP) + Data Platform (CDP).");
    println!("Sold as a stack so customers don't shop Adobe/Sitecore.");
}

fn run_stats() {
    println!("Stats Engine — Optimizely's secret sauce.");
    println!("  Sequential testing: results valid at any time, no peeking penalty.");
    println!("  False discovery rate (FDR) control across many metrics.");
    println!("  Published methodology (R. Johari et al., 2017 KDD paper).");
    println!("  Allowed marketing teams to make decisions before pre-declared");
    println!("  sample sizes — a usability win over classical fixed-horizon tests.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  IBM, BBC, Microsoft, Sony, Visa, Toyota");
    println!("  Carhartt, Helly Hansen, Foot Locker (Configured Commerce)");
    println!("  Vodafone, Telia, T-Mobile (Content Cloud)");
    println!("  StubHub, Atlassian, Salesforce (historical Classic users)");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "optimizely-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "classic" => run_classic(),
        "fullstack" => run_fullstack(),
        "web" => run_web(),
        "cms" => run_cms(),
        "commerce" => run_commerce(),
        "one" => run_one(),
        "stats" => run_stats(),
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
        run_classic();
        run_fullstack();
        run_web();
        run_cms();
        run_commerce();
        run_one();
        run_stats();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("optimizely-cli");
        print_version();
    }
}
