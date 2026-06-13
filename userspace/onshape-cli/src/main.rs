#![deny(clippy::all)]

//! onshape-cli — SlateOS PTC Onshape cloud-native CAD
//!
//! Single personality: `onshape`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_onshape(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: onshape [OPTIONS] [DOC_URL]");
        println!("PTC Onshape (SlateOS) — Cloud-native parametric CAD");
        println!();
        println!("Options:");
        println!("  --document URL         Open Onshape document by URL");
        println!("  --workspace WS         Switch workspace/branch");
        println!("  --featurescript FILE   Run FeatureScript");
        println!("  --export FORMAT        Export (STEP/IGES/STL/Parasolid/DXF)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("PTC Onshape 1.193 (SlateOS)"); return 0; }
    println!("PTC Onshape 1.193 (SlateOS)");
    println!("  Architecture: Cloud-native, runs in browser — no install needed");
    println!("  Branching/merging: Git-like version control for CAD documents");
    println!("  Collaboration: Real-time multi-user editing");
    println!("  Scripting: FeatureScript (proprietary type-safe language)");
    println!("  Mobile: Full iOS/Android apps");
    println!("  Format: native cloud + STEP/IGES/Parasolid/STL/DXF/SOLIDWORKS import");
    println!("  License: Free for public/educational, subscription for private docs");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "onshape".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_onshape(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_onshape};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/onshape"), "onshape");
        assert_eq!(basename(r"C:\bin\onshape.exe"), "onshape.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("onshape.exe"), "onshape");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_onshape(&["--help".to_string()], "onshape"), 0);
        assert_eq!(run_onshape(&["-h".to_string()], "onshape"), 0);
        let _ = run_onshape(&["--version".to_string()], "onshape");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_onshape(&[], "onshape");
    }
}
