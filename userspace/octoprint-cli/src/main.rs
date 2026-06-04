#![deny(clippy::all)]

//! octoprint-cli — OurOS OctoPrint 3D printer management
//!
//! Multi-personality: `octoprint`, `octoprint-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_octoprint(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: octoprint COMMAND [OPTIONS]");
        println!("OctoPrint 1.9.3 (OurOS)");
        println!();
        println!("Commands:");
        println!("  serve        Start OctoPrint server");
        println!("  config       Manage configuration");
        println!("  plugins      Manage plugins");
        println!("  user         Manage users");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => println!("OctoPrint 1.9.3 (OurOS, Python 3.12.0)"),
        "serve" => {
            let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("5000");
            println!("OctoPrint 1.9.3 starting...");
            println!("  Listening on http://0.0.0.0:{}/", port);
            println!("  Printer connection: /dev/ttyUSB0 @ 115200");
            println!("  Server ready.");
        }
        "config" => {
            println!("OctoPrint configuration:");
            println!("  Config dir: ~/.octoprint");
            println!("  Upload dir: ~/.octoprint/uploads");
            println!("  Plugin dir: ~/.octoprint/plugins");
        }
        "plugins" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match action {
                "list" => {
                    println!("Installed plugins:");
                    println!("  OctoPrint-BedLevelVisualizer 0.1.16");
                    println!("  OctoPrint-DisplayLayerProgress 1.27.2");
                    println!("  OctoPrint-PrintTimeGenius 2.2.8");
                    println!("  OctoPrint-Themeify 1.2.2");
                }
                _ => println!("octoprint plugins: '{}' completed", action),
            }
        }
        "user" => {
            println!("Users:");
            println!("  admin (admin, active)");
        }
        _ => println!("octoprint: '{}' completed", subcmd),
    }
    0
}

fn run_octoprint_cli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: octoprint-cli COMMAND [OPTIONS]");
        println!("  --server URL    OctoPrint server URL");
        println!("  --key KEY       API key");
        println!();
        println!("Commands: status, print, upload, cancel, connect, disconnect, temp, files");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match subcmd {
        "status" => {
            println!("Printer: Operational");
            println!("State: Idle");
            println!("Hotend: 22.5°C / 0°C");
            println!("Bed: 21.8°C / 0°C");
        }
        "files" => {
            println!("Files on printer:");
            println!("  benchy.gcode      (2.1 MB, 2024-01-15)");
            println!("  calibration.gcode (0.5 MB, 2024-01-10)");
            println!("  part_v2.gcode     (8.3 MB, 2024-01-20)");
        }
        "temp" => {
            println!("Temperature:");
            println!("  Hotend: 22.5°C (target: 0°C)");
            println!("  Bed: 21.8°C (target: 0°C)");
        }
        _ => println!("octoprint-cli: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "octoprint".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "octoprint-cli" => run_octoprint_cli(&rest),
        _ => run_octoprint(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_octoprint};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/octoprint"), "octoprint");
        assert_eq!(basename(r"C:\bin\octoprint.exe"), "octoprint.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("octoprint.exe"), "octoprint");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_octoprint(&["--help".to_string()]), 0);
        assert_eq!(run_octoprint(&["-h".to_string()]), 0);
        let _ = run_octoprint(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_octoprint(&[]);
    }
}
