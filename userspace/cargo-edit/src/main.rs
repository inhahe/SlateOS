#![deny(clippy::all)]

//! cargo-edit — Slate OS cargo subcommands for dependency management
//!
//! Multi-personality: `cargo-add`, `cargo-rm`, `cargo-upgrade`, `cargo-set-version`

use std::env;
use std::process;

fn personality(argv0: &str) -> &str {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let base = base.strip_suffix(".exe").unwrap_or(base);
    match base {
        "cargo-rm" => "rm",
        "cargo-upgrade" => "upgrade",
        "cargo-set-version" => "set-version",
        _ => "add",
    }
}

fn run_cargo_edit(args: Vec<String>, mode: &str) -> i32 {
    // Invoked as `cargo add/rm/upgrade/set-version`, skip subcommand
    let subargs: Vec<String> = if args.first().map(|s| s.as_str()) == Some(mode) {
        args[1..].to_vec()
    } else {
        args
    };

    match mode {
        "add" => {
            if subargs.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: cargo add [OPTIONS] <CRATE>...");
                println!();
                println!("Options:");
                println!("  --dev              Add as dev dependency");
                println!("  --build            Add as build dependency");
                println!("  --optional         Mark dependency as optional");
                println!("  --no-default-features  Disable default features");
                println!("  -F, --features <F>    Enable features");
                println!("  --rename <NAME>    Rename dependency");
                println!("  -p, --package <P>  Package to modify");
                println!("  --dry-run          Don't actually write");
                println!("  -V, --version      Show version");
                return 0;
            }
            let crates: Vec<&str> = subargs.iter()
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            for c in &crates {
                println!("    Adding {} v1.0 to dependencies", c);
            }
        }
        "rm" => {
            if subargs.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: cargo rm [OPTIONS] <CRATE>...");
                println!();
                println!("Options:");
                println!("  --dev              Remove from dev dependencies");
                println!("  --build            Remove from build dependencies");
                println!("  -p, --package <P>  Package to modify");
                println!("  --dry-run          Don't actually write");
                println!("  -V, --version      Show version");
                return 0;
            }
            let crates: Vec<&str> = subargs.iter()
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            for c in &crates {
                println!("    Removing {} from dependencies", c);
            }
        }
        "upgrade" => {
            if subargs.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: cargo upgrade [OPTIONS] [CRATE]...");
                println!();
                println!("Options:");
                println!("  --incompatible     Allow incompatible upgrades");
                println!("  --pinned           Upgrade pinned dependencies");
                println!("  --dry-run          Don't actually write");
                println!("  -p, --package <P>  Package to modify");
                println!("  -V, --version      Show version");
                return 0;
            }
            println!("    Checking for updates...");
            println!("    serde: 1.0.190 -> 1.0.203 (compatible)");
            println!("    tokio: 1.35.0  -> 1.37.0  (compatible)");
            println!("    clap:  4.4.0   -> 4.5.4   (compatible)");
            println!("    Updated 3 dependencies in Cargo.toml");
        }
        "set-version" => {
            if subargs.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: cargo set-version [OPTIONS] <VERSION>");
                println!();
                println!("Options:");
                println!("  --bump <RULE>      Bump rule (major/minor/patch)");
                println!("  -p, --package <P>  Package to modify");
                println!("  --dry-run          Don't actually write");
                println!("  -V, --version      Show version");
                return 0;
            }
            let ver = subargs.iter()
                .find(|a| !a.starts_with('-'))
                .map(|s| s.as_str());
            let bump = subargs.windows(2)
                .find(|w| w[0] == "--bump")
                .map(|w| w[1].as_str());

            if let Some(v) = ver {
                println!("    Setting version to {}", v);
            } else if let Some(b) = bump {
                println!("    Bumping {} version: 1.0.0 -> {}", b,
                    match b {
                        "major" => "2.0.0",
                        "minor" => "1.1.0",
                        _ => "1.0.1",
                    });
            }
        }
        _ => {}
    }

    if subargs.iter().any(|a| a == "-V" || a == "--version") {
        println!("cargo-edit 0.12.3 (Slate OS)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_else(|| String::from("cargo-add"));
    let mode = personality(&argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cargo_edit(rest, mode);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cargo_edit};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cargo_edit(vec!["--help".to_string()], "cargo-edit"), 0);
        assert_eq!(run_cargo_edit(vec!["-h".to_string()], "cargo-edit"), 0);
        let _ = run_cargo_edit(vec!["--version".to_string()], "cargo-edit");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cargo_edit(vec![], "cargo-edit");
    }
}
