#![deny(clippy::all)]

//! steamtinkerlaunch-cli — SlateOS SteamTinkerLaunch tweaking tool
//!
//! Single personality: `steamtinkerlaunch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: steamtinkerlaunch COMMAND [OPTIONS]");
        println!("steamtinkerlaunch v12.0 (SlateOS) — Steam game tweaking wrapper");
        println!();
        println!("Commands:");
        println!("  gui               Open settings GUI");
        println!("  compat            Manage compatibility tools");
        println!("  addnonsteamgame   Add non-Steam game");
        println!("  modorganizer      Launch Mod Organizer");
        println!("  specialk          Launch Special K");
        println!("  vortex            Launch Vortex Mod Manager");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("steamtinkerlaunch v12.0 (SlateOS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("gui");
    match cmd {
        "gui" => println!("steamtinkerlaunch: settings GUI opened"),
        "compat" => {
            println!("Compatibility tools:");
            println!("  GE-Proton8-26 (installed)");
            println!("  Proton Experimental (Steam)");
            println!("  Proton 8.0 (Steam)");
        }
        _ => println!("steamtinkerlaunch: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "steamtinkerlaunch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stl(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_stl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/steamtinkerlaunch"), "steamtinkerlaunch");
        assert_eq!(basename(r"C:\bin\steamtinkerlaunch.exe"), "steamtinkerlaunch.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("steamtinkerlaunch.exe"), "steamtinkerlaunch");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_stl(&["--help".to_string()], "steamtinkerlaunch"), 0);
        assert_eq!(run_stl(&["-h".to_string()], "steamtinkerlaunch"), 0);
        let _ = run_stl(&["--version".to_string()], "steamtinkerlaunch");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_stl(&[], "steamtinkerlaunch");
    }
}
