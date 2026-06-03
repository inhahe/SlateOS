#![deny(clippy::all)]

//! screenfetch-cli — OurOS screenFetch system information
//!
//! Single personality: `screenfetch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_screenfetch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: screenfetch [OPTIONS]");
        println!("screenfetch v3.9 (OurOS) — System information screenshot");
        println!();
        println!("Options:");
        println!("  -n             No ASCII art");
        println!("  -N             Strip colors");
        println!("  -s             Take screenshot");
        println!("  -D DISTRO      Set distro");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("screenfetch v3.9 (OurOS)"); return 0; }
    println!("         OS: OurOS 1.0");
    println!("     Kernel: 0.1.0-ouros");
    println!("     Uptime: 2h 15m");
    println!("      Shell: kshell");
    println!("        CPU: AMD Ryzen 7 @ 3.6GHz");
    println!("        GPU: AMD Radeon");
    println!("        RAM: 4096MiB / 16384MiB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "screenfetch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_screenfetch(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_screenfetch};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/screenfetch"), "screenfetch");
        assert_eq!(basename(r"C:\bin\screenfetch.exe"), "screenfetch.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("screenfetch.exe"), "screenfetch");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_screenfetch(&["--help".to_string()], "screenfetch"), 0);
        assert_eq!(run_screenfetch(&["-h".to_string()], "screenfetch"), 0);
        assert_eq!(run_screenfetch(&["--version".to_string()], "screenfetch"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_screenfetch(&[], "screenfetch"), 0);
    }
}
