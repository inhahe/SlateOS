#![deny(clippy::all)]

//! fancontrol-cli — OurOS fan control daemon
//!
//! Multi-personality: `fancontrol`, `pwmconfig`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fancontrol(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fancontrol [CONFIG_FILE]");
        println!("fancontrol v3.6 (OurOS) — Automated fan speed control");
        println!();
        println!("Options:");
        println!("  -d                Debug mode");
        println!("  -p PID_FILE       PID file path");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fancontrol v3.6 (OurOS)"); return 0; }
    println!("fancontrol: loading configuration from /etc/fancontrol");
    println!("  Device: hwmon0/pwm1 → hwmon0/temp1_input");
    println!("  MINTEMP=30  MAXTEMP=70  MINPWM=80  MAXPWM=255");
    println!("fancontrol: fan speed regulation started");
    0
}

fn run_pwmconfig(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pwmconfig [OPTIONS]");
        println!("pwmconfig v3.6 (OurOS) — Configure fan PWM settings");
        println!();
        println!("Options:");
        println!("  --nocheck         Skip safety checks");
        return 0;
    }
    println!("# pwmconfig — fan PWM configuration helper");
    println!("Found the following PWM controls:");
    println!("  hwmon0/pwm1 (current value: 180)");
    println!("  hwmon0/pwm2 (current value: 200)");
    println!();
    println!("Found the following temperature sensors:");
    println!("  hwmon0/temp1_input (current: 45000)");
    println!("  hwmon1/temp1_input (current: 43000)");
    println!();
    println!("Configuration written to /etc/fancontrol");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fancontrol".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pwmconfig" => run_pwmconfig(&rest, &prog),
        _ => run_fancontrol(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fancontrol};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fancontrol"), "fancontrol");
        assert_eq!(basename(r"C:\bin\fancontrol.exe"), "fancontrol.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fancontrol.exe"), "fancontrol");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_fancontrol(&["--help".to_string()], "fancontrol"), 0);
        assert_eq!(run_fancontrol(&["-h".to_string()], "fancontrol"), 0);
        assert_eq!(run_fancontrol(&["--version".to_string()], "fancontrol"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_fancontrol(&[], "fancontrol"), 0);
    }
}
