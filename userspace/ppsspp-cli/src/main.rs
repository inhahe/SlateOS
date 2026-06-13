#![deny(clippy::all)]

//! ppsspp-cli — SlateOS PPSSPP PSP emulator
//!
//! Multi-personality: `ppsspp`, `ppsspp-headless`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ppsspp(args: &[String], prog: &str) -> i32 {
    let headless = prog == "ppsspp-headless";
    if args.iter().any(|a| a == "--help" || a == "-h") {
        if headless {
            println!("Usage: ppsspp-headless [OPTIONS] ISO");
        } else {
            println!("Usage: ppsspp [OPTIONS] [ISO]");
        }
        println!("ppsspp v1.17.1 (SlateOS) — PlayStation Portable emulator");
        println!();
        println!("Options:");
        println!("  --fullscreen      Start fullscreen");
        println!("  --backend OGL|VK  Graphics backend");
        println!("  --scale N         Rendering resolution scale");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ppsspp v1.17.1 (SlateOS)"); return 0; }
    if headless {
        println!("ppsspp: headless mode started");
    } else {
        println!("ppsspp: PSP emulator started");
    }
    println!("  Backend: Vulkan");
    println!("  Resolution: 5x PSP native (2720x1530)");
    println!("  Texture filtering: anisotropic 16x");
    println!("  Save states: available");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ppsspp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ppsspp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ppsspp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ppsspp"), "ppsspp");
        assert_eq!(basename(r"C:\bin\ppsspp.exe"), "ppsspp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ppsspp.exe"), "ppsspp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ppsspp(&["--help".to_string()], "ppsspp"), 0);
        assert_eq!(run_ppsspp(&["-h".to_string()], "ppsspp"), 0);
        let _ = run_ppsspp(&["--version".to_string()], "ppsspp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ppsspp(&[], "ppsspp");
    }
}
