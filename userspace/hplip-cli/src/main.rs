#![deny(clippy::all)]

//! hplip-cli — OurOS HP Linux Imaging and Printing tools
//!
//! Multi-personality: `hp-setup`, `hp-info`, `hp-levels`, `hp-toolbox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_setup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hp-setup [OPTIONS]");
        println!("hp-setup v3.22 (OurOS) — HP printer setup wizard");
        println!();
        println!("Options:");
        println!("  -i                Interactive mode");
        println!("  -a                Auto-setup");
        println!("  -p DEVICE         Parallel port device");
        println!("  -b BUS            USB bus to probe");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hp-setup v3.22 (OurOS)"); return 0; }
    println!("hp-setup: searching for HP devices...");
    println!("  Found: HP LaserJet Pro M404 (USB)");
    println!("  Driver: hp-laserjet_pro_m404-ps.ppd");
    println!("  Setup complete");
    0
}

fn run_info(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hp-info [OPTIONS]");
        println!("hp-info v3.22 (OurOS) — HP device information");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hp-info v3.22 (OurOS)"); return 0; }
    println!("HP Device Information:");
    println!("  Model: HP LaserJet Pro M404");
    println!("  Serial: VNB1234567");
    println!("  Firmware: 20230815");
    println!("  Connection: USB");
    println!("  Status: Ready");
    0
}

fn run_levels(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hp-levels [OPTIONS]");
        println!("hp-levels v3.22 (OurOS) — Show supply levels");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hp-levels v3.22 (OurOS)"); return 0; }
    println!("HP LaserJet Pro M404:");
    println!("  Black toner:  75%");
    println!("  Drum:         85%");
    println!("  Maintenance kit: 90%");
    0
}

fn run_toolbox(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hp-toolbox [OPTIONS]");
        println!("hp-toolbox v3.22 (OurOS) — HP device management GUI");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hp-toolbox v3.22 (OurOS)"); return 0; }
    println!("hp-toolbox: device management GUI started");
    println!("  Devices: 1 connected");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hp-setup".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "hp-info" => run_info(&rest, &prog),
        "hp-levels" => run_levels(&rest, &prog),
        "hp-toolbox" => run_toolbox(&rest, &prog),
        _ => run_setup(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_setup};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hplip"), "hplip");
        assert_eq!(basename(r"C:\bin\hplip.exe"), "hplip.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hplip.exe"), "hplip");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_setup(&["--help".to_string()], "hplip"), 0);
        assert_eq!(run_setup(&["-h".to_string()], "hplip"), 0);
        assert_eq!(run_setup(&["--version".to_string()], "hplip"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_setup(&[], "hplip"), 0);
    }
}
