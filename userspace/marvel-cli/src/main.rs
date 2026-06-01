#![deny(clippy::all)]
//! marvel-cli — personality CLI for Marvel App, the London-founded
//! design + prototype platform.
//!
//! Founded 2013 in London by Murat Mutlu (CEO, ex-Foolproof / ex-Smith
//! Magazine design) and Brendan Moore. Marvel started as a side-project
//! mobile-first prototyping tool with the strongest 'paste in Dropbox-
//! synced mockups + get a clickable prototype' onboarding of its era.
//! Picked up Series A from Connect Ventures + JamJar Investments in
//! 2017. Marvel acquired Sketch handoff specialist Sympli and grew the
//! handoff tab + user-testing module, then plateaued during the Figma
//! takeover. As of 2024, Marvel still operates as an independent
//! UK-based platform serving education + smaller design teams + agencies
//! looking for a lighter-weight alternative to Figma — a survivor story
//! in the otherwise brutal post-Figma design-tools shakeout.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Marvel London-based design + prototype personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Murat Mutlu + Brendan Moore 2013 London");
    println!("    prototype     Mobile-first clickable prototypes from static screens");
    println!("    canvas        Marvel canvas + light wireframe + design editor");
    println!("    handoff       Developer handoff + CSS + asset export + Sympli heritage");
    println!("    usertesting   Marvel User Testing — record real-user sessions on prototypes");
    println!("    education     Marvel for Education — the UK-strong segment");
    println!("    pricing       Free + Pro + Team + Enterprise tiers");
    println!("    customers     UK SMBs + education + agencies + Figma-averse teams");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("marvel-cli 0.1.0 (london-design-and-prototype personality build)"); }

fn run_about() {
    println!("Marvel Prototyping Ltd.");
    println!("  Founded:    2013, London, United Kingdom.");
    println!("  Founders:   Murat Mutlu (CEO; ex-Foolproof + ex-Smith Magazine design) +");
    println!("              Brendan Moore (co-founder).");
    println!("  Backers:    Connect Ventures, JamJar Investments, Hoxton Ventures, AngelPad,");
    println!("              prominent UK + EU design + product angels.");
    println!("  Funding:    ~\\$8M Series A 2017; smaller follow-on rounds since.");
    println!("  Position:   lightweight design + prototype platform with strong UK + EU SMB");
    println!("              + education presence; the InVision-shaped tool that survived.");
    println!("  Notable:    acquired Sympli (Sketch + design-handoff tool) in 2018.");
}

fn run_prototype() {
    println!("Prototyping (the original product).");
    println!("  Connect a Dropbox folder + Marvel picks up new mockups + sync screens.");
    println!("  Drag-drop hotspots to wire transitions between screens.");
    println!("  Transition types: dissolve, push, slide, modal, custom timing curves.");
    println!("  Share-link prototypes with stakeholder commenting + version history.");
    println!("  Native preview as iOS + Android app shell on real devices.");
    println!("  Designed for the 2013-2018 'static Photoshop / Sketch screens' workflow,");
    println!("  still useful for designers + agencies on that toolchain.");
}

fn run_canvas() {
    println!("Marvel canvas (in-app editor).");
    println!("  Light wireframe + visual design editor inside Marvel itself —");
    println!("  designers can build prototypes without an external tool.");
    println!("  Vector + image + text + basic component primitives.");
    println!("  Pre-built device frames + UI kits for iOS, Android, web, smartwatch.");
    println!("  Less ambitious than Figma + InVision Studio — deliberately simple to learn,");
    println!("  positioned at design teams wanting a hosted in-browser editor.");
    println!("  Strong onboarding for designers + product managers without deep tool training.");
}

fn run_handoff() {
    println!("Developer handoff (Sympli heritage).");
    println!("  Inspect tab: CSS + Sass + Swift + Android colour / font / spacing values.");
    println!("  Asset export: PNG / SVG / JPEG / PDF at multiple resolutions per device.");
    println!("  Sticker sheets for component libraries shared across projects.");
    println!("  Sympli integration (acquired 2018): bridge to Sketch + Adobe XD source files.");
    println!("  Plays well with developers on Visual Studio + Xcode + Android Studio side.");
    println!("  This is the workflow that kept Marvel relevant after Figma's dominance.");
}

fn run_usertesting() {
    println!("Marvel User Testing.");
    println!("  Record real-user sessions on a Marvel prototype — taps, swipes, hesitations,");
    println!("  facial expression on opt-in, think-aloud audio.");
    println!("  Task scripts: 'find the cancel-subscription flow' + measure completion + time.");
    println!("  Annotated heatmaps of where testers tapped vs where they were expected to.");
    println!("  Comparable to Maze + UserTesting + Lookback but bundled with the prototype.");
    println!("  Common with UX research teams running formative + summative studies on UI.");
}

fn run_education() {
    println!("Marvel for Education.");
    println!("  Marvel runs a strong education programme — free + heavily-discounted tiers");
    println!("  for UK + EU design + product + UX courses at university + bootcamp level.");
    println!("  Many UK design students learn Marvel before encountering Figma in industry.");
    println!("  Common in further-education + degree programmes for visual + UX design.");
    println!("  This produces a long-term funnel into Marvel's professional tier — students");
    println!("  who already know Marvel often bring it into their first design jobs.");
    println!("  A genuine strategic differentiator vs Figma's enterprise + product-led growth.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free:       limited projects + Marvel branding + watermark.");
    println!("  Pro:        ~\\$12/user/month, unlimited projects + handoff + integrations.");
    println!("  Team:       ~\\$42/user/month, design library + advanced collaboration.");
    println!("  Enterprise: custom — SSO, advanced security, dedicated success, SLA.");
    println!("  Education:  free + deeply discounted tiers for verified students + educators.");
    println!("  Pricing is competitive vs Figma + InVision; the wedge is being simpler + cheaper.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: UK + EU SMB design + product teams 5-200 employees, agencies,");
    println!("  and design programmes at universities + bootcamps.");
    println!("  Industries: digital agencies, retail product design, financial services UX,");
    println!("  government digital services (UK GDS-adjacent teams), education,");
    println!("  charities + non-profits attracted by Marvel's UK base + lower pricing.");
    println!("  Geographic: very heavy UK + EU; modest US + APAC; growing emerging markets.");
    println!("  Common journey: designer wants something lighter than Figma + cheaper than");
    println!("  InVision Pro for a small agency or in-house team — picks Marvel.");
    println!("  Anti-segment: large enterprise design systems (still Figma + Sketch).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "marvel-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "prototype" => run_prototype(),
        "canvas" => run_canvas(),
        "handoff" => run_handoff(),
        "usertesting" => run_usertesting(),
        "education" => run_education(),
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
        run_canvas();
        run_handoff();
        run_usertesting();
        run_education();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("marvel-cli");
        print_version();
    }
}
