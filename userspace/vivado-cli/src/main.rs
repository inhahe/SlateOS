#![deny(clippy::all)]

//! vivado-cli — SlateOS AMD Xilinx Vivado FPGA toolchain
//!
//! Single personality: `vivado`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vivado(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vivado [OPTIONS] [PROJECT]");
        println!("AMD Xilinx Vivado 2024.1 (Slate OS) — FPGA/SoC design suite");
        println!();
        println!("Options:");
        println!("  -mode tcl              Interactive Tcl shell");
        println!("  -mode batch -source S  Batch with Tcl script");
        println!("  --hls                  Vitis HLS (C++ → RTL)");
        println!("  --ila                  Integrated Logic Analyzer");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("AMD Xilinx Vivado 2024.1 (Slate OS)"); return 0; }
    println!("AMD Xilinx Vivado 2024.1 (Slate OS)");
    println!("  Editions: ML Standard (free), ML Enterprise");
    println!("  Devices: Artix/Kintex/Virtex 7, UltraScale/UltraScale+, Versal ACAP,");
    println!("           Zynq-7000/UltraScale+ MPSoC/RFSoC, Spartan-7");
    println!("  Languages: VHDL, Verilog, SystemVerilog, block design, IP Integrator");
    println!("  ML-based optimization: synthesis, P&R timing closure prediction");
    println!("  Vitis: unified software platform (HLS, embedded, accelerated apps)");
    println!("  PetaLinux: Yocto-based Linux for Zynq/Versal");
    println!("  Scripting: Tcl (extensive), Python via PYNQ for Zynq");
    println!("  License: Free (ML Standard) / per-seat (ML Enterprise)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vivado".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vivado(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vivado};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vivado"), "vivado");
        assert_eq!(basename(r"C:\bin\vivado.exe"), "vivado.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vivado.exe"), "vivado");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vivado(&["--help".to_string()], "vivado"), 0);
        assert_eq!(run_vivado(&["-h".to_string()], "vivado"), 0);
        let _ = run_vivado(&["--version".to_string()], "vivado");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vivado(&[], "vivado");
    }
}
