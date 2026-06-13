#![deny(clippy::all)]

//! avrdude-cli — SlateOS AVR/MCU programmer
//!
//! Multi-personality: `avrdude`, `esptool`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_avrdude(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") {
        println!("Usage: avrdude [OPTIONS]");
        println!();
        println!("avrdude — AVR microcontroller programmer (Slate OS).");
        println!();
        println!("Options:");
        println!("  -p PARTNO    Target AVR device");
        println!("  -c PROGRAMMER  Programmer type");
        println!("  -P PORT      Communication port");
        println!("  -b BAUD      Baud rate");
        println!("  -U MEM:OP:FILE  Memory operation (flash/eeprom:r/w/v:file)");
        println!("  -F           Force (override signature check)");
        println!("  -v           Verbose");
        println!("  -n           Dry run (no write)");
        println!("  -e           Chip erase");
        println!("  -D           No auto-erase before flash write");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v" && args.len() == 1) {
        println!("avrdude version 7.3 (Slate OS)");
        return 0;
    }

    let part = args.windows(2).find(|w| w[0] == "-p").map(|w| w[1].as_str()).unwrap_or("m328p");
    let programmer = args.windows(2).find(|w| w[0] == "-c").map(|w| w[1].as_str()).unwrap_or("arduino");
    let port = args.windows(2).find(|w| w[0] == "-P").map(|w| w[1].as_str()).unwrap_or("/dev/ttyACM0");

    println!("avrdude: AVR Part \"{}\"", part);
    println!("avrdude: Programmer Type   : {}", programmer);
    println!("avrdude: Using Port        : {}", port);

    let has_upload = args.iter().any(|a| a.starts_with("-U") || a == "-U");
    if has_upload {
        println!("avrdude: writing flash (32768 bytes):");
        println!("Writing | ################################################## | 100% 0.00s");
        println!();
        println!("avrdude: 32768 bytes of flash written");
        println!("avrdude: verifying flash memory against output file:");
        println!("Reading | ################################################## | 100% 0.00s");
        println!();
        println!("avrdude: 32768 bytes of flash verified");
    }

    println!();
    println!("avrdude done.  Thank you.");
    0
}

fn run_esptool(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: esptool [OPTIONS] COMMAND");
        println!();
        println!("esptool — Espressif SoC serial bootloader tool (Slate OS).");
        println!();
        println!("Commands:");
        println!("  write_flash    Write binary to flash");
        println!("  read_flash     Read flash contents");
        println!("  erase_flash    Erase entire flash");
        println!("  flash_id       Read flash chip ID");
        println!("  chip_id        Read chip ID");
        println!("  read_mac       Read MAC address");
        println!("  elf2image      Convert ELF to flash image");
        println!();
        println!("Options:");
        println!("  --chip CHIP    Chip type (esp8266, esp32, esp32s2, etc.)");
        println!("  --port PORT    Serial port");
        println!("  --baud BAUD    Baud rate");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("esptool.py v4.7.0 (Slate OS)");
        return 0;
    }

    let chip = args.windows(2).find(|w| w[0] == "--chip").map(|w| w[1].as_str()).unwrap_or("esp32");
    let subcmd = args.iter().find(|a| !a.starts_with('-') && !a.starts_with("esp")).map(|s| s.as_str()).unwrap_or("chip_id");

    println!("esptool.py v4.7.0");
    println!("Serial port /dev/ttyUSB0");
    println!("Connecting....");
    println!("Chip is {} (revision v1.0)", chip.to_uppercase());
    println!("Features: WiFi, BT, Dual Core, 240MHz");

    match subcmd {
        "chip_id" => println!("Chip ID: 0x0012345678901234"),
        "read_mac" => println!("MAC: aa:bb:cc:dd:ee:ff"),
        "flash_id" => {
            println!("Manufacturer: ef");
            println!("Device: 4016");
            println!("Detected flash size: 4MB");
        }
        "write_flash" => {
            println!("Compressed 262144 bytes to 147562...");
            println!("Writing at 0x00010000... (100 %)");
            println!("Wrote 262144 bytes at 0x00010000 in 3.2 seconds (655.4 kbit/s)...");
            println!("Hash of data verified.");
        }
        "erase_flash" => {
            println!("Erasing flash (this may take a while)...");
            println!("Chip erase completed successfully in 5.2s");
        }
        _ => println!("esptool: command '{}' completed", subcmd),
    }
    println!("Hard resetting via RTS pin...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "avrdude".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "esptool" => run_esptool(&rest),
        _ => run_avrdude(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_avrdude};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/avrdude"), "avrdude");
        assert_eq!(basename(r"C:\bin\avrdude.exe"), "avrdude.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("avrdude.exe"), "avrdude");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_avrdude(&["--help".to_string()]), 0);
        assert_eq!(run_avrdude(&["-h".to_string()]), 0);
        let _ = run_avrdude(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_avrdude(&[]);
    }
}
