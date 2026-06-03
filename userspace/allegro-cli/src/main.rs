#![deny(clippy::all)]

//! allegro-cli — OurOS Cadence Allegro high-speed PCB design
//!
//! Single personality: `allegro`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_allegro(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: allegro [OPTIONS] [FILE]");
        println!("Cadence Allegro X 23.1 (OurOS) — High-speed PCB design");
        println!();
        println!("Options:");
        println!("  -nograph FILE          Run script on board without GUI");
        println!("  -s SCRIPT              SKILL script");
        println!("  --pcb-designer         Allegro PCB Designer");
        println!("  --sigrity              Sigrity SI/PI analysis");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cadence Allegro X 23.1 (OurOS)"); return 0; }
    println!("Cadence Allegro X 23.1 (OurOS)");
    println!("  Industry-leading high-speed PCB design (servers, telecom, ASIC interposers)");
    println!("  Constraint Manager: rule-driven design for SI/PI/EMI");
    println!("  Routing: ActiHS auto, dynamic differential pairs, flex/rigid-flex");
    println!("  Sigrity: signal/power integrity, EMI, thermal");
    println!("  Format: .brd (Allegro), .mcm (Multi-Chip Module), IPC-2581, ODB++");
    println!("  Scripting: SKILL (Cadence Lisp), Tcl/Tk, Python");
    println!("  Integration: Virtuoso (custom IC), OrCAD Capture (schematic)");
    println!("  License: enterprise (very expensive)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "allegro".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_allegro(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_allegro};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/allegro"), "allegro");
        assert_eq!(basename(r"C:\bin\allegro.exe"), "allegro.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("allegro.exe"), "allegro");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_allegro(&["--help".to_string()], "allegro"), 0);
        assert_eq!(run_allegro(&["-h".to_string()], "allegro"), 0);
        assert_eq!(run_allegro(&["--version".to_string()], "allegro"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_allegro(&[], "allegro"), 0);
    }
}
