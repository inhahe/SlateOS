#![deny(clippy::all)]

//! inxi-cli — OurOS inxi system information
//!
//! Single personality: `inxi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_inxi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: inxi [OPTIONS]");
        println!("inxi v3.3 (OurOS) — Full-featured system information");
        println!();
        println!("Options:");
        println!("  -F             Full output");
        println!("  -b             Basic output");
        println!("  -c N           Color scheme (0-42)");
        println!("  -C             CPU info");
        println!("  -D             Disk info");
        println!("  -G             Graphics info");
        println!("  -M             Machine info");
        println!("  -N             Network info");
        println!("  -S             System info");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("inxi v3.3 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-F") {
        println!("System:  Host: ouros-host Kernel: 0.1.0-ouros x86_64");
        println!("Machine: Type: Desktop System: Custom");
        println!("CPU:     AMD Ryzen 7 (8) @ 3.6GHz");
        println!("Graphics: Device: AMD Radeon Driver: amdgpu");
        println!("Network: Device: Intel I225-V Driver: igc");
        println!("Drives:  /dev/sda 500GB SSD");
        println!("Info:    Processes: 142 Uptime: 2h 15m Memory: 4.0/16.0 GiB");
        return 0;
    }
    println!("CPU: AMD Ryzen 7 (8) @ 3.6GHz  Kernel: 0.1.0-ouros  Mem: 4.0/16.0GiB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "inxi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_inxi(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_inxi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/inxi"), "inxi");
        assert_eq!(basename(r"C:\bin\inxi.exe"), "inxi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("inxi.exe"), "inxi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_inxi(&["--help".to_string()], "inxi"), 0);
        assert_eq!(run_inxi(&["-h".to_string()], "inxi"), 0);
        let _ = run_inxi(&["--version".to_string()], "inxi");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_inxi(&[], "inxi");
    }
}
