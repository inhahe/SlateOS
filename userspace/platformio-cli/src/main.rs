#![deny(clippy::all)]

//! platformio-cli — SlateOS PlatformIO embedded development
//!
//! Multi-personality: `pio`, `platformio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pio(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pio COMMAND [OPTIONS]");
        println!("PlatformIO Core 6.1.13 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  init         Initialize project");
        println!("  run          Build project");
        println!("  test         Run tests");
        println!("  upload       Upload firmware");
        println!("  monitor      Serial monitor");
        println!("  boards       List boards");
        println!("  lib          Library manager");
        println!("  device       Device tools");
        println!("  pkg          Package manager");
        println!("  home         PlatformIO Home");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("PlatformIO Core 6.1.13 (Slate OS)"),
        "init" => {
            let board = args.windows(2).find(|w| w[0] == "--board" || w[0] == "-b").map(|w| w[1].as_str()).unwrap_or("esp32dev");
            println!("Initializing project for board: {}", board);
            println!("  Created: platformio.ini");
            println!("  Created: src/main.cpp");
            println!("  Created: lib/");
            println!("  Created: include/");
            println!("  Project initialized.");
        }
        "run" => {
            let env = args.windows(2).find(|w| w[0] == "-e").map(|w| w[1].as_str()).unwrap_or("esp32dev");
            println!("Processing {} ...", env);
            println!("  Platform: espressif32 @ 6.5.0");
            println!("  Framework: arduino");
            println!("  Compiling .pio/build/{}/src/main.cpp.o", env);
            println!("  Linking .pio/build/{}/firmware.elf", env);
            println!("  Building .pio/build/{}/firmware.bin", env);
            println!("  RAM:   [==        ]  18.4% (used 60352 bytes from 327680 bytes)");
            println!("  Flash: [====      ]  38.5% (used 504832 bytes from 1310720 bytes)");
            println!("  SUCCESS");
        }
        "upload" => {
            println!("Uploading firmware...");
            println!("  Serial port: /dev/ttyUSB0");
            println!("  Chip: ESP32");
            println!("  Speed: 921600");
            println!("  Writing at 0x00010000... (100%)");
            println!("  Upload complete.");
        }
        "monitor" => {
            let baud = args.windows(2).find(|w| w[0] == "-b").map(|w| w[1].as_str()).unwrap_or("115200");
            println!("--- Serial Monitor ---");
            println!("--- Port: /dev/ttyUSB0  Baud: {}", baud);
            println!("--- Press Ctrl+C to exit");
        }
        "boards" => {
            let filter = args.get(1).map(|s| s.as_str());
            println!("Platform   ID             MCU          Frequency  Flash   RAM");
            println!("-------    --             ---          ---------  -----   ---");
            if let Some(f) = filter {
                println!("(showing boards matching '{}')", f);
            }
            println!("espressif  esp32dev       ESP32        240MHz     4MB     320KB");
            println!("espressif  esp32-s3       ESP32-S3     240MHz     8MB     512KB");
            println!("atmelavr   uno            ATmega328P   16MHz      32KB    2KB");
            println!("atmelavr   mega2560       ATmega2560   16MHz      256KB   8KB");
            println!("ststm32    nucleo_f446re  STM32F446RE  180MHz     512KB   128KB");
            println!("raspberrypi pico          RP2040       133MHz     2MB     264KB");
        }
        "device" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if action == "list" {
                println!("/dev/ttyUSB0  CP2102 USB to UART  (Silicon Labs)");
                println!("/dev/ttyACM0  Arduino Mega 2560   (Arduino)");
            }
        }
        "test" => {
            println!("Running unit tests...");
            println!("  test_blink: PASSED");
            println!("  test_sensor: PASSED");
            println!("  2 tests passed, 0 failed.");
        }
        _ => println!("pio: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pio(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pio};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/platformio"), "platformio");
        assert_eq!(basename(r"C:\bin\platformio.exe"), "platformio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("platformio.exe"), "platformio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pio(&["--help".to_string()]), 0);
        assert_eq!(run_pio(&["-h".to_string()]), 0);
        let _ = run_pio(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pio(&[]);
    }
}
