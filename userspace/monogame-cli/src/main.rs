#![deny(clippy::all)]

//! monogame-cli — SlateOS MonoGame (open-source XNA successor)
//!
//! Single personality: `monogame`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: monogame [OPTIONS]");
        println!("MonoGame 3.8.2 (SlateOS) — Cross-platform .NET game framework (XNA spiritual successor)");
        println!();
        println!("Options:");
        println!("  --new TEMPLATE         New project (windowsdx/opengl/android/ios)");
        println!("  --content              MonoGame Content Pipeline (asset preprocessor, .mgcb files)");
        println!("  --mgcb-editor          MGCB Editor (GUI for content pipeline)");
        println!("  --templates            dotnet new templates (mgwindowsdx / mgdesktopgl / etc.)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("MonoGame 3.8.2.1105 (SlateOS)"); return 0; }
    println!("MonoGame 3.8.2.1105 (SlateOS)");
    println!("  License: MS-PL (Microsoft Public License) — fully open source");
    println!("  Repo: github.com/MonoGame/MonoGame");
    println!("  Maintainer: MonoGame Foundation (501c6 non-profit, established 2023)");
    println!("  Pricing: FREE — community foundation funded by donations / sponsorships");
    println!("  Origin: API-compatible reimplementation of Microsoft XNA Game Studio 4.0 (2010)");
    println!("         (XNA was discontinued by Microsoft 2013; MonoGame community took over)");
    println!("  Language: C# (or any .NET language) — built on Mono / .NET 8 cross-platform runtime");
    println!("  Targets: Windows (DX11), Linux/Mac (OpenGL), iOS, Android, console (PS4/5, Xbox, Switch via licensing)");
    println!("  Philosophy: code-first 2D/3D framework — no editor, no scene graph, just an API");
    println!("             you write Update() + Draw() loops, draw sprites + meshes manually");
    println!("  Famous MonoGame games:");
    println!("    - Stardew Valley (ConcernedApe, 2016) — 30M+ copies sold");
    println!("    - Celeste (Maddy Thorson + Noel Berry, 2018) — Game Awards GOTY nominee");
    println!("    - FEZ (Phil Fish, 2012)");
    println!("    - Bastion (Supergiant, 2011) — Supergiant's first hit");
    println!("    - Streets of Rage 4 (Dotemu / Lizardcube, 2020)");
    println!("    - Owlboy, Salt and Sanctuary, Axiom Verge");
    println!("  Differentiator: code-only, lightweight, full C# experience — preferred by 2D AAA-quality indies");
    println!("  Compared to: Unity (heavyweight editor), Godot (different language), MonoGame is just C# + a render API");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "monogame".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mg(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mg};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/monogame"), "monogame");
        assert_eq!(basename(r"C:\bin\monogame.exe"), "monogame.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("monogame.exe"), "monogame");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mg(&["--help".to_string()], "monogame"), 0);
        assert_eq!(run_mg(&["-h".to_string()], "monogame"), 0);
        let _ = run_mg(&["--version".to_string()], "monogame");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mg(&[], "monogame");
    }
}
