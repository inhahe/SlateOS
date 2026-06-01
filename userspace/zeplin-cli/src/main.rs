#![deny(clippy::all)]
//! zeplin-cli — personality CLI for Zeplin, the design-to-developer
//! handoff + design-systems platform.
//!
//! Founded 2014 in San Francisco / Istanbul by Pelin Kenez (CEO),
//! Berk Çebi (CTO) and the Zeplin co-founder cohort. Zeplin filled the
//! gap between design tools (Sketch, then Figma, then Adobe XD) and
//! engineering: a destination where developers could open a screen,
//! see exact pixel values + colours + typography + CSS + iOS + Android
//! snippets without needing access to the source design file. Picked
//! up Series B from eVentures + ICONIQ in 2018. As design tools added
//! native handoff tabs (Figma's Inspect, Sketch Inspector), Zeplin
//! repositioned upmarket: design-system management, organisation
//! libraries, and Connected Components — the bridge that ties a Figma
//! component to its real Storybook + React + iOS implementation.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Zeplin design + developer handoff + design-systems personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Kenez + Cebi 2014 SF/Istanbul; Series B 2018");
    println!("    handoff       Inspect mode: CSS + Swift + Kotlin + asset export");
    println!("    components    Connected Components: Figma + Storybook + React + iOS link");
    println!("    styleguide    Project + organisation style guides + design tokens");
    println!("    integrations  Figma + Sketch + Adobe XD + Jira + Slack + Trello");
    println!("    pivot         The post-Figma-handoff-tab upmarket pivot to design systems");
    println!("    pricing       Free + Starter + Growth + Business + Enterprise tiers");
    println!("    customers     Apple + Pinterest + Slack + design-systems heavy orgs");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("zeplin-cli 0.1.0 (design-handoff-and-systems personality build)"); }

fn run_about() {
    println!("Zeplin, Inc.");
    println!("  Founded:    2014, San Francisco + Istanbul, Turkey.");
    println!("  Founders:   Pelin Kenez (CEO) + Berk Cebi (CTO) + co-founders.");
    println!("  Backers:    eVentures, ICONIQ Capital, Silicon Valley Bank, Borusan Holding,");
    println!("              prominent Bay Area + Turkish operator angels.");
    println!("  Funding:    \\$13M Series B 2018 led by eVentures + ICONIQ; ~\\$28M total raised.");
    println!("  Team:       distributed across SF + Istanbul + Western Europe + LATAM.");
    println!("  Position:   design-to-developer handoff (original); design-systems platform");
    println!("              + Connected Components (current upmarket pivot).");
    println!("  Heritage:   the original company that turned designer + developer handoff");
    println!("              into a discrete SaaS category.");
}

fn run_handoff() {
    println!("Handoff (the original product).");
    println!("  Designer exports screens from Sketch / Figma / Adobe XD / Photoshop to Zeplin.");
    println!("  Developer opens Zeplin + clicks any element to see exact CSS / Sass values,");
    println!("  iOS Swift snippets, Android Kotlin / XML snippets, Flutter Dart snippets.");
    println!("  Asset export: PNG / SVG / JPEG / PDF at 1x / 2x / 3x / @1x / @2x / @3x.");
    println!("  Per-element annotations + comments + version pinning.");
    println!("  This was the original wedge: developers no longer needed Sketch licences +");
    println!("  Photoshop access just to read pixel values off a design file.");
}

fn run_components() {
    println!("Connected Components (the strategic bet).");
    println!("  A Figma / Sketch component in Zeplin links to its real engineering counterpart:");
    println!("  the React + Vue + Angular Storybook story, the iOS SwiftUI / UIKit class,");
    println!("  the Android Jetpack Compose composable, the documentation in Notion.");
    println!("  Developers see a design in Zeplin and immediately follow a link to the");
    println!("  Storybook story showing how that component is already built.");
    println!("  Designers see which components have engineering coverage + which don't.");
    println!("  This is the Zeplin moat now that Figma owns native inspect-mode handoff.");
}

fn run_styleguide() {
    println!("Style guides + design tokens.");
    println!("  Per-project style guides: colours, type styles, spacing tokens, components.");
    println!("  Organisation-wide style guides: shared design system across projects + teams.");
    println!("  Design tokens export: JSON / CSS / Sass / Less / iOS / Android / SwiftUI.");
    println!("  Version history + diffs on token changes across releases.");
    println!("  Bridge into Style Dictionary + other token-pipeline tooling.");
    println!("  Frequently the canonical source of truth for the org design system at scale.");
}

fn run_integrations() {
    println!("Integrations.");
    println!("  Design tools: Figma, Sketch, Adobe XD, Photoshop — first-class plug-ins for");
    println!("  publishing screens + libraries from the source design file into Zeplin.");
    println!("  Project management: Jira, Trello, Asana — link cards to screens + comments.");
    println!("  Collaboration: Slack, Microsoft Teams — notifications + share-link previews.");
    println!("  Storybook: bidirectional Connected Components link to live component code.");
    println!("  CLI: zeplin CLI for CI publication of component metadata + token snapshots.");
    println!("  API + Visual Studio Code extension for in-IDE handoff lookups.");
}

fn run_pivot() {
    println!("The strategic pivot.");
    println!("  2014-2018: Zeplin owns design + developer handoff as a discrete SaaS category.");
    println!("  2019:      Figma launches Inspect mode — handoff goes from category to feature.");
    println!("  2020:      Sketch + Adobe XD ship similar handoff tabs natively in-tool.");
    println!("  2021:      Zeplin repositions upmarket — design systems, organisation libraries,");
    println!("             Connected Components, source-of-truth role for engineering parity.");
    println!("  2022-2024: design-systems story + Storybook integration becomes the pitch.");
    println!("  Result: Zeplin survived the handoff-as-feature shift by moving up the stack");
    println!("  to design-systems management — where Figma is still less opinionated.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free:       up to 1 project for solo designers + dev teams.");
    println!("  Starter:    ~\\$6/user/month, multiple projects + basic handoff.");
    println!("  Growth:     ~\\$12/user/month, design-system features + Connected Components.");
    println!("  Business:   ~\\$26/user/month, SSO + advanced governance + token APIs.");
    println!("  Enterprise: custom — SAML SSO, audit, dedicated success, SLA, residency.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: design + engineering orgs 50-50,000+ employees with serious");
    println!("  design-systems investment + parity between design + engineering libraries.");
    println!("  Named customers: Apple, Pinterest, Slack, Microsoft (selective), Salesforce,");
    println!("  Twitch, Squarespace, Etsy, Mailchimp, Audi, large banks + telecoms.");
    println!("  Geographic: heavy US + EU + Turkey + LATAM; growing APAC.");
    println!("  Common origin: 'we already use Figma + we need a way to keep components");
    println!("  in design + components in code from drifting apart over time'.");
    println!("  Anti-segment: small startups (Figma alone is enough).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "zeplin-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "handoff" => run_handoff(),
        "components" => run_components(),
        "styleguide" => run_styleguide(),
        "integrations" => run_integrations(),
        "pivot" => run_pivot(),
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
        run_handoff();
        run_components();
        run_styleguide();
        run_integrations();
        run_pivot();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("zeplin-cli");
        print_version();
    }
}
