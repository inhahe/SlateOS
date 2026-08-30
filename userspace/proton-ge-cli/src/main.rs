#![deny(clippy::all)]

//! proton-ge-cli — Slate OS Proton GE custom Proton build
//!
//! Single personality: `proton-ge`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_proton_ge(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: proton-ge [OPTIONS]");
        println!("proton-ge v8-26 (Slate OS) — GloriousEggroll custom Proton build");
        println!();
        println!("Options:");
        println!("  --list            List installed GE-Proton versions");
        println!("  --info            Show build information");
        println!("  --version         Show version");
        println!();
        println!("Features over stock Proton:");
        println!("  Wine-staging patches, DXVK/VKD3D-Proton, fsync, media codecs");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("proton-ge GE-Proton8-26 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--list") {
        println!("Installed GE-Proton versions:");
        println!("  GE-Proton8-26 (current)");
        println!("  GE-Proton8-25");
        println!("  GE-Proton8-24");
        println!("  GE-Proton8-21");
        return 0;
    }
    if args.iter().any(|a| a == "--info") {
        println!("GE-Proton8-26 build info:");
        println!("  Wine: wine-staging 9.0");
        println!("  DXVK: 2.4");
        println!("  VKD3D-Proton: 2.12");
        println!("  Sync: fsync (futex-based)");
        println!("  Media: gstreamer + FFmpeg codecs");
        println!("  Patches: 1200+ custom patches");
        return 0;
    }
    println!("proton-ge: GE-Proton8-26 compatibility tool");
    println!("  Use Steam > Properties > Compatibility to select");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "proton-ge".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_proton_ge(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_proton_ge};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/proton-ge"), "proton-ge");
        assert_eq!(basename(r"C:\bin\proton-ge.exe"), "proton-ge.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("proton-ge.exe"), "proton-ge");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_proton_ge(&["--help".to_string()], "proton-ge"), 0);
        assert_eq!(run_proton_ge(&["-h".to_string()], "proton-ge"), 0);
        let _ = run_proton_ge(&["--version".to_string()], "proton-ge");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_proton_ge(&[], "proton-ge");
    }
}
