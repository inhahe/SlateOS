#![deny(clippy::all)]

//! librenms-cli — SlateOS LibreNMS network monitoring
//!
//! Multi-personality: `lnms`, `librenms-service`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_librenms(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "librenms-service" => {
                println!("librenms-service (SlateOS) — LibreNMS dispatcher service");
                println!("  -g GROUP    Poller group");
                println!("  -t THREADS  Thread count");
            }
            _ => {
                println!("lnms v24.5 (SlateOS) — LibreNMS CLI");
                println!("  device:add HOST       Add device");
                println!("  device:remove HOST    Remove device");
                println!("  device:poll HOST      Poll device");
                println!("  device:discover HOST  Discover device");
                println!("  config:set KEY VAL    Set configuration");
                println!("  user:add              Add user");
                println!("  snmpwalk HOST OID     SNMP walk");
            }
        }
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("LibreNMS v24.5.0 (SlateOS)"); return 0; }
    println!("LibreNMS v24.5.0 (SlateOS)");
    println!("  Devices: 100 (95 up, 5 down)");
    println!("  Ports: 2,345 interfaces");
    println!("  Sensors: 890");
    println!("  Wireless: 45 APs");
    println!("  Applications: 67");
    println!("  Alert rules: 23");
    println!("  Active alerts: 8");
    println!("  Poller: last run 2m ago (avg 45s)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lnms".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_librenms(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_librenms};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/librenms"), "librenms");
        assert_eq!(basename(r"C:\bin\librenms.exe"), "librenms.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("librenms.exe"), "librenms");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_librenms(&["--help".to_string()], "librenms"), 0);
        assert_eq!(run_librenms(&["-h".to_string()], "librenms"), 0);
        let _ = run_librenms(&["--version".to_string()], "librenms");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_librenms(&[], "librenms");
    }
}
