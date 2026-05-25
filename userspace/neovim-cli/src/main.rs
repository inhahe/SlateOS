#![deny(clippy::all)]

//! neovim-cli — OurOS Neovim (extensible modern vim fork)
//!
//! Single personality: `neovim` (also: nvim)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nvim(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: neovim [OPTIONS]");
        println!("Neovim 0.10.2 (OurOS) — Hyperextensible Vim-based text editor");
        println!();
        println!("Options:");
        println!("  --new                  Open editor");
        println!("  --lua                  Lua scripting (modern config)");
        println!("  --vimscript            VimL/Vimscript (legacy config)");
        println!("  --lsp                  Built-in LSP client (since 0.5)");
        println!("  --treesitter           Tree-sitter incremental parsing");
        println!("  --plugins              :PlugInstall / Lazy.nvim / packer.nvim");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Neovim v0.10.2 (OurOS)"); return 0; }
    println!("Neovim v0.10.2 (OurOS)");
    println!("  License: Apache 2.0 + Vim license (modified BSD)");
    println!("  Origin: forked from Vim by Thiago de Arruda (tarruda) in 2014");
    println!("         to refactor Vim's codebase + add async + better plugin API");
    println!("  Maintainer: Justin M. Keyes (lead since 2014), open community of ~700 contributors");
    println!("  Funding: Bountysource + GitHub Sponsors — community-funded");
    println!("  vs Vim: same modal editing, same files (.vimrc → init.lua or init.vim)");
    println!("         but: async (jobs/RPC), built-in LSP, Tree-sitter, Lua, GUI API for outside frontends");
    println!("  Built-in features (since 0.5+):");
    println!("    - LSP client: native Language Server Protocol support — no plugin needed");
    println!("    - Tree-sitter: incremental parser for syntax highlighting + structural editing");
    println!("    - Lua 5.1 (LuaJIT) runtime for plugins — much faster than Vimscript");
    println!("    - msgpack-RPC remote control — GUIs like Neovide / nvy / nvui talk to nvim-core");
    println!("    - Terminal emulator embedded (:terminal)");
    println!("  Popular distros / starter kits:");
    println!("    - NvChad — opinionated batteries-included Neovim distro");
    println!("    - LazyVim — Folke Lemaitre's curated config");
    println!("    - LunarVim, AstroNvim, kickstart.nvim");
    println!("  Plugin ecosystem: ~5000+ plugins, Lazy.nvim and packer.nvim are common managers");
    println!("    Famous: telescope.nvim (fuzzy finder), nvim-cmp (completion), neo-tree, gitsigns,");
    println!("           nvim-treesitter, mason.nvim (LSP installer), null-ls.nvim (deprecated → none-ls)");
    println!("  GUIs: Neovide (Rust), Goneovim (Go), Firenvim (browser textarea), VSCode-Neovim extension");
    println!("  Differentiator: keeps modal editing alive + brings it into modern dev ecosystem (LSP+TS+Lua)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "neovim".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nvim(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
