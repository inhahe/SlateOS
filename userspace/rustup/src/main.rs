#![deny(clippy::all)]

//! rustup — OurOS Rust toolchain installer and manager
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `rustup` (default) — toolchain manager
//! - `rustc` — Rust compiler (proxy)
//! - `cargo` — Rust package manager (proxy)
//! - `rustfmt` — Rust code formatter (proxy)
//! - `clippy-driver` — Rust linter (proxy)
//! - `rust-analyzer` — Rust language server (proxy)

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_rustup(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("rustup — The Rust toolchain installer");
            println!();
            println!("Usage: rustup [OPTIONS] <COMMAND>");
            println!();
            println!("Commands:");
            println!("  show        Show the active and installed toolchains");
            println!("  update      Update Rust toolchains and rustup");
            println!("  default     Set the default toolchain");
            println!("  toolchain   Modify or query the installed toolchains");
            println!("  target      Modify a toolchain's supported targets");
            println!("  component   Modify a toolchain's installed components");
            println!("  override    Modify toolchain overrides for directories");
            println!("  run         Run a command with an environment for a given toolchain");
            println!("  which       Display which binary will be run for a given command");
            println!("  doc         Open documentation for the current toolchain");
            println!("  self        Modify the rustup installation");
            println!("  --version   Show version");
            0
        }
        "--version" | "-V" => { println!("rustup 1.27.0 (OurOS)"); 0 }
        "show" => {
            println!("Default host: x86_64-ouros");
            println!("rustup home:  /home/user/.rustup");
            println!();
            println!("installed toolchains");
            println!("--------------------");
            println!("stable-x86_64-ouros (default)");
            println!("nightly-x86_64-ouros");
            println!();
            println!("active toolchain");
            println!("----------------");
            println!("stable-x86_64-ouros (default)");
            println!("rustc 1.77.0 (OurOS)");
            0
        }
        "update" => {
            let toolchain = cmd_args.first().map(|s| s.as_str());
            match toolchain {
                Some(tc) => {
                    println!("info: syncing channel updates for '{}'", tc);
                    println!("info: latest update on 2025-05-22");
                    println!("  {} updated - rustc 1.77.0", tc);
                }
                None => {
                    println!("info: syncing channel updates for 'stable-x86_64-ouros'");
                    println!("info: syncing channel updates for 'nightly-x86_64-ouros'");
                    println!("  stable-x86_64-ouros unchanged - rustc 1.77.0");
                    println!("  nightly-x86_64-ouros updated - rustc 1.79.0-nightly");
                }
            }
            0
        }
        "default" => {
            if let Some(tc) = cmd_args.first() {
                println!("info: default toolchain set to '{}'", tc);
            } else {
                println!("stable-x86_64-ouros (default)");
            }
            0
        }
        "toolchain" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("stable-x86_64-ouros (default)");
                    println!("nightly-x86_64-ouros");
                }
                "install" => {
                    let tc = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("stable");
                    println!("info: installing toolchain '{}'", tc);
                    println!("info: toolchain '{}' installed", tc);
                }
                "uninstall" => {
                    let tc = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("nightly");
                    println!("info: uninstalling toolchain '{}'", tc);
                }
                _ => println!("toolchain {}: (simulated)", sub),
            }
            0
        }
        "target" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("x86_64-ouros (installed)");
                    println!("x86_64-unknown-linux-gnu");
                    println!("x86_64-pc-windows-msvc");
                    println!("aarch64-ouros");
                    println!("wasm32-unknown-unknown");
                }
                "add" => {
                    let target = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("target");
                    println!("info: downloading component 'rust-std' for '{}'", target);
                    println!("info: installing component 'rust-std' for '{}'", target);
                }
                _ => println!("target {}: (simulated)", sub),
            }
            0
        }
        "component" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("cargo-x86_64-ouros (installed)");
                    println!("clippy-x86_64-ouros (installed)");
                    println!("rust-analyzer-x86_64-ouros (installed)");
                    println!("rust-src (installed)");
                    println!("rust-std-x86_64-ouros (installed)");
                    println!("rustc-x86_64-ouros (installed)");
                    println!("rustfmt-x86_64-ouros (installed)");
                    println!("llvm-tools");
                    println!("miri");
                }
                "add" => {
                    let comp = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("component");
                    println!("info: downloading component '{}'", comp);
                    println!("info: installing component '{}'", comp);
                }
                _ => println!("component {}: (simulated)", sub),
            }
            0
        }
        "which" => {
            let binary = cmd_args.first().map(|s| s.as_str()).unwrap_or("rustc");
            println!("/home/user/.rustup/toolchains/stable-x86_64-ouros/bin/{}", binary);
            0
        }
        "doc" => { println!("Opening documentation in browser (simulated)"); 0 }
        "self" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("update");
            match sub {
                "update" => println!("info: checking for self-updates\ninfo: rustup is up to date"),
                "uninstall" => println!("info: this will uninstall rustup (simulated)"),
                _ => println!("self {}: (simulated)", sub),
            }
            0
        }
        "run" => {
            let tc = cmd_args.first().map(|s| s.as_str()).unwrap_or("stable");
            println!("(running with toolchain {} — simulated)", tc);
            0
        }
        "override" => { println!("(override management — simulated)"); 0 }
        other => { eprintln!("rustup: unknown command '{}'", other); 1 }
    }
}

fn run_rustc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("rustc 1.77.0 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rustc [OPTIONS] INPUT");
        return 0;
    }
    let file = args.first().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("main.rs");
    println!("Compiling {} (simulated)", file);
    0
}

fn run_cargo(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    match cmd.as_str() {
        "--version" | "-V" => { println!("cargo 1.77.0 (OurOS)"); 0 }
        "build" | "b" => { println!("   Compiling myproject v0.1.0 (/project)\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.5s"); 0 }
        "run" | "r" => { println!("   Compiling myproject v0.1.0\n    Finished `dev` profile\n     Running `target/debug/myproject`\n(simulated)"); 0 }
        "test" | "t" => { println!("   Compiling myproject v0.1.0\n    Finished `test` profile\n     Running tests\ntest result: ok. 5 passed; 0 failed (simulated)"); 0 }
        "check" | "c" => { println!("    Checking myproject v0.1.0\n    Finished `dev` profile in 0.5s"); 0 }
        "clippy" => { println!("    Checking myproject v0.1.0\n    Finished `dev` profile (no warnings)"); 0 }
        "fmt" => { println!("(formatting — simulated)"); 0 }
        "new" => { let name = args.get(1).map(|s| s.as_str()).unwrap_or("myproject"); println!("     Created binary (application) `{}` package", name); 0 }
        "init" => { println!("     Created binary (application) package"); 0 }
        "add" => { let dep = args.get(1).map(|s| s.as_str()).unwrap_or("dep"); println!("      Adding {} v1.0.0 to dependencies", dep); 0 }
        "publish" => { println!("   Publishing myproject v0.1.0 (simulated)"); 0 }
        _ => { println!("Rust's package manager\n\nUsage: cargo [OPTIONS] [COMMAND]"); 0 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("rustup");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "rustc" => run_rustc(rest),
        "cargo" => run_cargo(rest),
        "rustfmt" => { println!("(rustfmt — simulated)"); 0 }
        "clippy-driver" => { println!("(clippy — simulated)"); 0 }
        "rust-analyzer" => { println!("rust-analyzer 1.77.0 (OurOS) — language server"); 0 }
        _ => run_rustup(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{run_rustup};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_rustup(vec!["--help".to_string()]), 0);
        assert_eq!(run_rustup(vec!["-h".to_string()]), 0);
        assert_eq!(run_rustup(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_rustup(vec![]), 0);
    }
}
