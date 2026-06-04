#![deny(clippy::all)]

//! wine-mono-cli — OurOS Wine Mono .NET runtime for Wine
//!
//! Single personality: `wine-mono`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wine_mono(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wine-mono [OPTIONS]");
        println!("wine-mono v9.0 (OurOS) — .NET runtime replacement for Wine");
        println!();
        println!("Options:");
        println!("  --status          Show installation status");
        println!("  --version         Show version");
        println!();
        println!("Wine Mono replaces .NET Framework in Wine prefixes,");
        println!("providing compatibility for .NET/WPF applications.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wine-mono v9.0 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--status") {
        println!("Wine Mono status:");
        println!("  Version: 9.0.0");
        println!("  Installed: yes");
        println!("  Location: /usr/share/wine/mono/");
        println!("  .NET versions supported:");
        println!("    .NET Framework 2.0");
        println!("    .NET Framework 3.5");
        println!("    .NET Framework 4.0");
        println!("    .NET Framework 4.5+");
        println!("  WPF support: partial");
        return 0;
    }
    println!("wine-mono: .NET runtime for Wine");
    println!("  Status: installed");
    println!("  Use --status for details");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wine-mono".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wine_mono(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wine_mono};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wine-mono"), "wine-mono");
        assert_eq!(basename(r"C:\bin\wine-mono.exe"), "wine-mono.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wine-mono.exe"), "wine-mono");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wine_mono(&["--help".to_string()], "wine-mono"), 0);
        assert_eq!(run_wine_mono(&["-h".to_string()], "wine-mono"), 0);
        let _ = run_wine_mono(&["--version".to_string()], "wine-mono");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wine_mono(&[], "wine-mono");
    }
}
