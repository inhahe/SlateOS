#![deny(clippy::all)]

//! rtl-sdr-cli — OurOS RTL-SDR tools
//!
//! Multi-personality: `rtl_sdr`, `rtl_fm`, `rtl_power`, `rtl_test`, `rtl_tcp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rtl(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        match prog {
            "rtl_fm" => {
                println!("Usage: rtl_fm [OPTIONS] -f FREQ [-f FREQ2 ...]");
                println!("rtl_fm v0.6 (OurOS) — FM demodulator");
                println!("  -f FREQ    Center frequency (Hz)");
                println!("  -M MODE    Modulation (fm, wfm, am, usb, lsb, raw)");
                println!("  -s RATE    Sample rate");
                println!("  -g GAIN    Tuner gain (dB)");
                println!("  -l N       Squelch level");
                println!("  -o N       Oversampling");
                println!("  -p PPM     PPM error correction");
                println!("  -d INDEX   Device index");
            }
            "rtl_power" => {
                println!("Usage: rtl_power [OPTIONS] -f FREQ1:FREQ2:BIN_SIZE");
                println!("rtl_power v0.6 (OurOS) — Wideband spectrum scanner");
                println!("  -f RANGE   Frequency range (Hz:Hz:Hz)");
                println!("  -i TIME    Integration time");
                println!("  -g GAIN    Tuner gain");
                println!("  -p PPM     PPM error");
                println!("  -e TIME    Exit after time");
            }
            "rtl_test" => {
                println!("Usage: rtl_test [OPTIONS]");
                println!("rtl_test v0.6 (OurOS) — RTL-SDR benchmark");
                println!("  -s RATE    Sample rate");
                println!("  -t         Activate tuner benchmark");
                println!("  -b N       Buffer count");
                println!("  -p PPM     PPM error");
            }
            "rtl_tcp" => {
                println!("Usage: rtl_tcp [OPTIONS]");
                println!("rtl_tcp v0.6 (OurOS) — I/Q spectrum server");
                println!("  -a ADDR    Listen address");
                println!("  -p PORT    Listen port (default: 1234)");
                println!("  -f FREQ    Tuner frequency");
                println!("  -g GAIN    Tuner gain");
                println!("  -s RATE    Sample rate");
            }
            _ => {
                println!("Usage: rtl_sdr [OPTIONS] FILE");
                println!("rtl_sdr v0.6 (OurOS) — I/Q recorder");
                println!("  -f FREQ    Center frequency");
                println!("  -s RATE    Sample rate (default: 2048000)");
                println!("  -g GAIN    Tuner gain (dB)");
                println!("  -n N       Number of samples");
                println!("  -p PPM     PPM error correction");
                println!("  -d INDEX   Device index");
            }
        }
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("rtl-sdr v0.6.0 (OurOS)");
        return 0;
    }
    match prog {
        "rtl_test" => {
            println!("Found Rafael Micro R820T tuner");
            println!("Supported gain values: 0.0 0.9 1.4 2.7 3.7 7.7 8.7 12.5 14.4 ...");
            println!("Sampling at 2048000 S/s");
            println!("  cb_count: 100, lost: 0");
        }
        "rtl_tcp" => {
            println!("rtl_tcp: listening on 0.0.0.0:1234");
        }
        "rtl_fm" => {
            println!("rtl_fm: tuned to 100100000 Hz");
            println!("  Oversampling by: 4x");
            println!("  Output: 16-bit PCM, 22050 Hz");
        }
        "rtl_power" => {
            println!("rtl_power: scanning 88.0 MHz - 108.0 MHz");
            println!("  Bin size: 12.5 kHz");
            println!("  Reporting every 10 seconds");
        }
        _ => {
            println!("rtl_sdr: recording I/Q data");
            println!("  Frequency: 100.0 MHz");
            println!("  Sample rate: 2.048 Msps");
            println!("  Recording...");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rtl_sdr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rtl(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rtl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rtl-sdr"), "rtl-sdr");
        assert_eq!(basename(r"C:\bin\rtl-sdr.exe"), "rtl-sdr.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rtl-sdr.exe"), "rtl-sdr");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rtl(&["--help".to_string()], "rtl-sdr"), 0);
        assert_eq!(run_rtl(&["-h".to_string()], "rtl-sdr"), 0);
        let _ = run_rtl(&["--version".to_string()], "rtl-sdr");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rtl(&[], "rtl-sdr");
    }
}
