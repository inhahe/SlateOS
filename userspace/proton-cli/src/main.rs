#![deny(clippy::all)]

//! proton-cli — Slate OS Proton/Steam compatibility CLI
//!
//! Multi-personality: `proton`, `steam`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_proton(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: proton VERB [OPTIONS]");
        println!();
        println!("Proton — Steam Play compatibility tool (Slate OS).");
        println!();
        println!("Verbs:");
        println!("  run PROGRAM          Run program through Proton");
        println!("  waitforexitandrun P   Wait for prefix, then run");
        println!("  getcompatpath P       Get compat path");
        println!("  getnativepath P       Get native path");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Proton 9.0-1 (Slate OS)");
        return 0;
    }

    let verb = args.first().map(|s| s.as_str()).unwrap_or("");
    match verb {
        "run" | "waitforexitandrun" => {
            let program = args.get(1).map(|s| s.as_str()).unwrap_or("game.exe");
            println!("Proton: setting up prefix...");
            println!("Proton: DXVK enabled");
            println!("Proton: VKD3D enabled");
            println!("Proton: running '{}'", program);
        }
        "getcompatpath" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("/home/user");
            println!("Z:{}", path.replace('/', "\\"));
        }
        "getnativepath" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("C:\\");
            println!("/home/user/.steam/compatdata/prefix/drive_c/{}", path.replace('\\', "/"));
        }
        _ => {
            eprintln!("proton: unknown verb '{}'. See --help.", verb);
            return 1;
        }
    }
    0
}

fn run_steam(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: steam [OPTIONS] [steam://URL]");
        println!();
        println!("Steam — game platform client (Slate OS).");
        println!();
        println!("Options:");
        println!("  -applaunch APPID  Launch game by app ID");
        println!("  -console          Open console");
        println!("  -dev              Developer mode");
        println!("  -login USER PASS  Login");
        println!("  -shutdown          Shutdown Steam");
        println!("  -silent           Start minimized");
        println!("  -tcp              Use TCP");
        println!("  -bigpicture       Big Picture mode");
        println!("  --reset           Reset Steam");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Steam client version 1707267757 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "-shutdown") {
        println!("Steam: shutting down...");
    } else if args.iter().any(|a| a == "-applaunch") {
        let appid = args.windows(2).find(|w| w[0] == "-applaunch").map(|w| w[1].as_str()).unwrap_or("0");
        println!("Steam: launching app {}", appid);
    } else {
        println!("Steam client starting...");
        println!("  Runtime: Proton 9.0-1");
        println!("  Library: 42 games installed");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "proton".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "steam" => run_steam(&rest),
        _ => run_proton(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_proton};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/proton"), "proton");
        assert_eq!(basename(r"C:\bin\proton.exe"), "proton.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("proton.exe"), "proton");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_proton(&["--help".to_string()]), 0);
        assert_eq!(run_proton(&["-h".to_string()]), 0);
        let _ = run_proton(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_proton(&[]);
    }
}
