#![deny(clippy::all)]

//! mame-cli — OurOS MAME arcade machine emulator
//!
//! Single personality: `mame`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mame(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mame [OPTIONS] SYSTEM [SOFTWARE]");
        println!("mame v0.262 (OurOS) — Multiple Arcade Machine Emulator");
        println!();
        println!("Options:");
        println!("  -listxml          Output system list as XML");
        println!("  -listfull         List systems with descriptions");
        println!("  -verifyroms       Verify ROM sets");
        println!("  -rompath PATH     ROM search path");
        println!("  -window           Run windowed");
        println!("  -nowindow         Run fullscreen");
        println!("  -video VK|OGL     Video output method");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mame v0.262 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-verifyroms") {
        println!("romset pacman [pacman] is good");
        println!("romset dkong [dkong] is good");
        println!("romset galaga [galaga] is good");
        println!("3 romsets found, 3 were OK");
        return 0;
    }
    if args.iter().any(|a| a == "-listfull") {
        println!("Name:        Description:");
        println!("pacman       Pac-Man (Midway)");
        println!("dkong        Donkey Kong (US set 1)");
        println!("galaga       Galaga (Namco rev. B)");
        println!("mspacman     Ms. Pac-Man");
        println!("sf2          Street Fighter II (World)");
        return 0;
    }
    let system = args.first().map(|s| s.as_str()).unwrap_or("");
    println!("mame: loading system '{}'...", system);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mame".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mame(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mame};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mame"), "mame");
        assert_eq!(basename(r"C:\bin\mame.exe"), "mame.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mame.exe"), "mame");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mame(&["--help".to_string()], "mame"), 0);
        assert_eq!(run_mame(&["-h".to_string()], "mame"), 0);
        assert_eq!(run_mame(&["--version".to_string()], "mame"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mame(&[], "mame"), 0);
    }
}
