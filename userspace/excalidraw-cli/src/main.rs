#![deny(clippy::all)]
//! excalidraw-cli — personality CLI for Excalidraw, the open-source
//! hand-drawn-style virtual whiteboard.
//!
//! Started in January 2020 by Christopher Chedeau (vjeux, ex-Facebook
//! React core team) as a weekend hobby project — a tiny TypeScript
//! React app that drew shapes with a deliberately rough, sketchy,
//! hand-drawn aesthetic via Rough.js. The minimalism + instantly-
//! shareable URLs + the aesthetic took off virally during the COVID-
//! remote-work surge. Excalidraw is now MIT-licensed open source on
//! GitHub with 80k+ stars + a vibrant maintainer community led by
//! Christopher Chedeau, David Luzar, Aakansha Doshi, Lipis. There is
//! also a commercial Excalidraw+ tier launched 2022 + acquired by
//! Notion in 2024, but the core editor remains free + open source.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Excalidraw open-source hand-drawn whiteboard personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Chedeau + maintainers Jan 2020; MIT; Notion acquisition 2024");
    println!("    canvas        Sketchy hand-drawn shapes + arrows + text + freedraw");
    println!("    plus          Excalidraw+ commercial tier + Notion 2024 acquisition");
    println!("    library       Libraries: published community shape collections");
    println!("    selfhost      Self-host the editor + docker + npm package + embed");
    println!("    rough         Rough.js — the sketchy-drawing library underneath");
    println!("    customers     Engineers + product + technical writers + diagramming-averse");
    println!("    licence       MIT licence + GitHub-first + permissive ecosystem");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("excalidraw-cli 0.1.0 (open-source-sketchy-whiteboard personality build)"); }

fn run_about() {
    println!("Excalidraw (open-source project).");
    println!("  Started:    January 2020 by Christopher Chedeau (vjeux), ex-Facebook React.");
    println!("  Maintainers (core team): Christopher Chedeau, David Luzar, Aakansha Doshi,");
    println!("                            Lipis, plus a wider community of contributors.");
    println!("  Licence:    MIT (the editor + libraries are fully open source).");
    println!("  GitHub:     excalidraw/excalidraw — 80k+ stars + extremely active issues.");
    println!("  Origin:     a weekend hobby project that went viral during 2020 remote work.");
    println!("  Commercial: Excalidraw+ tier launched 2022 for paid hosted version.");
    println!("  Acquired:   Excalidraw+ acquired by Notion in 2024;");
    println!("              core MIT editor remains independent + open source.");
}

fn run_canvas() {
    println!("Canvas + aesthetic.");
    println!("  Deliberately rough, sketchy, hand-drawn-feel strokes via Rough.js underneath.");
    println!("  Primitives: rectangle, ellipse, diamond, line, arrow, freedraw, text, image.");
    println!("  Snap-to-grid + alignment guides + group + lock for structured diagrams.");
    println!("  Curved + straight + step arrows with directional + bidirectional heads.");
    println!("  Excalidraw library shapes: AWS, GCP, Azure, hand-drawn icons, etc.");
    println!("  Export to PNG / SVG / clipboard / .excalidraw JSON file (the source format).");
    println!("  The aesthetic is a feature: 'this is a sketch, not a finished diagram'.");
}

fn run_plus() {
    println!("Excalidraw+ + Notion acquisition.");
    println!("  Launched 2022 as a paid hosted tier: workspaces, teams, persistent links,");
    println!("  large file size, asset CDN, comments, custom domains, AI diagram-from-text.");
    println!("  Funded development for the open-source core through Excalidraw+ revenue.");
    println!("  2024: Notion acquires Excalidraw+ — Notion embeds Excalidraw deeply into");
    println!("  Notion docs as a built-in diagramming surface alongside Notion's own canvas.");
    println!("  Critical commitment: the core open-source editor on excalidraw.com remains");
    println!("  freely available + MIT-licensed + community-led post-acquisition.");
}

fn run_library() {
    println!("Excalidraw Libraries.");
    println!("  Community-published collections of shapes packaged as .excalidrawlib files.");
    println!("  Browse + install from libraries.excalidraw.com — official library hub.");
    println!("  Popular libraries: AWS architecture icons, GCP, Azure, Kubernetes, system");
    println!("  design symbols, UI wireframe kits, flowchart kits, hand-drawn emoji + icons.");
    println!("  Publish a library by submitting a PR to the libraries GitHub repository —");
    println!("  the same open-source workflow as the editor itself.");
    println!("  Drag-drop import: pull a library into any Excalidraw session in one click.");
}

fn run_selfhost() {
    println!("Self-host + embed.");
    println!("  Docker image: official excalidraw/excalidraw image runs the full editor.");
    println!("  npm package: @excalidraw/excalidraw — embed the editor as a React component");
    println!("  in your own application; very popular for in-app collaborative diagramming.");
    println!("  Realtime collaboration backend: optional + separate (excalidraw-room).");
    println!("  Used as embedded canvas inside Logseq, Outline, Obsidian (plug-in), CodeSandbox,");
    println!("  GitBook, Replit, plus many internal product spec docs at engineering orgs.");
    println!("  This embeddability is a core reason Excalidraw is the open-source default.");
}

fn run_rough() {
    println!("Rough.js (the sketchy-drawing library underneath).");
    println!("  Rough.js: tiny JavaScript library that renders normal SVG / Canvas paths with");
    println!("  hand-drawn-looking jitter, multiple overlaid strokes, sketchy fills.");
    println!("  Authored by Preet Shihn; MIT licensed; widely reused beyond Excalidraw.");
    println!("  The Excalidraw aesthetic is essentially Rough.js applied consistently across");
    println!("  every primitive on the canvas, with carefully tuned seeded randomness so");
    println!("  shapes stay stable across re-renders + don't visually shimmer.");
    println!("  A great example of a small open-source library defining a category aesthetic.");
}

fn run_customers() {
    println!("User profile.");
    println!("  Sweet spot: software engineers + technical writers + product managers who");
    println!("  want to sketch a system diagram + flowchart + sequence + wireframe quickly");
    println!("  without opening Lucidchart, Visio, or even logging into Miro.");
    println!("  Common use cases: system design interviews, RFC docs, on-call runbooks,");
    println!("  architecture decision records (ADRs), conference talk slides, blog posts.");
    println!("  Geographic: global; particularly strong in the engineering + open-source");
    println!("  developer community; widely used in education + classroom whiteboarding.");
    println!("  Common pattern: 'I just want a diagram in 60 seconds + export PNG into the doc'.");
    println!("  Anti-segment: enterprise design-systems + workshop facilitation at scale.");
}

fn run_licence() {
    println!("Licence + ecosystem.");
    println!("  Editor + libraries: MIT licence.");
    println!("  Source on GitHub: excalidraw/excalidraw + open governance + clear contributing.");
    println!("  Open-source funding: Open Collective + Excalidraw+ revenue subsidised the");
    println!("  core editor development before the Notion acquisition.");
    println!("  Post-2024: Notion's stewardship of Excalidraw+ is structured to keep the");
    println!("  open-source core free + permissively licensed indefinitely.");
    println!("  One of the most successful 2020s-era open-source-tools-that-built-a-business");
    println!("  stories — comparable to PlanetScale's Vitess or Vercel's Next.js.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "excalidraw-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "canvas" => run_canvas(),
        "plus" => run_plus(),
        "library" => run_library(),
        "selfhost" => run_selfhost(),
        "rough" => run_rough(),
        "customers" => run_customers(),
        "licence" | "license" => run_licence(),
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
        run_canvas();
        run_plus();
        run_library();
        run_selfhost();
        run_rough();
        run_customers();
        run_licence();
    }

    #[test]
    fn help_and_version() {
        print_help("excalidraw-cli");
        print_version();
    }
}
