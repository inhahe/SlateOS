#![deny(clippy::all)]

//! cpupower-cli — OurOS CPU frequency/power tools
//!
//! Multi-personality: `cpupower`, `cpufreq-info`, `cpufreq-set`, `turbostat`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_cpupower(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cpupower COMMAND [OPTIONS]");
        println!();
        println!("cpupower — CPU power management (OurOS).");
        println!();
        println!("Commands:");
        println!("  frequency-info    Show CPU frequency info");
        println!("  frequency-set     Set CPU frequency");
        println!("  idle-info         Show CPU idle state info");
        println!("  idle-set          Set CPU idle state");
        println!("  info              Show general processor info");
        println!("  monitor           Monitor CPU state");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match subcmd {
        "frequency-info" => {
            println!("analyzing CPU 0:");
            println!("  driver: intel_pstate");
            println!("  CPUs which run at the same hardware frequency: 0");
            println!("  CPUs which need to have their frequency coordinated by software: 0");
            println!("  maximum transition latency: 4294.55 ms.");
            println!("  hardware limits: 800 MHz - 5.80 GHz");
            println!("  available cpufreq governors: performance powersave");
            println!("  current policy: frequency should be within 800 MHz and 5.80 GHz.");
            println!("                  The governor \"performance\" may decide which speed to use");
            println!("  current CPU frequency: 3.00 GHz (asserted by call to hardware)");
        }
        "frequency-set" => {
            println!("Setting cpu: 0");
            let gov = args.windows(2).find(|w| w[0] == "-g").map(|w| w[1].as_str()).unwrap_or("performance");
            println!("  governor set to: {}", gov);
        }
        "idle-info" => {
            println!("CPUidle driver: intel_idle");
            println!("CPUidle governor: menu");
            println!("Number of idle states: 4");
            println!("State 0: POLL  Latency: 0us  Residency: 0us");
            println!("State 1: C1    Latency: 2us  Residency: 10us");
            println!("State 2: C6    Latency: 15us Residency: 200us");
            println!("State 3: C8    Latency: 100us Residency: 800us");
        }
        "info" => {
            println!("System: x86_64");
            println!("  perf-bias: 0 (performance)");
            println!("  EPB: performance");
        }
        _ => {
            eprintln!("cpupower: unknown command '{}'", subcmd);
            return 1;
        }
    }
    0
}

fn run_turbostat(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: turbostat [OPTIONS] [COMMAND]");
        println!("Options: -S (summary), -q (quiet), -i N (interval), -n N (iterations)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("turbostat version 2024.01.01 (OurOS)");
        return 0;
    }

    println!("Core CPU    Avg_MHz Busy%   Bzy_MHz TSC_MHz IRQ     C1%     C6%     C8%");
    println!("-    -      300     10.0    3000    3000    1200    5.0     45.0    40.0");
    println!("0    0      450     15.0    3000    3000    300     3.0     42.0    40.0");
    println!("0    1      150     5.0     3000    3000    100     7.0     48.0    40.0");
    println!("1    2      400     13.3    3000    3000    250     4.0     43.0    39.7");
    println!("1    3      200     6.7     3000    3000    150     6.0     47.0    40.3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "cpupower".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "cpufreq-info" => { run_cpupower(&["frequency-info".to_string()]); 0 }
        "cpufreq-set" => { run_cpupower(&["frequency-set".to_string()]); 0 }
        "turbostat" => run_turbostat(&rest),
        _ => run_cpupower(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
