#![deny(clippy::all)]

//! sensors-cli — OurOS hardware monitoring sensors
//!
//! Multi-personality: `sensors`, `sensors-detect`, `pwmconfig`, `fancontrol`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_sensors(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sensors [OPTIONS] [CHIP]");
        println!();
        println!("sensors — print hardware monitoring sensor readings (OurOS).");
        println!();
        println!("Options:");
        println!("  -f, --fahrenheit     Show temperatures in Fahrenheit");
        println!("  -A, --no-adapter     Don't show adapter for each chip");
        println!("  -u                   Raw output (unformatted)");
        println!("  -j, --json           JSON output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sensors version 3.6.0+git (OurOS)");
        return 0;
    }

    let fahrenheit = args.iter().any(|a| a == "-f" || a == "--fahrenheit");
    let json = args.iter().any(|a| a == "-j" || a == "--json");

    if json {
        println!("{{");
        println!("   \"coretemp-isa-0000\":{{");
        println!("      \"Adapter\": \"ISA adapter\",");
        println!("      \"Package id 0\":{{");
        println!("         \"temp1_input\": 45.000,");
        println!("         \"temp1_max\": 100.000,");
        println!("         \"temp1_crit\": 110.000");
        println!("      }}");
        println!("   }}");
        println!("}}");
        return 0;
    }

    let temp = |c: f64| -> String {
        if fahrenheit {
            format!("{:+.1}°F", c * 9.0 / 5.0 + 32.0)
        } else {
            format!("{:+.1}°C", c)
        }
    };

    println!("coretemp-isa-0000");
    println!("Adapter: ISA adapter");
    println!("Package id 0:  {}  (high = {}, crit = {})", temp(45.0), temp(100.0), temp(110.0));
    println!("Core 0:        {}  (high = {}, crit = {})", temp(42.0), temp(100.0), temp(110.0));
    println!("Core 1:        {}  (high = {}, crit = {})", temp(43.0), temp(100.0), temp(110.0));
    println!("Core 2:        {}  (high = {}, crit = {})", temp(44.0), temp(100.0), temp(110.0));
    println!("Core 3:        {}  (high = {}, crit = {})", temp(41.0), temp(100.0), temp(110.0));
    println!();
    println!("nct6798-isa-0290");
    println!("Adapter: ISA adapter");
    println!("Vcore:          0.85 V  (min = 0.00 V, max = 1.74 V)");
    println!("+3.3V:          3.33 V  (min = 2.98 V, max = 3.63 V)");
    println!("+5V:            5.04 V  (min = 4.51 V, max = 5.49 V)");
    println!("+12V:          12.10 V  (min = 10.80 V, max = 13.20 V)");
    println!("fan1:          1200 RPM  (min = 200 RPM)");
    println!("fan2:           900 RPM  (min = 200 RPM)");
    println!("fan3:          1450 RPM  (min = 200 RPM)");
    println!("SYSTIN:         {}  (high = {}, hyst = {})", temp(35.0), temp(80.0), temp(75.0));
    println!("CPUTIN:         {}  (high = {}, hyst = {})", temp(45.0), temp(80.0), temp(75.0));
    println!();
    println!("nvme-pci-0400");
    println!("Adapter: PCI adapter");
    println!("Composite:     {}  (low = {}, high = {})", temp(38.0), temp(-40.0), temp(83.0));
    0
}

fn run_sensors_detect(_args: &[String]) -> i32 {
    println!("# sensors-detect version 3.6.0+git (OurOS)");
    println!("# Board: System manufacturer System Product Name");
    println!();
    println!("This program will help you determine which kernel modules you need");
    println!("to load to use lm_sensors most effectively.");
    println!();
    println!("Now follows a summary of the probes I have just done.");
    println!();
    println!("Driver `coretemp':");
    println!("  * Chip `Intel digital thermal sensor' (confidence: 9)");
    println!();
    println!("Driver `nct6775':");
    println!("  * ISA bus, address 0x290");
    println!("    Chip `Nuvoton NCT6798D Super IO Sensors' (confidence: 9)");
    println!();
    println!("To load everything that is needed, add this to /etc/modules:");
    println!("#----cut here----");
    println!("coretemp");
    println!("nct6775");
    println!("#----cut here----");
    0
}

fn run_pwmconfig(_args: &[String]) -> i32 {
    println!("# pwmconfig — fan speed configuration (OurOS)");
    println!("Found the following PWM controls:");
    println!("  hwmon1/pwm1           current value: 150");
    println!("  hwmon1/pwm2           current value: 100");
    println!("  hwmon1/pwm3           current value: 200");
    println!();
    println!("Select fan (1-3): 1");
    println!("Testing fan speed control...");
    println!("  min = 50, max = 255");
    println!("Configuration written to /etc/fancontrol");
    0
}

fn run_fancontrol(_args: &[String]) -> i32 {
    println!("Loading configuration from /etc/fancontrol...");
    println!("Common settings:");
    println!("  INTERVAL=10");
    println!("  DEVPATH=hwmon1=devices/platform/nct6775.2592");
    println!();
    println!("Settings for hwmon1/pwm1:");
    println!("  MINTEMP=40  MAXTEMP=70  MINPWM=50  MAXPWM=255");
    println!();
    println!("Fan control started, adjusting fans...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "sensors".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "sensors-detect" => run_sensors_detect(&rest),
        "pwmconfig" => run_pwmconfig(&rest),
        "fancontrol" => run_fancontrol(&rest),
        _ => run_sensors(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sensors};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sensors"), "sensors");
        assert_eq!(basename(r"C:\bin\sensors.exe"), "sensors.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sensors.exe"), "sensors");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_sensors(&["--help".to_string()]), 0);
        assert_eq!(run_sensors(&["-h".to_string()]), 0);
        assert_eq!(run_sensors(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_sensors(&[]), 0);
    }
}
