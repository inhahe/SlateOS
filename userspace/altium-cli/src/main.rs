#![deny(clippy::all)]

//! altium-cli — SlateOS Altium Designer PCB EDA
//!
//! Single personality: `altium`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_altium(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: altium [OPTIONS] [FILE]");
        println!("Altium Designer 24 (Slate OS) — Professional PCB design");
        println!();
        println!("Options:");
        println!("  -openPrj FILE          Open project (.PrjPCB)");
        println!("  -openSch FILE          Open schematic (.SchDoc)");
        println!("  -openPcb FILE          Open PCB (.PcbDoc)");
        println!("  --A365 SPACE           Connect to Altium 365 cloud workspace");
        println!("  --script SCRIPT        Run DelphiScript/JScript/VBScript");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Altium Designer 24.10.1 (Slate OS)"); return 0; }
    println!("Altium Designer 24.10.1 (Slate OS)");
    println!("  Workflow: Unified schematic + PCB + library + harness + multi-board");
    println!("  Format: .SchDoc/.PcbDoc/.PrjPCB native + IPC-2581, ODB++, Gerber X2");
    println!("  Routing: ActiveRoute (auto), Glossy routing, length tuning, differential pairs");
    println!("  Signal integrity: SI, power integrity (PDN analyzer), thermal");
    println!("  3D MCAD bridge: STEP export, SOLIDWORKS/Fusion 360/Creo");
    println!("  Cloud: Altium 365 — manufacturer parts, design sharing, MCAD CoDesign");
    println!("  Scripting: DelphiScript (primary), JScript, VBScript, C#");
    println!("  License: subscription (Designer/Pro/Enterprise/Concord)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "altium".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_altium(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_altium};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/altium"), "altium");
        assert_eq!(basename(r"C:\bin\altium.exe"), "altium.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("altium.exe"), "altium");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_altium(&["--help".to_string()], "altium"), 0);
        assert_eq!(run_altium(&["-h".to_string()], "altium"), 0);
        let _ = run_altium(&["--version".to_string()], "altium");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_altium(&[], "altium");
    }
}
