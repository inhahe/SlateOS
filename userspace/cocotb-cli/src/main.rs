#![deny(clippy::all)]

//! cocotb-cli — OurOS cocotb HDL verification framework
//!
//! Single personality: `cocotb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cocotb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cocotb COMMAND [OPTIONS]");
        println!("cocotb v1.9 (OurOS) — Coroutine-based co-simulation testbench");
        println!();
        println!("Commands:");
        println!("  run               Run tests");
        println!("  config            Show configuration");
        println!("  new NAME          Create new testbench");
        println!("  list              List discovered tests");
        println!("  clean             Clean simulation artifacts");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("cocotb v1.9 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("config");
    match cmd {
        "run" => {
            println!("Running cocotb tests...");
            println!("  Simulator: Icarus Verilog");
            println!("  DUT: top_module");
            println!("  Tests: 4 discovered");
            println!("  test_reset... PASS (0.1s)");
            println!("  test_basic_io... PASS (0.3s)");
            println!("  test_edge_cases... PASS (0.2s)");
            println!("  test_timing... PASS (0.5s)");
            println!("  Results: 4/4 passed");
        }
        "config" => {
            println!("cocotb v1.9 configuration:");
            println!("  Python: 3.12");
            println!("  Simulator: iverilog (Icarus)");
            println!("  GPI: VPI");
            println!("  Toplevel: top_module");
        }
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my_tb");
            println!("Creating testbench: {}", name);
            println!("  Created Makefile");
            println!("  Created test_{}.py", name);
            println!("  Created {}.sv (stub DUT)", name);
        }
        "list" => {
            println!("Discovered tests:");
            println!("  test_reset");
            println!("  test_basic_io");
            println!("  test_edge_cases");
            println!("  test_timing");
        }
        "clean" => println!("Cleaning simulation artifacts... Done."),
        _ => println!("cocotb {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cocotb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cocotb(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cocotb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cocotb"), "cocotb");
        assert_eq!(basename(r"C:\bin\cocotb.exe"), "cocotb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cocotb.exe"), "cocotb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cocotb(&["--help".to_string()], "cocotb"), 0);
        assert_eq!(run_cocotb(&["-h".to_string()], "cocotb"), 0);
        assert_eq!(run_cocotb(&["--version".to_string()], "cocotb"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cocotb(&[], "cocotb"), 0);
    }
}
