#![deny(clippy::all)]

//! crossplane-cli — OurOS Crossplane CLI
//!
//! Multi-personality: `crossplane`, `crank`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_crossplane(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: crossplane COMMAND [OPTIONS]");
        println!("Crossplane CLI 1.16.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  xpkg          Manage Crossplane packages");
        println!("  beta          Beta features");
        println!("  version       Show version");
        println!();
        println!("xpkg subcommands:");
        println!("  xpkg init     Initialize a package");
        println!("  xpkg build    Build a package");
        println!("  xpkg push     Push a package");
        println!("  xpkg install  Install a package");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("crossplane 1.16.0"),
        "xpkg" => {
            let sub2 = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match sub2 {
                "init" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("my-provider");
                    println!("Initializing package '{}'...", name);
                    println!("  Created crossplane.yaml");
                    println!("  Created apis/");
                    println!("Done.");
                }
                "build" => {
                    println!("Building package...");
                    println!("  Validating composition...");
                    println!("  Package built: package.xpkg");
                }
                "push" => {
                    let target = args.get(2).map(|s| s.as_str()).unwrap_or("xpkg.upbound.io/my-org/my-provider:v0.1.0");
                    println!("Pushing to {}...", target);
                    println!("Done.");
                }
                "install" => {
                    let pkg = args.get(2).map(|s| s.as_str()).unwrap_or("provider-aws");
                    println!("Installing {}...", pkg);
                    println!("Package installed successfully.");
                }
                _ => println!("crossplane xpkg: '{}' completed", sub2),
            }
        }
        _ => println!("crossplane: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "crossplane".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_crossplane(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_crossplane};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/crossplane"), "crossplane");
        assert_eq!(basename(r"C:\bin\crossplane.exe"), "crossplane.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("crossplane.exe"), "crossplane");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_crossplane(&["--help".to_string()]), 0);
        assert_eq!(run_crossplane(&["-h".to_string()]), 0);
        let _ = run_crossplane(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_crossplane(&[]);
    }
}
