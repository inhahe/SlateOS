#![deny(clippy::all)]
//! invision-cli — personality CLI for InVision, the once-dominant
//! product-design + prototype platform that was eclipsed by Figma
//! and shut its core product down at the end of 2024.
//!
//! Founded 2011 in New York City by Clark Valberg (CEO) and Ben Nadel
//! (CTO). InVision built one of the first cloud-based design + prototyping
//! tools and rode the 2014-2018 design-tools boom: raised \$350M Series F
//! at a \$2B valuation in 2018, then a 2020 round at \$2B+ — the highest-
//! profile design-tools company in the world at the time. The shift was
//! brutal: Figma's collaborative browser-native editor displaced InVision
//! Studio + Cloud, and InVision never produced a competitive answer.
//! Mass layoffs in 2022 + 2023. InVision officially shut down the
//! InVision design + prototype platform on 31 December 2024 — the end of
//! a once-defining design-tools brand.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — InVision (the once-dominant design + prototype tool) personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Valberg + Nadel 2011 NYC; \\$2B+ peak 2018; sunset Dec 2024");
    println!("    prototype     InVision Cloud — the original clickable-prototype product");
    println!("    studio        InVision Studio — the screen-design app that never landed");
    println!("    freehand      Freehand whiteboard — survives as the divested product");
    println!("    designforward DesignForward Fund + Inside Design — content + community arm");
    println!("    decline       The InVision-to-Figma migration that ended the company");
    println!("    pricing       Final pricing tiers + sunset migration paths");
    println!("    customers     Fortune-500 design orgs that mostly migrated to Figma");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("invision-cli 0.1.0 (sunset-design-tool personality build)"); }

fn run_about() {
    println!("InVisionApp, Inc.");
    println!("  Founded:    2011, New York City.");
    println!("  Founders:   Clark Valberg (CEO) + Ben Nadel (CTO).");
    println!("  Peak HC:    ~800 employees at the 2018 peak.");
    println!("  Backers:    Accel, Tiger Global, ICONIQ, Spark Capital, Battery, FirstMark,");
    println!("              Goldman Sachs, BlackRock at later stages.");
    println!("  Funding:    \\$350M Series F 2018 at \\$1.9B; further \\$115M 2020 at \\$2B+.");
    println!("              Cumulative raise > \\$355M across the run.");
    println!("  Position (peak): the dominant cloud-based product-design + prototype tool.");
    println!("  Position (end):  sunset 31 Dec 2024 — design + prototype platform shut down.");
    println!("                   Freehand whiteboard spun out + acquired by Miro in 2024.");
}

fn run_prototype() {
    println!("InVision Cloud (the original product).");
    println!("  Upload static screens from Photoshop / Sketch + wire up clickable hotspots");
    println!("  + transitions to produce a navigable interactive prototype.");
    println!("  Built-in comment threads on every screen + per-pixel annotations.");
    println!("  Inspect view for developer handoff: CSS values, colour tokens, asset export.");
    println!("  Version history + share-link reviews + stakeholder approval flow.");
    println!("  This was the canonical 2014-2018 product-design review tool — almost every");
    println!("  serious in-house design team carried it as table-stakes infra.");
}

fn run_studio() {
    println!("InVision Studio (the bet that never landed).");
    println!("  Announced 2017 + shipped 2018 as a native screen-design editor + animation");
    println!("  tool intended as InVision's answer to Sketch + the (then rising) Figma.");
    println!("  Single-document desktop editor with a powerful timeline animation engine.");
    println!("  Adoption was modest; Figma's browser-native multiplayer-by-default model");
    println!("  ate the entire category within 18 months of Studio's launch.");
    println!("  Studio was effectively abandoned by 2021. The lesson became a case study in");
    println!("  design-tooling history: distribution + collaboration > single-user power.");
}

fn run_freehand() {
    println!("Freehand (the survivor).");
    println!("  Freehand was InVision's collaborative whiteboard product — sticky notes,");
    println!("  cursors, voting, templates — built well before Miro or FigJam mattered.");
    println!("  Throughout 2022-2024 Freehand became InVision's only growing product.");
    println!("  In 2024, with the rest of InVision being wound down, Freehand was spun out");
    println!("  and sold to Miro, where it merges into the Miro platform proper.");
    println!("  The brand effectively transferred ownership while the core company exited.");
}

fn run_designforward() {
    println!("DesignForward Fund + Inside Design (the community arm).");
    println!("  DesignForward: VC arm funding design-led startups + design-systems tools.");
    println!("  Inside Design: long-running design-thought-leadership content site +");
    println!("  podcast network; one of the most respected design-industry publications");
    println!("  during the 2015-2020 design-tooling boom. Largely wound down with the company.");
    println!("  Open-source: numerous tooling contributions including parts of the design-");
    println!("  systems-handbook ecosystem.");
}

fn run_decline() {
    println!("The InVision-to-Figma migration (the bigger story).");
    println!("  2016-2018: InVision dominant; Figma small but rising.");
    println!("  2019:      design teams begin moving editing into Figma; InVision Cloud");
    println!("             becomes review-only for many orgs.");
    println!("  2020-2021: Figma adds prototyping + commenting + inspect tabs natively.");
    println!("             InVision's last differentiated workflows collapse into Figma.");
    println!("  2022-2023: mass layoffs at InVision; product investment essentially halts.");
    println!("  2024:      InVision announces sunset; Freehand sold to Miro; design tools");
    println!("             generation that grew up on InVision moves wholesale to Figma.");
    println!("  31 Dec 2024: InVision platform officially shuts down.");
    println!("  A canonical case of 'the disruptor got disrupted' in SaaS history.");
}

fn run_pricing() {
    println!("Final pricing + sunset migration.");
    println!("  Free + Pro + Enterprise tiers in the platform's final years.");
    println!("  Pro:        ~\\$15-25/user/month at the final pricing.");
    println!("  Enterprise: custom; mostly Fortune 500 design-systems teams.");
    println!("  Sunset migration paths: export prototypes + assets; recommend Figma + Miro");
    println!("  as successors; InVision wrote migration guides + provided export tools.");
    println!("  No new signups accepted in the final months leading to the December 2024 shutdown.");
}

fn run_customers() {
    println!("Customer profile (peak era).");
    println!("  Sweet spot: in-house product-design teams at large enterprises + agencies.");
    println!("  Named users (peak): Airbnb, Netflix, IBM, Salesforce, Slack, NASA, HBO,");
    println!("  ESPN, Sony, JPMorgan, hundreds of Fortune 500 design teams worldwide.");
    println!("  Geographic: very heavy US + EU enterprise + design agencies.");
    println!("  Post-sunset: nearly all of the above migrated to Figma for editing +");
    println!("  prototyping + commenting; Miro for whiteboarding; Notion for design docs.");
    println!("  InVision served as the on-ramp for an entire generation of product designers.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "invision-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "prototype" => run_prototype(),
        "studio" => run_studio(),
        "freehand" => run_freehand(),
        "designforward" => run_designforward(),
        "decline" => run_decline(),
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
        run_prototype();
        run_studio();
        run_freehand();
        run_designforward();
        run_decline();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("invision-cli");
        print_version();
    }
}
