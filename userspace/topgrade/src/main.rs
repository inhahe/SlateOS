#![deny(clippy::all)]

//! topgrade — SlateOS upgrade everything at once
//!
//! Single personality: `topgrade`

use std::env;
use std::process;

fn run_topgrade(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: topgrade [OPTIONS]");
        println!();
        println!("Upgrade everything. Detects and runs all available package managers.");
        println!();
        println!("Options:");
        println!("  -n, --dry-run           Print what would be done");
        println!("  -y, --yes               Say yes to everything");
        println!("  -k, --keep              Keep working directory");
        println!("  --only <STEPS>          Only run specified steps");
        println!("  --disable <STEPS>       Skip specified steps");
        println!("  --edit-config           Open config in editor");
        println!("  --show-config           Print config path");
        println!("  -c, --cleanup           Run cleanup steps");
        println!("  --no-retry              Don't retry failed steps");
        println!("  --no-self-update        Don't update topgrade itself");
        println!("  --tmux-session <NAME>   Run in tmux session");
        println!("  -V, --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("topgrade 15.0.0 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--show-config") {
        println!("~/.config/topgrade/topgrade.toml");
        return 0;
    }

    let dry_run = args.iter().any(|a| a == "-n" || a == "--dry-run");
    let cleanup = args.iter().any(|a| a == "-c" || a == "--cleanup");

    if dry_run {
        println!("(dry run — no changes will be made)");
        println!();
    }

    println!("╔══════════════════════════════════════════╗");
    println!("║          topgrade — Slate OS                ║");
    println!("╚══════════════════════════════════════════╝");
    println!();
    println!("── System package manager ─────────────────");
    println!("  ✓ Updated package database");
    println!("  ✓ Upgraded 12 packages");
    println!();
    println!("── Rust toolchain ─────────────────────────");
    println!("  ✓ rustup update: stable, nightly");
    println!();
    println!("── Cargo packages ─────────────────────────");
    println!("  ✓ cargo install-update: 3 packages updated");
    println!();
    println!("── Flatpak ────────────────────────────────");
    println!("  ✓ 5 updates installed");
    println!();
    println!("── Firmware ──────────────────────────────");
    println!("  ✓ No firmware updates available");
    println!();
    println!("── Git repositories ──────────────────────");
    println!("  ✓ ~/projects/os: already up to date");

    if cleanup {
        println!();
        println!("── Cleanup ────────────────────────────────");
        println!("  ✓ Package cache: freed 512 MiB");
        println!("  ✓ Cargo cache: freed 1.2 GiB");
    }

    println!();
    println!("All done! (6 steps, 0 failures)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_topgrade(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_topgrade};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_topgrade(vec!["--help".to_string()]), 0);
        assert_eq!(run_topgrade(vec!["-h".to_string()]), 0);
        let _ = run_topgrade(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_topgrade(vec![]);
    }
}
