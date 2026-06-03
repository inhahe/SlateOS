#![deny(clippy::all)]

//! quartus-cli — OurOS Intel/Altera Quartus Prime FPGA toolchain
//!
//! Single personality: `quartus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_quartus(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: quartus [OPTIONS] [PROJECT]");
        println!("Intel Quartus Prime Pro 24.2 (OurOS) — Altera FPGA design suite");
        println!();
        println!("Options:");
        println!("  --shell                Quartus shell (Tcl interactive)");
        println!("  --map / --fit / --asm  Run specific compilation stage");
        println!("  --signaltap            Launch SignalTap II Logic Analyzer");
        println!("  --pgm                  Quartus Programmer (download bitstream)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Intel Quartus Prime Pro 24.2.0 (OurOS)"); return 0; }
    println!("Intel Quartus Prime Pro 24.2.0 (OurOS)");
    println!("  Editions: Lite (free), Standard, Pro (Stratix/Agilex)");
    println!("  Devices: Cyclone V/10, Arria 10, Stratix 10, Agilex 5/7/9, MAX 10");
    println!("  Languages: VHDL, Verilog, SystemVerilog, AHDL, schematic");
    println!("  Synthesis + place & route, timing analysis (TimeQuest/Quartus TA)");
    println!("  Platform Designer (formerly Qsys): SoC system integration");
    println!("  HLS: Intel HLS Compiler (C++ → RTL)");
    println!("  Scripting: Tcl (Quartus Shell), Python");
    println!("  License: Free (Lite) / subscription (Pro)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "quartus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_quartus(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_quartus};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/quartus"), "quartus");
        assert_eq!(basename(r"C:\bin\quartus.exe"), "quartus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("quartus.exe"), "quartus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_quartus(&["--help".to_string()], "quartus"), 0);
        assert_eq!(run_quartus(&["-h".to_string()], "quartus"), 0);
        assert_eq!(run_quartus(&["--version".to_string()], "quartus"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_quartus(&[], "quartus"), 0);
    }
}
