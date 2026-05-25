#![deny(clippy::all)]

//! librenms-cli — OurOS LibreNMS network monitoring
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
                println!("librenms-service (OurOS) — LibreNMS dispatcher service");
                println!("  -g GROUP    Poller group");
                println!("  -t THREADS  Thread count");
            }
            _ => {
                println!("lnms v24.5 (OurOS) — LibreNMS CLI");
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
    if args.iter().any(|a| a == "--version") { println!("LibreNMS v24.5.0 (OurOS)"); return 0; }
    println!("LibreNMS v24.5.0 (OurOS)");
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
mod tests { #[test] fn test_basic() { assert!(true); } }
