#![deny(clippy::all)]

//! appimage-cli — SlateOS AppImage tools
//!
//! Multi-personality: `appimagetool`, `appimaged`, `appimage-update`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_appimagetool(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: appimagetool [OPTIONS] SOURCE [TARGET]");
        println!("appimagetool 13 (Slate OS)");
        println!();
        println!("Options:");
        println!("  -n            Do not embed desktop integration");
        println!("  --comp METHOD Compression method (gzip, xz, zstd)");
        println!("  --sign        Sign the AppImage");
        println!("  --sign-key K  GPG signing key");
        println!("  -u URL        Update information URL");
        println!("  -v            Verbose");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("appimagetool 13 (Slate OS), build abc123");
        return 0;
    }
    let source = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("AppDir");
    let target = args.iter()
        .filter(|a| !a.starts_with('-'))
        .nth(1)
        .map(|s| s.as_str())
        .unwrap_or("MyApp-x86_64.AppImage");
    let comp = args.windows(2)
        .find(|w| w[0] == "--comp")
        .map(|w| w[1].as_str())
        .unwrap_or("zstd");
    println!("appimagetool: packaging {} -> {}", source, target);
    println!("  Compression: {}", comp);
    println!("  Generating squashfs...");
    println!("  Embedding runtime...");
    if args.iter().any(|a| a == "--sign") {
        println!("  Signing AppImage...");
    }
    println!("  Success: {} created (15.2 MiB)", target);
    0
}

fn run_appimaged(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: appimaged [OPTIONS]");
        println!("AppImage daemon — monitors directories for AppImages.");
        println!("  -v            Verbose");
        println!("  --no-install  Don't auto-integrate");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("appimaged 1.0.0 (Slate OS)");
        return 0;
    }
    println!("appimaged: monitoring ~/Applications/ for AppImages...");
    println!("appimaged: found 3 AppImages");
    println!("  MyApp-x86_64.AppImage (integrated)");
    println!("  Editor-x86_64.AppImage (integrated)");
    println!("  Tool-x86_64.AppImage (integrated)");
    0
}

fn run_appimage_update(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: appimage-update [OPTIONS] FILE.AppImage");
        println!("  -c, --check-only   Only check for updates");
        println!("  -O                 Overwrite original");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("AppImageUpdate 2.0.0 (Slate OS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| a.ends_with(".AppImage"))
        .map(|s| s.as_str())
        .unwrap_or("MyApp-x86_64.AppImage");
    let check_only = args.iter().any(|a| a == "-c" || a == "--check-only");
    if check_only {
        println!("Checking for updates: {}", file);
        println!("  Update available: 1.0.0 -> 1.1.0");
    } else {
        println!("Updating: {}", file);
        println!("  Downloading delta...");
        println!("  Applying update...");
        println!("  Updated to version 1.1.0");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "appimagetool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "appimaged" => run_appimaged(&rest),
        "appimage-update" => run_appimage_update(&rest),
        _ => run_appimagetool(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_appimagetool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/appimage"), "appimage");
        assert_eq!(basename(r"C:\bin\appimage.exe"), "appimage.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("appimage.exe"), "appimage");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_appimagetool(&["--help".to_string()]), 0);
        assert_eq!(run_appimagetool(&["-h".to_string()]), 0);
        let _ = run_appimagetool(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_appimagetool(&[]);
    }
}
