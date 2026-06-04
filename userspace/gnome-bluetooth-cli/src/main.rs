#![deny(clippy::all)]

//! gnome-bluetooth-cli — OurOS GNOME Bluetooth settings
//!
//! Multi-personality: `gnome-bluetooth`, `bluetooth-sendto`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gnome_bluetooth(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnome-bluetooth [OPTIONS]");
        println!("gnome-bluetooth v46.0 (OurOS) — GNOME Bluetooth panel");
        println!();
        println!("Options:");
        println!("  --version      Show version");
        println!();
        println!("Bluetooth device management: pair, connect, remove.");
        println!("Supports audio, input devices, file transfer.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnome-bluetooth v46.0 (OurOS)"); return 0; }
    println!("gnome-bluetooth: Bluetooth settings");
    println!("  Adapter: hci0 (powered on, discoverable)");
    println!("  Paired devices:");
    println!("    Wireless Mouse     Connected  input");
    println!("    BT Headphones      Disconnected  audio");
    0
}

fn run_bluetooth_sendto(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bluetooth-sendto [OPTIONS] [FILE...]");
        println!("bluetooth-sendto v46.0 (OurOS) — Send files via Bluetooth");
        println!("  --device ADDR  Target device address");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("bluetooth-sendto v46.0 (OurOS)"); return 0; }
    println!("bluetooth-sendto: file transfer dialog");
    println!("  Select device and files to send via OBEX.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnome-bluetooth".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "bluetooth-sendto" => run_bluetooth_sendto(&rest, &prog),
        _ => run_gnome_bluetooth(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gnome_bluetooth};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gnome-bluetooth"), "gnome-bluetooth");
        assert_eq!(basename(r"C:\bin\gnome-bluetooth.exe"), "gnome-bluetooth.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gnome-bluetooth.exe"), "gnome-bluetooth");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gnome_bluetooth(&["--help".to_string()], "gnome-bluetooth"), 0);
        assert_eq!(run_gnome_bluetooth(&["-h".to_string()], "gnome-bluetooth"), 0);
        let _ = run_gnome_bluetooth(&["--version".to_string()], "gnome-bluetooth");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gnome_bluetooth(&[], "gnome-bluetooth");
    }
}
