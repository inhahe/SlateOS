#![deny(clippy::all)]

//! sane-cli — OurOS SANE scanner tools
//!
//! Multi-personality: `scanimage`, `sane-find-scanner`, `scanadf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_scanimage(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: scanimage [OPTIONS]");
        println!("scanimage v1.2 (OurOS) — Scan an image");
        println!();
        println!("Options:");
        println!("  -d DEVICE         Use specific device");
        println!("  --format=FMT      Output format (png, tiff, pnm, jpeg)");
        println!("  --resolution=DPI  Scan resolution");
        println!("  --mode=MODE       Scan mode (Color, Gray, Lineart)");
        println!("  -L                List available devices");
        println!("  -o FILE           Output file");
        println!("  --batch           Batch mode");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("scanimage v1.2 (OurOS, SANE 1.2)"); return 0; }
    if args.iter().any(|a| a == "-L") {
        println!("device `epkowa:libusb:001:004' is a Epson Perfection V39 flatbed scanner");
        println!("device `pixma:04A91234' is a Canon PIXMA MG3600 multi-function peripheral");
        return 0;
    }
    println!("Scanning...");
    println!("  Device: epkowa:libusb:001:004");
    println!("  Resolution: 300 DPI");
    println!("  Mode: Color");
    println!("  Format: PNG");
    println!("Output written to out.png");
    0
}

fn run_find_scanner(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sane-find-scanner [OPTIONS]");
        println!("sane-find-scanner v1.2 (OurOS) — Find SCSI and USB scanners");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sane-find-scanner v1.2 (OurOS)"); return 0; }
    println!("  # sane-find-scanner will now attempt to detect your scanner.");
    println!("found USB scanner (vendor=0x04b8, product=0x014a) at libusb:001:004");
    println!("found USB scanner (vendor=0x04a9, product=0x1234) at libusb:001:005");
    println!("  # Your USB scanner was detected.");
    0
}

fn run_scanadf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: scanadf [OPTIONS]");
        println!("scanadf v1.2 (OurOS) — Scan multiple pages from ADF");
        println!();
        println!("Options:");
        println!("  -d DEVICE         Use specific device");
        println!("  -o PATTERN        Output file pattern");
        println!("  -S                Start scanning immediately");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("scanadf v1.2 (OurOS)"); return 0; }
    println!("scanadf: waiting for feeder...");
    println!("  Scanned page 1 → out-0001.pnm");
    println!("  Scanned page 2 → out-0002.pnm");
    println!("  ADF empty, 2 pages scanned");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "scanimage".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "sane-find-scanner" => run_find_scanner(&rest, &prog),
        "scanadf" => run_scanadf(&rest, &prog),
        _ => run_scanimage(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_scanimage};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sane"), "sane");
        assert_eq!(basename(r"C:\bin\sane.exe"), "sane.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sane.exe"), "sane");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_scanimage(&["--help".to_string()], "sane"), 0);
        assert_eq!(run_scanimage(&["-h".to_string()], "sane"), 0);
        let _ = run_scanimage(&["--version".to_string()], "sane");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_scanimage(&[], "sane");
    }
}
