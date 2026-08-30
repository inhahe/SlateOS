#![deny(clippy::all)]
//! devcycle-cli — personality CLI for DevCycle, the feature management
//! platform that grew out of the Taplytics mobile experimentation company.
//!
//! Taplytics was founded 2014 in Toronto by Aaron Glazer and Andrew Norris,
//! originally a mobile A/B testing and analytics platform popular with
//! consumer apps. In 2022 the company rebranded its focus to feature
//! management, launching DevCycle as a sister product and progressively
//! consolidating the brand. Differentiator: a WebAssembly bucketing engine
//! shared across SDKs and edge runtimes (Cloudflare Workers, Vercel Edge,
//! Fastly Compute, Akamai EdgeWorkers) for sub-millisecond evaluation.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — DevCycle feature management personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Taplytics origins, rebrand to DevCycle");
    println!("    features      Variables, variations, features model");
    println!("    bucketing     Shared WebAssembly bucketing engine");
    println!("    edge          Edge runtime SDK matrix");
    println!("    openfeature   OpenFeature provider compliance");
    println!("    ai            AI Suggestions for targeting rules");
    println!("    pricing       Free tier and per-MAU bands");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("devcycle-cli 0.1.0 (Taplytics-lineage personality build)"); }

fn run_about() {
    println!("DevCycle (by Taplytics, Inc.)");
    println!("  Parent founded: 2014 in Toronto as Taplytics.");
    println!("  Founders:       Aaron Glazer (CEO), Andrew Norris.");
    println!("  Origin product: mobile A/B testing + analytics.");
    println!("  Pivot:          2022 launch of DevCycle as a developer-first");
    println!("                  feature management platform; Taplytics product");
    println!("                  rolled forward for legacy mobile customers.");
    println!("  HQ:             Toronto, Ontario.");
    println!("  License:        Closed source SaaS; SDKs MIT-licensed on GitHub.");
}

fn run_features() {
    println!("Domain model:");
    println!("  Feature       a logical capability flag.");
    println!("  Variations    named variants of a Feature (e.g. control/treatment).");
    println!("  Variables     typed values (bool/string/number/JSON) attached");
    println!("                to a Variation. SDKs read Variables, not Features,");
    println!("                so SDKs are decoupled from the Variation taxonomy.");
    println!("  Targeting     audience-rule list, ordered, first-match wins.");
}

fn run_bucketing() {
    println!("Bucketing engine — shared WebAssembly.");
    println!("  Single Rust core compiled to WASM.");
    println!("  Bundled into every SDK so evaluation logic is byte-identical");
    println!("  across server, browser, mobile, and edge SDKs.");
    println!("  Performance target: <1ms per variable evaluation on commodity HW.");
    println!("  Side effect: feature parity ships everywhere on every release.");
}

fn run_edge() {
    println!("Edge runtime support (first-class):");
    println!("  Cloudflare Workers");
    println!("  Vercel Edge Functions");
    println!("  Fastly Compute@Edge");
    println!("  Akamai EdgeWorkers");
    println!("  AWS Lambda@Edge");
    println!("  Deno Deploy");
    println!("Targeting decision made at the CDN edge so HTML/JSON responses");
    println!("are pre-flag-evaluated by the time they reach the user.");
}

fn run_openfeature() {
    println!("OpenFeature compliance.");
    println!("  DevCycle ships an OpenFeature provider implementation, so");
    println!("  customers using the OpenFeature SDK can swap underlying");
    println!("  vendors without code changes.");
    println!("  DevCycle co-maintains OpenFeature work alongside LaunchDarkly,");
    println!("  Split, Statsig, GrowthBook, and the CNCF community.");
}

fn run_ai() {
    println!("AI features (the 'modern feature management' pitch):");
    println!("  AI Suggestions     proposes targeting rules from natural-language");
    println!("                     descriptions ('roll out to users in EU on Pro').");
    println!("  AI Summaries       summarise impressions + rule activity per flag.");
    println!("  Code Insights      LLM-driven hint at where a flag is used and");
    println!("                     when it can be safely cleaned up.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free          1,000 MAU, unlimited features, unlimited seats.");
    println!("  Pro           per-MAU tiers, audit log, SSO basic.");
    println!("  Enterprise    custom, advanced SSO/SCIM, audit retention,");
    println!("                dedicated support, custom data residency.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  Mercedes-Benz.io   automotive digital products");
    println!("  Capital One        US bank");
    println!("  Shopify Plus       merchants on the enterprise tier");
    println!("  Coursera           ed-tech platform");
    println!("  Marriott           hotel digital products");
    println!("  Pearson VUE        certification platform");
    println!("  Hopper             travel app (legacy Taplytics)");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "devcycle-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "features" => run_features(),
        "bucketing" => run_bucketing(),
        "edge" => run_edge(),
        "openfeature" => run_openfeature(),
        "ai" => run_ai(),
        "pricing" => run_pricing(),
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
        run_features();
        run_bucketing();
        run_edge();
        run_openfeature();
        run_ai();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("devcycle-cli");
        print_version();
    }
}
