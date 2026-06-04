#![deny(clippy::all)]

//! canva-cli — OurOS Canva (web-based design platform, AU-founded, now owns Affinity)
//!
//! Single personality: `canva`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_canva(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: canva [OPTIONS]");
        println!("Canva 1.103 (OurOS) — Online graphic design platform (Sydney, Australia)");
        println!();
        println!("Options:");
        println!("  --new TYPE             social-post/presentation/doc/video/print/web");
        println!("  --magic-studio         Magic Studio (generative AI suite)");
        println!("  --magic-write          AI text generator (powered by OpenAI)");
        println!("  --magic-design         AI design generator");
        println!("  --brand-kit            Canva Brand Kit (colors/fonts/logos)");
        println!("  --teams                Canva for Teams (collab)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Canva 1.103.0 (OurOS)"); return 0; }
    println!("Canva 1.103.0 (OurOS)");
    println!("  Vendor: Canva Pty Ltd (HQ Sydney, Australia — founded 2013)");
    println!("  Founders: Melanie Perkins (CEO), Cliff Obrecht, Cameron Adams");
    println!("  Valuation: ~$26B USD (2024 secondary sale) — one of largest private tech firms");
    println!("  Mission: 'Empower everyone to design anything, publish anywhere'");
    println!("  Acquisition: Affinity (Serif Labs) — Mar 2024, brought pro tools in-house");
    println!("  Platform: web-first (Electron desktop wrapper), iOS/Android/iPad apps");
    println!("  Users: 200M+ monthly active (2024), 24B+ designs created");
    println!("  Free tier: huge library + basic tools — fremium model is the growth engine");
    println!("  Canva Pro: $14.99/mo or $119.99/yr (premium templates, Magic Studio, Brand Kit)");
    println!("  Canva for Teams: $14.99/mo for first 5 users, real-time collab");
    println!("  Magic Studio: Magic Write (text), Magic Design (layouts), Magic Edit/Eraser/Expand,");
    println!("               Magic Switch (resize), Magic Animate, DreamLab (image gen, ex-Leonardo.AI)");
    println!("  Acquisitions: Pexels, Pixabay (stock), Affinity (Photo/Designer/Publisher),");
    println!("               Flourish (data viz), Kaleido (Remove.bg), Leonardo.AI (Aug 2024)");
    println!("  Use cases: social media graphics, slide decks, marketing, simple video edits,");
    println!("            print docs (cards/posters/menus), websites, whiteboards, doc co-edit");
    println!("  Strength: insanely easy UX, vast template library, near-zero learning curve");
    println!("  Weakness: not for precision pro design (Affinity now covers that within group)");
    println!("  IPO: rumored 2025+ (rejected 2022 offers), profitable since 2017");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "canva".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_canva(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_canva};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/canva"), "canva");
        assert_eq!(basename(r"C:\bin\canva.exe"), "canva.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("canva.exe"), "canva");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_canva(&["--help".to_string()], "canva"), 0);
        assert_eq!(run_canva(&["-h".to_string()], "canva"), 0);
        let _ = run_canva(&["--version".to_string()], "canva");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_canva(&[], "canva");
    }
}
