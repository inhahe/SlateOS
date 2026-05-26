#![deny(clippy::all)]
//! whimsical-cli — personality CLI for Whimsical, the Toronto-founded
//! visual collaboration platform for flowcharts, wireframes, mind maps,
//! and sticky-note boards.
//!
//! Founded 2017 in Toronto + Riga by Steve Schoeffel (CEO, ex-Wave) and
//! Kaspars Dancis (CTO, ex-Wave). Both founders previously built engineering
//! at Wave Financial in Toronto. Whimsical positioned itself between Miro
//! (free-form whiteboard) and Lucidchart (rigorous diagramming) — opinionated,
//! pretty, fast, with built-in templates for the specific workflows product
//! + design + engineering teams use most often. Bootstrapped + profitable;
//! no significant outside funding, no acquisition pressure, no SaaS race-to-
//! the-bottom pricing. The product is a defining example of the indie-bootstrap-
//! profitable SaaS strategy: deep in a narrow surface, charge for it, never
//! raise a round.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Whimsical visual-collaboration personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Schoeffel + Dancis 2017 Toronto + Riga; bootstrapped");
    println!("    flowcharts    Flowchart editor with auto-routing connectors");
    println!("    wireframes    Wireframe + lo-fi UI mockups for product spec docs");
    println!("    mindmaps      Mind-map editor for brainstorming + outlining");
    println!("    boards        Sticky-note + free-form board for workshops");
    println!("    docs          Whimsical Docs — embed visuals into a doc page");
    println!("    pricing       Free + Pro + Enterprise tiers");
    println!("    customers     Product + design + engineering teams 5-500 employees");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("whimsical-cli 0.1.0 (visual-collab-bootstrapped personality build)"); }

fn run_about() {
    println!("Whimsical, Inc.");
    println!("  Founded:    2017, Toronto, Canada + Riga, Latvia.");
    println!("  Founders:   Steve Schoeffel (CEO; ex-Wave) + Kaspars Dancis (CTO; ex-Wave).");
    println!("              Both previously built at Wave Financial (Toronto small-business");
    println!("              accounting startup, acquired by H&R Block in 2019).");
    println!("  Backers:    none significant — bootstrapped + profitable from early on.");
    println!("  Funding:    no announced rounds; revenue-funded growth.");
    println!("  Position:   opinionated visual-collaboration product sitting between Miro");
    println!("              (free-form whiteboard) + Lucidchart (rigorous diagramming).");
    println!("  Strategy:   bootstrap + profitable + opinionated + small team forever.");
    println!("              Canonical indie SaaS playbook in the visual-tools category.");
}

fn run_flowcharts() {
    println!("Flowcharts.");
    println!("  Snap-to-grid shapes: rectangles, diamonds, parallelograms, terminators.");
    println!("  Auto-routing connectors: lines avoid shapes + redraw cleanly when nodes move.");
    println!("  Quick-add: press tab to spawn a connected child shape — keyboard-first editing.");
    println!("  Markdown labels inside shapes; rich-text + emoji + icon library.");
    println!("  Lane + group containers for swimlanes + bounded subsystems.");
    println!("  Output is consistently pretty without manual layout tweaking —");
    println!("  the deliberate Whimsical aesthetic.");
}

fn run_wireframes() {
    println!("Wireframes.");
    println!("  Library of low-fidelity UI primitives: buttons, inputs, navigation bars,");
    println!("  toggles, modal dialogs, sidebars, tabs, cards, dropdowns, image placeholders.");
    println!("  Wireframes intentionally stylised: hand-drawn-feeling stroke + grey-tone");
    println!("  palette + sans-serif fonts to signal 'not the final visual design'.");
    println!("  Designed for PMs + engineering leads writing spec docs, not production design.");
    println!("  Common workflow: paste a wireframe inline into a Notion / Linear / Confluence");
    println!("  spec doc to communicate intent without overcommitting to visual design.");
}

fn run_mindmaps() {
    println!("Mind maps.");
    println!("  Tree-style branching with one root node + N child branches.");
    println!("  Tab + enter + arrow-key navigation: keyboard-driven mind-mapping is the focus.");
    println!("  Outline view: flip the mind map into an indented text outline + back again.");
    println!("  Common uses: brainstorming session output, blog-post outlining, OKR drafts,");
    println!("  feature breakdowns, user-journey mapping, retrospective grouped feedback.");
    println!("  Subtree collapse + drag-reparent for fast reorganisation during a session.");
}

fn run_boards() {
    println!("Sticky-note + free-form boards.");
    println!("  Infinite canvas with sticky notes, shapes, connectors, text, images.");
    println!("  Lighter-weight than a Miro board: opinionated layout + smaller template library.");
    println!("  Common shapes: retrospectives, planning poker, dot voting, lean canvases,");
    println!("  user-story mapping, customer-journey workshops.");
    println!("  Realtime multi-user editing with presence cursors + comment threads.");
    println!("  Sits as the 'workshop facilitation' surface alongside flowcharts + wireframes.");
}

fn run_docs() {
    println!("Whimsical Docs.");
    println!("  In-app document editor: paragraphs + headings + lists + code blocks +");
    println!("  embedded Whimsical flowcharts + wireframes + mind maps + boards inline.");
    println!("  The pitch: write a product spec where every diagram is a real editable");
    println!("  Whimsical object, not a screenshot pasted into Confluence.");
    println!("  Comments + mentions + emoji reactions for async review.");
    println!("  Positioned against Notion / Confluence for product + engineering spec writing,");
    println!("  with the visual editing being the differentiator.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free:        small project quota + Whimsical branding on share links.");
    println!("  Pro:         ~\\$10-12/user/month, unlimited boards + version history.");
    println!("  Enterprise:  custom — SSO + audit + dedicated success.");
    println!("  Pricing reflects the bootstrap strategy: middle-of-the-market pricing,");
    println!("  no race-to-the-bottom free tier, no aggressive enterprise discount.");
    println!("  Revenue per customer is healthy enough to never need a venture round.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: product + design + engineering teams 5-500 employees writing");
    println!("  light-to-medium technical + product documentation with diagrams.");
    println!("  Industries: SaaS startups + scale-ups, design + product agencies, in-house");
    println!("  product teams at non-tech companies, individual indie product makers.");
    println!("  Geographic: heavy US + Canada + EU; smaller APAC + LATAM presence.");
    println!("  Common origin: 'I'm a PM who hates fighting Lucidchart + I want flowcharts");
    println!("  that look nice without me being a designer'.");
    println!("  Anti-segment: large-enterprise diagramming (Lucid, Visio) + workshop");
    println!("  facilitation at scale (Miro, Mural).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "whimsical-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "flowcharts" => run_flowcharts(),
        "wireframes" => run_wireframes(),
        "mindmaps" => run_mindmaps(),
        "boards" => run_boards(),
        "docs" => run_docs(),
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
        run_flowcharts();
        run_wireframes();
        run_mindmaps();
        run_boards();
        run_docs();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("whimsical-cli");
        print_version();
    }
}
