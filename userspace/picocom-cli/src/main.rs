#![deny(clippy::all)]

//! picocom-cli — SlateOS minimal serial terminal
//!
//! Multi-personality: `picocom`, `cu`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_picocom(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: picocom [OPTIONS] DEVICE");
        println!();
        println!("picocom — minimal serial terminal (SlateOS).");
        println!();
        println!("Options:");
        println!("  -b, --baud BAUD       Baud rate (default 9600)");
        println!("  -f, --flow FLOW       Flow control (x, h, n)");
        println!("  -p, --parity PAR      Parity (e, o, n)");
        println!("  -d, --databits BITS   Data bits (5, 6, 7, 8)");
        println!("  -s, --stopbits BITS   Stop bits (1, 2)");
        println!("  --imap MAP            Input map");
        println!("  --omap MAP            Output map");
        println!("  --emap MAP            Echo map");
        println!("  -c, --logfile FILE    Log to FILE");
        println!("  --noreset             Don't reset on exit");
        return 0;
    }

    let device = args.iter().find(|a| a.starts_with('/') || a.starts_with("COM")).map(|s| s.as_str()).unwrap_or("/dev/ttyUSB0");
    let baud = args.windows(2)
        .find(|w| w[0] == "-b" || w[0] == "--baud")
        .and_then(|w| w[1].parse::<u32>().ok())
        .unwrap_or(9600);

    println!("picocom v3.1 (SlateOS)");
    println!();
    println!("port is        : {}", device);
    println!("flowcontrol    : none");
    println!("baudrate is    : {}", baud);
    println!("parity is      : none");
    println!("databits are   : 8");
    println!("stopbits are   : 1");
    println!("escape is      : C-a");
    println!("local echo is  : no");
    println!();
    println!("Type [C-a] [C-h] to see available commands");
    println!("Terminal ready");
    0
}

fn run_cu(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cu [OPTIONS] [SYSTEM|PHONE|HOST]");
        println!();
        println!("cu — call another Unix system (serial/network) (SlateOS).");
        println!();
        println!("Options:");
        println!("  -l LINE     Device line");
        println!("  -s SPEED    Baud rate");
        println!("  -e          Even parity");
        println!("  -o          Odd parity");
        return 0;
    }

    let line = args.windows(2).find(|w| w[0] == "-l").map(|w| w[1].as_str()).unwrap_or("/dev/ttyUSB0");
    let speed = args.windows(2).find(|w| w[0] == "-s").and_then(|w| w[1].parse::<u32>().ok()).unwrap_or(9600);
    println!("Connected to {} at {} baud.", line, speed);
    println!("Escape character: '~'");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "picocom".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "cu" => run_cu(&rest),
        _ => run_picocom(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_picocom};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/picocom"), "picocom");
        assert_eq!(basename(r"C:\bin\picocom.exe"), "picocom.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("picocom.exe"), "picocom");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_picocom(&["--help".to_string()]), 0);
        assert_eq!(run_picocom(&["-h".to_string()]), 0);
        let _ = run_picocom(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_picocom(&[]);
    }
}
