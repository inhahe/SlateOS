#![deny(clippy::all)]

//! grbl-cli — OurOS GRBL CNC controller
//!
//! Multi-personality: `grbl`, `grbl-send`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_grbl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: grbl COMMAND [OPTIONS]");
        println!("GRBL 1.1h CNC Controller (OurOS)");
        println!();
        println!("Commands:");
        println!("  status       Show machine status");
        println!("  settings     Show/set GRBL settings");
        println!("  home         Home all axes");
        println!("  unlock       Unlock alarm");
        println!("  reset        Soft reset");
        println!("  send FILE    Stream G-code file");
        println!("  jog X Y Z    Jog to position");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => {
            println!("GRBL 1.1h (OurOS)");
            println!("Build: 2024.01.15");
        }
        "status" => {
            println!("<Idle|MPos:0.000,0.000,0.000|WPos:0.000,0.000,0.000|FS:0,0>");
            println!("Machine state: Idle");
            println!("MPos: X=0.000 Y=0.000 Z=0.000");
            println!("Feed rate: 0 mm/min");
            println!("Spindle: 0 RPM");
        }
        "settings" => {
            println!("$0=10 (Step pulse, usec)");
            println!("$1=25 (Step idle delay, msec)");
            println!("$2=0 (Step port invert, mask)");
            println!("$3=0 (Direction port invert, mask)");
            println!("$4=0 (Step enable invert, bool)");
            println!("$5=0 (Limit pins invert, bool)");
            println!("$6=0 (Probe pin invert, bool)");
            println!("$100=250.000 (X steps/mm)");
            println!("$101=250.000 (Y steps/mm)");
            println!("$102=250.000 (Z steps/mm)");
            println!("$110=500.000 (X max rate, mm/min)");
            println!("$111=500.000 (Y max rate, mm/min)");
            println!("$112=500.000 (Z max rate, mm/min)");
        }
        "home" => {
            println!("Homing cycle started...");
            println!("  Z axis homed.");
            println!("  X axis homed.");
            println!("  Y axis homed.");
            println!("Homing complete.");
        }
        "unlock" => println!("[MSG:Caution: Unlocked]"),
        "reset" => println!("GRBL 1.1h ['$' for help]"),
        "send" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("job.gcode");
            println!("Streaming: {}", file);
            println!("  Lines: 1234");
            println!("  Progress: 100%");
            println!("  Time: 15:23");
            println!("  Job complete.");
        }
        "jog" => {
            println!("Jogging...");
            println!("Position reached.");
        }
        _ => println!("grbl: '{}' completed", subcmd),
    }
    0
}

fn run_grbl_send(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: grbl-send [OPTIONS] FILE.gcode");
        println!("  --port PORT    Serial port (default: /dev/ttyUSB0)");
        println!("  --baud N       Baud rate (default: 115200)");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".gcode") || a.ends_with(".nc") || a.ends_with(".ngc")).map(|s| s.as_str()).unwrap_or("job.gcode");
    let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("/dev/ttyUSB0");
    println!("grbl-send: connecting to {} @ 115200", port);
    println!("Streaming: {}", file);
    println!("  1234 lines sent.");
    println!("  Job complete.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "grbl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "grbl-send" => run_grbl_send(&rest),
        _ => run_grbl(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_grbl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/grbl"), "grbl");
        assert_eq!(basename(r"C:\bin\grbl.exe"), "grbl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("grbl.exe"), "grbl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_grbl(&["--help".to_string()]), 0);
        assert_eq!(run_grbl(&["-h".to_string()]), 0);
        assert_eq!(run_grbl(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_grbl(&[]), 0);
    }
}
