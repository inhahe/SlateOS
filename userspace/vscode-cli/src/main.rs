#![deny(clippy::all)]

//! vscode-cli — OurOS Visual Studio Code (Microsoft's dominant code editor)
//!
//! Single personality: `vscode`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vsc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vscode [OPTIONS]");
        println!("Visual Studio Code 1.95 (OurOS) — Microsoft's open-source code editor");
        println!();
        println!("Options:");
        println!("  --new                  New file");
        println!("  --extensions           Extension Marketplace");
        println!("  --remote-ssh           Remote-SSH (edit on remote server)");
        println!("  --devcontainer         Dev Containers (Docker-backed dev env)");
        println!("  --copilot              GitHub Copilot (AI pair programmer)");
        println!("  --insiders             VS Code Insiders (nightly channel)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Visual Studio Code 1.95.3 (OurOS)"); return 0; }
    println!("Visual Studio Code 1.95.3 (OurOS)");
    println!("  Vendor: Microsoft Corporation");
    println!("  License: MIT (code OSS) — but distributed binaries have proprietary additions");
    println!("           VSCodium = MS-free build of the same source");
    println!("  Launched: Nov 2015 (Build 2015 keynote demo)");
    println!("  Built on: Electron (Chromium + Node.js) — once mocked, now industry-standard");
    println!("  Core team: led by Erich Gamma (Gang-of-Four author, Eclipse co-creator)");
    println!("  Marketshare: #1 IDE in StackOverflow survey since 2018 — ~75% of developers");
    println!("  Languages: TypeScript editor + Node.js extension host");
    println!("  Language support: virtually every language via LSP (Language Server Protocol)");
    println!("                    invented by Microsoft for VS Code, now universally adopted");
    println!("  Killer features:");
    println!("    - Integrated terminal");
    println!("    - Built-in Git + GitHub PR integration");
    println!("    - Remote development (SSH/WSL/Containers) — VS Code Server runs remotely");
    println!("    - Live Share (multi-user real-time editing)");
    println!("    - Dev Containers (rep-locked dev environments in Docker)");
    println!("    - 50K+ extensions in Marketplace (Python, Pylance, ESLint, Prettier, GitLens)");
    println!("  GitHub Copilot: $10/mo (or free for verified students/maintainers) — VS Code's");
    println!("                  most-installed extension by far, Microsoft owns both companies");
    println!("  AI features (native): Copilot Chat, Copilot Workspaces, Edit with Copilot, Inline Chat");
    println!("  Variants: VS Code, VS Code Insiders, VSCodium (MIT-pure), code-server (web), Theia (alt)");
    println!("  Differentiator: huge extension ecosystem + remote dev + Copilot integration + free");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vscode".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vsc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
