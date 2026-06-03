#![deny(clippy::all)]

//! openocd-cli — OurOS on-chip debugger
//!
//! Multi-personality: `openocd`, `st-flash`, `st-info`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_openocd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: openocd [OPTIONS]");
        println!();
        println!("openocd — Open On-Chip Debugger (OurOS).");
        println!();
        println!("Options:");
        println!("  -f FILE         Use configuration file");
        println!("  -s DIR          Search directory for config files");
        println!("  -c CMD          Execute command");
        println!("  -d [N]          Debug level (0-4)");
        println!("  -l FILE         Log to file");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Open On-Chip Debugger 0.12.0 (OurOS)");
        return 0;
    }

    let config = args.windows(2).find(|w| w[0] == "-f").map(|w| w[1].as_str());
    println!("Open On-Chip Debugger 0.12.0");
    println!("Licensed under GNU GPL v2");
    if let Some(cfg) = config {
        println!("Info : using config file: {}", cfg);
    }
    println!("Info : Listening on port 3333 for gdb connections");
    println!("Info : Listening on port 4444 for telnet connections");
    println!("Info : clock speed 2000 kHz");
    println!("Info : SWD DPIDR 0x0bc11477");
    println!("Info : [stm32f4x.cpu] Cortex-M4 r0p1 processor detected");
    println!("Info : [stm32f4x.cpu] target voltage: 3.300000V");
    println!("Info : starting gdb server for stm32f4x.cpu on port 3333");
    0
}

fn run_st_flash(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: st-flash [OPTIONS] COMMAND [FILE] [ADDR] [SIZE]");
        println!();
        println!("st-flash — STMicroelectronics flash tool (OurOS).");
        println!();
        println!("Commands: read, write, erase, reset");
        println!();
        println!("Options:");
        println!("  --reset          Reset after flash");
        println!("  --format FORMAT  ihex or binary");
        println!("  --serial SERIAL  Device serial");
        println!("  --freq FREQ      SWD frequency");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("write");
    let file = args.get(1).map(|s| s.as_str()).unwrap_or("firmware.bin");

    println!("st-flash 1.8.0 (OurOS)");
    match cmd {
        "write" => {
            println!("Flash page at addr: 0x08000000 erased");
            println!("Attempting to write {} to stm32 address: 0x08000000", file);
            println!("  Flash page at addr: 0x08000000 of size: 16384");
            println!("  Flash page at addr: 0x08004000 of size: 16384");
            println!("  Wrote 32768 bytes (2 pages) in 0.50s");
        }
        "erase" => {
            println!("Mass erasing flash memory");
            println!("Flash chip erased");
        }
        "read" => {
            println!("Reading {} bytes from 0x08000000 to {}", 32768, file);
        }
        "reset" => {
            println!("Resetting device...");
        }
        _ => println!("st-flash: unknown command '{}'", cmd),
    }
    0
}

fn run_st_info(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: st-info [OPTIONS]");
        println!("Options: --probe, --serial, --flash, --sram, --chipid, --descr");
        return 0;
    }

    let probe = args.iter().any(|a| a == "--probe");
    if probe {
        println!("Found 1 stlink programmers");
        println!(" serial: 303030303030303030303031");
        println!(" hla-serial: \"\\x30\\x30\\x30\\x30\\x30\\x30\\x30\\x30\\x30\\x30\\x30\\x31\"");
        println!(" flash: 524288 (pagesize: 16384)");
        println!(" sram: 131072");
        println!(" chipid: 0x0413");
        println!(" descr: F4xx");
    } else {
        if args.iter().any(|a| a == "--flash") { println!("524288"); }
        if args.iter().any(|a| a == "--sram") { println!("131072"); }
        if args.iter().any(|a| a == "--chipid") { println!("0x0413"); }
        if args.iter().any(|a| a == "--descr") { println!("F4xx"); }
        if args.iter().any(|a| a == "--serial") { println!("303030303030303030303031"); }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "openocd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "st-flash" => run_st_flash(&rest),
        "st-info" => run_st_info(&rest),
        _ => run_openocd(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_openocd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/openocd"), "openocd");
        assert_eq!(basename(r"C:\bin\openocd.exe"), "openocd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("openocd.exe"), "openocd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_openocd(&["--help".to_string()]), 0);
        assert_eq!(run_openocd(&["-h".to_string()]), 0);
        assert_eq!(run_openocd(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_openocd(&[]), 0);
    }
}
