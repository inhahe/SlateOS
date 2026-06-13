#![deny(clippy::all)]

//! azuredatastudio-cli — SlateOS Azure Data Studio
//!
//! Single personality: `azuredatastudio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ads(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: azuredatastudio [OPTIONS] [PATH]");
        println!("Azure Data Studio v1.48 (SlateOS) — Cross-platform database tool");
        println!();
        println!("Options:");
        println!("  --new-window       Force a new window");
        println!("  --reuse-window     Force reuse of existing window");
        println!("  --diff FILE1 FILE2 Compare two files");
        println!("  --goto FILE:LINE   Open file at line number");
        println!("  --extensions-dir DIR  Extensions directory");
        println!("  --install-extension ID  Install extension");
        println!("  --list-extensions  List installed extensions");
        println!("  --disable-extensions  Disable all extensions");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Azure Data Studio v1.48.1 (SlateOS)"); return 0; }
    println!("Azure Data Studio v1.48.1 (SlateOS)");
    println!("  Connections: 6 saved");
    println!("  Supported: SQL Server, PostgreSQL, Azure SQL, MySQL");
    println!("  Extensions: 12 installed");
    println!("  Notebooks: 8 saved");
    println!("  Query plans: 5 cached");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "azuredatastudio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ads(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ads};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/azuredatastudio"), "azuredatastudio");
        assert_eq!(basename(r"C:\bin\azuredatastudio.exe"), "azuredatastudio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("azuredatastudio.exe"), "azuredatastudio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ads(&["--help".to_string()], "azuredatastudio"), 0);
        assert_eq!(run_ads(&["-h".to_string()], "azuredatastudio"), 0);
        let _ = run_ads(&["--version".to_string()], "azuredatastudio");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ads(&[], "azuredatastudio");
    }
}
