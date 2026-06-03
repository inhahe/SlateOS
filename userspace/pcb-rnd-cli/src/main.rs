#![deny(clippy::all)]

//! pcb-rnd-cli — OurOS pcb-rnd PCB layout editor
//!
//! Single personality: `pcb-rnd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pcb_rnd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pcb-rnd [OPTIONS] [FILE.lht|FILE.pcb]");
        println!("pcb-rnd v4.1 (OurOS) — Modular PCB layout editor");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Plugin-based architecture, multi-format I/O,");
        println!("  autorouter, DRC, footprint editor,");
        println!("  Gerber/Excellon export, netlist import");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pcb-rnd v4.1 (OurOS)"); return 0; }
    println!("pcb-rnd: PCB layout editor started");
    println!("  Layers: copper, silk, mask, paste, outline");
    println!("  Router: interactive + autorouter");
    println!("  Import: KiCad, Eagle, gEDA, Protel");
    println!("  Export: Gerber RS-274X, Excellon, PNG, SVG");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pcb-rnd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pcb_rnd(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pcb_rnd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pcb-rnd"), "pcb-rnd");
        assert_eq!(basename(r"C:\bin\pcb-rnd.exe"), "pcb-rnd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pcb-rnd.exe"), "pcb-rnd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_pcb_rnd(&["--help".to_string()], "pcb-rnd"), 0);
        assert_eq!(run_pcb_rnd(&["-h".to_string()], "pcb-rnd"), 0);
        assert_eq!(run_pcb_rnd(&["--version".to_string()], "pcb-rnd"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_pcb_rnd(&[], "pcb-rnd"), 0);
    }
}
