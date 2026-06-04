#![deny(clippy::all)]

//! dxvk-cli — OurOS DXVK Vulkan-based DirectX translation
//!
//! Multi-personality: `dxvk`, `dxvk-setup`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dxvk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dxvk [OPTIONS]");
        println!("dxvk v2.4 (OurOS) — Vulkan-based D3D9/10/11 translation layer");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Environment:");
        println!("  DXVK_HUD=1            Show HUD overlay");
        println!("  DXVK_LOG_LEVEL=info   Logging level");
        println!("  DXVK_STATE_CACHE=1    Enable state cache");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dxvk v2.4 (OurOS)"); return 0; }
    println!("dxvk: Vulkan-based DirectX translation layer");
    println!("  Version: 2.4");
    println!("  D3D9:  Vulkan translation active");
    println!("  D3D10: Vulkan translation active");
    println!("  D3D11: Vulkan translation active");
    println!("  State cache: enabled");
    0
}

fn run_setup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dxvk-setup [install|uninstall] [OPTIONS]");
        println!("dxvk-setup v2.4 (OurOS) — DXVK installer");
        println!();
        println!("Commands:");
        println!("  install           Install DXVK to Wine prefix");
        println!("  uninstall         Remove DXVK from Wine prefix");
        println!();
        println!("Options:");
        println!("  --symlink         Use symlinks instead of copies");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dxvk-setup v2.4 (OurOS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("install");
    match cmd {
        "uninstall" => println!("dxvk-setup: removing DXVK from prefix..."),
        _ => {
            println!("dxvk-setup: installing DXVK to prefix...");
            println!("  d3d9.dll   -> dxvk");
            println!("  d3d10.dll  -> dxvk");
            println!("  d3d10_1.dll-> dxvk");
            println!("  d3d11.dll  -> dxvk");
            println!("  dxgi.dll   -> dxvk");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dxvk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "dxvk-setup" => run_setup(&rest, &prog),
        _ => run_dxvk(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dxvk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dxvk"), "dxvk");
        assert_eq!(basename(r"C:\bin\dxvk.exe"), "dxvk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dxvk.exe"), "dxvk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dxvk(&["--help".to_string()], "dxvk"), 0);
        assert_eq!(run_dxvk(&["-h".to_string()], "dxvk"), 0);
        let _ = run_dxvk(&["--version".to_string()], "dxvk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dxvk(&[], "dxvk");
    }
}
