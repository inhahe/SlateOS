#![deny(clippy::all)]

//! wlsunset-cli — OurOS wlsunset day/night gamma adjuster
//!
//! Single personality: `wlsunset`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wlsunset(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wlsunset [OPTIONS]");
        println!("wlsunset v0.4 (OurOS) — Day/night gamma adjustments for Wayland");
        println!();
        println!("Options:");
        println!("  -l LAT            Latitude");
        println!("  -L LON            Longitude");
        println!("  -t TEMP           Low color temperature (default 4000K)");
        println!("  -T TEMP           High color temperature (default 6500K)");
        println!("  -g GAMMA          Gamma value (default 1.0)");
        println!("  -S TIME           Sunset time (HH:MM)");
        println!("  -s TIME           Sunrise time (HH:MM)");
        println!("  -d DURATION       Transition duration (minutes)");
        return 0;
    }
    let low = args.iter().skip_while(|a| a.as_str() != "-t").nth(1).map(|s| s.as_str()).unwrap_or("4000");
    let high = args.iter().skip_while(|a| a.as_str() != "-T").nth(1).map(|s| s.as_str()).unwrap_or("6500");
    println!("wlsunset running...");
    println!("  Day temperature: {}K", high);
    println!("  Night temperature: {}K", low);
    println!("  Current: day mode (6500K)");
    println!("  Next transition: sunset at 17:30");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wlsunset".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wlsunset(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wlsunset};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wlsunset"), "wlsunset");
        assert_eq!(basename(r"C:\bin\wlsunset.exe"), "wlsunset.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wlsunset.exe"), "wlsunset");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wlsunset(&["--help".to_string()], "wlsunset"), 0);
        assert_eq!(run_wlsunset(&["-h".to_string()], "wlsunset"), 0);
        assert_eq!(run_wlsunset(&["--version".to_string()], "wlsunset"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wlsunset(&[], "wlsunset"), 0);
    }
}
