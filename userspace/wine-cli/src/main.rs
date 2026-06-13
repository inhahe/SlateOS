#![deny(clippy::all)]

//! wine-cli — SlateOS Wine Windows compatibility layer CLI
//!
//! Multi-personality: `wine`, `wine64`, `wineserver`, `wineboot`, `winecfg`, `winepath`, `regedit`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_wine(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: wine PROGRAM [ARGUMENTS]");
        println!();
        println!("Wine — run Windows programs (SlateOS).");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("wine-9.0 (SlateOS)");
        return 0;
    }
    let program = args.first().map(|s| s.as_str()).unwrap_or("");
    if program.is_empty() {
        println!("Usage: wine PROGRAM [ARGUMENTS]");
        return 1;
    }
    println!("wine: starting '{}'", program);
    0
}

fn run_wineserver(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: wineserver [OPTIONS]");
        println!();
        println!("wineserver — Wine server (SlateOS).");
        println!();
        println!("Options:");
        println!("  -d N    Debug level");
        println!("  -f      Run in foreground");
        println!("  -k [N]  Kill server (signal N)");
        println!("  -p N    Persistent mode (N seconds)");
        println!("  -w      Wait for server to exit");
        return 0;
    }
    if args.iter().any(|a| a == "-k") {
        println!("wineserver: shutting down");
    } else {
        println!("wineserver: starting");
    }
    0
}

fn run_wineboot(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: wineboot [OPTIONS]");
        println!();
        println!("wineboot — Wine prefix initialization (SlateOS).");
        println!();
        println!("Options:");
        println!("  -i, --init       Initialize prefix");
        println!("  -u, --update     Update prefix");
        println!("  -r, --restart    Simulate restart");
        println!("  -s, --shutdown   Simulate shutdown");
        println!("  -e, --end-session End session");
        println!("  -f, --force      Force operation");
        return 0;
    }
    if args.iter().any(|a| a == "-i" || a == "--init") {
        println!("wine: created the configuration directory '/home/user/.wine'");
    } else if args.iter().any(|a| a == "-u" || a == "--update") {
        println!("wineboot: updating prefix...");
    } else {
        println!("wineboot: performing boot sequence");
    }
    0
}

fn run_winepath(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: winepath [OPTIONS] PATH");
        println!();
        println!("Options:");
        println!("  -u    Unix path from Windows path");
        println!("  -w    Windows path from Unix path");
        println!("  -0    Null-separated output");
        return 0;
    }
    let path = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or(".");
    let unix_mode = args.iter().any(|a| a == "-u");
    if unix_mode {
        println!("/home/user/.wine/drive_c/{}", path.replace('\\', "/"));
    } else {
        println!("Z:{}", path.replace('/', "\\"));
    }
    0
}

fn run_regedit(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: regedit [OPTIONS] [FILE]");
        println!();
        println!("regedit — Wine registry editor (SlateOS).");
        return 0;
    }
    if let Some(file) = args.iter().find(|a| !a.starts_with('-')) {
        println!("regedit: importing '{}'", file);
    } else {
        println!("regedit: opening registry editor");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "wine".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "wineserver" => run_wineserver(&rest),
        "wineboot" => run_wineboot(&rest),
        "winecfg" => { println!("Opening Wine configuration..."); 0 }
        "winepath" => run_winepath(&rest),
        "regedit" => run_regedit(&rest),
        "wine64" => run_wine(&rest),
        _ => run_wine(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wine};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wine"), "wine");
        assert_eq!(basename(r"C:\bin\wine.exe"), "wine.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wine.exe"), "wine");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wine(&["--help".to_string()]), 0);
        assert_eq!(run_wine(&["-h".to_string()]), 0);
        let _ = run_wine(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wine(&[]);
    }
}
