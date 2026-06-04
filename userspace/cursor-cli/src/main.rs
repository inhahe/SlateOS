#![deny(clippy::all)]

//! cursor-cli — OurOS Cursor (AI-first VS Code fork by Anysphere)
//!
//! Single personality: `cursor`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_curs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cursor [OPTIONS]");
        println!("Cursor 0.43 (OurOS) — AI-native code editor (VS Code fork by Anysphere)");
        println!();
        println!("Options:");
        println!("  --new                  New file");
        println!("  --cmd-k                Cmd+K (inline AI edit on selection)");
        println!("  --cmd-l                Cmd+L (Composer / multi-file Chat panel)");
        println!("  --tab                  Tab — predictive code completion (Cursor's killer feature)");
        println!("  --agent                Agent mode (autonomous multi-step coding)");
        println!("  --models               Choose model (Claude / GPT-4o / Gemini / o1)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cursor 0.43.6 (OurOS)"); return 0; }
    println!("Cursor 0.43.6 (OurOS)");
    println!("  Vendor: Anysphere Inc. (HQ San Francisco — founded 2022)");
    println!("  Founders: Michael Truell, Sualeh Asif, Aman Sanger, Arvid Lunnemark (MIT grads)");
    println!("  Funding: a16z + Thrive + Index — Series B $105M (Aug 2024), $2.5B valuation");
    println!("          Series C $100M (Oct 2024), $2.5B+ → grew to $9B (Q4 2024 talks)");
    println!("  Built on: VS Code OSS fork (the open-source half) — extension-compatible with VS Code");
    println!("           Anysphere wrote the AI integration layer + custom UI shells");
    println!("  Pricing: Free tier (limited slow GPT-4 completions)");
    println!("          Pro $20/mo — 500 fast GPT-4 / Claude Sonnet completions + unlimited slow");
    println!("          Business $40/mo per user — team admin + privacy mode");
    println!("  Models: GPT-4o, GPT-4 Turbo, Claude 3.5 Sonnet, Claude 3.5 Haiku, Gemini Pro 1.5, o1-preview / o1-mini");
    println!("  Killer features:");
    println!("    - 'Tab' completion: predictive multi-line edits, jump-to-next edit hint (insanely fast UX)");
    println!("    - Cmd+K: select code → write natural-language instruction → AI edits inline with diff");
    println!("    - Cmd+L (Chat/Composer): multi-file edits with chat panel, @-mention files/symbols/docs");
    println!("    - Agent mode (since Oct 2024): autonomous mode — Cursor runs commands, edits files, debugs");
    println!("    - @Codebase: indexes your entire repo for context-aware suggestions");
    println!("    - @Docs: bring official library docs into the prompt");
    println!("  Privacy: 'Privacy Mode' (Pro+) — Anysphere doesn't store code, models don't train on it");
    println!("  Critique: closed source (despite VS Code OSS base), telemetry concerns, fast-moving UX");
    println!("  Differentiator: tab-key prediction UX is unmatched — feels like AI knows your next edit");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cursor".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_curs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_curs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cursor"), "cursor");
        assert_eq!(basename(r"C:\bin\cursor.exe"), "cursor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cursor.exe"), "cursor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_curs(&["--help".to_string()], "cursor"), 0);
        assert_eq!(run_curs(&["-h".to_string()], "cursor"), 0);
        let _ = run_curs(&["--version".to_string()], "cursor");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_curs(&[], "cursor");
    }
}
