#![deny(clippy::all)]

//! parallels-cli — OurOS Parallels Desktop for Mac
//!
//! Single personality: `parallels`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: parallels [OPTIONS]");
        println!("Parallels Desktop 20 for Mac (OurOS) — macOS-host virtualization");
        println!();
        println!("Options:");
        println!("  --new                  Create new VM");
        println!("  --windows              Install Windows 11 (downloads ARM image)");
        println!("  --coherence            Coherence Mode (apps overlay on macOS desktop)");
        println!("  --ras                  Parallels Remote Application Server (RAS)");
        println!("  --pro                  Parallels Desktop Pro");
        println!("  --business             Parallels Desktop Business");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Parallels Desktop 20.1.2 (55742) (OurOS)"); return 0; }
    println!("Parallels Desktop 20.1.2 (55742) (OurOS)");
    println!("  Vendor: Alludo (acquired Corel parent of Parallels Dec 2022)");
    println!("  Founded: 1999 by Serguei Beloussov (also Acronis founder) in Singapore");
    println!("  Sold to: Corel 2018, then Alludo (formerly Corel rebrand) 2022");
    println!("  Engine: Intel: hardware HVM (KVM-derived); Apple Silicon: Hypervisor.framework");
    println!("  Apple Silicon: only solution to run Windows on ARM Macs (officially licensed");
    println!("                 by Microsoft for Windows 11 ARM since 2023)");
    println!("  Editions: Standard ($99.99/yr), Pro ($119.99/yr), Business ($149.99/yr)");
    println!("  Coherence: VM windows mixed with macOS windows (most polished of any hypervisor)");
    println!("  Features: x86 emulation on M-series (slow, limited use), Touch Bar passthrough,");
    println!("            macOS guest support (for Apple-on-Apple test labs)");
    println!("  Performance: highly optimized — outperforms VMware Fusion on macOS by ~20%");
    println!("  Mobile companion: Parallels Access (iOS/Android remote into Mac/PC)");
    println!("  Sister product: Parallels Toolbox (utility collection), Desktop for ChromeOS");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "parallels".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pl(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/parallels"), "parallels");
        assert_eq!(basename(r"C:\bin\parallels.exe"), "parallels.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("parallels.exe"), "parallels");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pl(&["--help".to_string()], "parallels"), 0);
        assert_eq!(run_pl(&["-h".to_string()], "parallels"), 0);
        let _ = run_pl(&["--version".to_string()], "parallels");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pl(&[], "parallels");
    }
}
