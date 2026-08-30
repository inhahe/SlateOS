#![deny(clippy::all)]

//! bottom — Slate OS graphical system/process monitor (btm)
//!
//! Single personality: `btm`

use std::env;
use std::process;

fn run_btm(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: btm [OPTIONS]");
        println!();
        println!("A cross-platform graphical process/system monitor.");
        println!();
        println!("Options:");
        println!("  -a, --hide-avg-cpu         Hide average CPU in chart");
        println!("  -b, --basic                Basic mode (no charts)");
        println!("  --battery                  Show battery widget");
        println!("  -S, --case-sensitive       Case-sensitive search by default");
        println!("  -c, --celsius              Use Celsius for temperature (default)");
        println!("  -f, --fahrenheit           Use Fahrenheit for temperature");
        println!("  -k, --kelvin               Use Kelvin for temperature");
        println!("  -C, --config <FILE>        Config file path");
        println!("  -u, --current-usage        Use current CPU usage for process sorting");
        println!("  -t, --default-time <MS>    Default time value (default: 60000)");
        println!("  -d, --time-delta <MS>      Time delta per zoom (default: 15000)");
        println!("  --disable-click            Disable mouse click");
        println!("  -m, --dot-marker           Use dot marker instead of braille");
        println!("  -e, --expanded             Start in expanded mode");
        println!("  --hide-table-gap           Hide spacing between table headers/entries");
        println!("  --hide-time                Hide time graph");
        println!("  -l, --left-legend          Place legend on left side");
        println!("  -n, --unnormalized-cpu     Show unnormalized CPU usage");
        println!("  --network-use-binary-prefix  Use binary prefix for network");
        println!("  --network-use-bytes        Show network in bytes (not bits)");
        println!("  --network-use-log          Use log scale for network");
        println!("  -g, --group-processes      Group processes by name");
        println!("  -p, --process-memory-as-value  Show process memory as value");
        println!("  -r, --rate <MS>            Refresh rate (default: 1000)");
        println!("  --regex                    Use regex by default for search");
        println!("  --show-table-scroll-position  Show scroll position in tables");
        println!("  -T, --tree                 Show process tree by default");
        println!("  --whole-word               Use whole-word search by default");
        println!("  -W, --default-widget-type <W>  Default widget type");
        println!("  --default-widget-count <N> Use the Nth widget of given type");
        println!("  --use-old-network-legend   Use old legend style");
        println!("  --mem-as-values            Show memory as values");
        println!("  --retention <TIME>         Data retention time");
        println!("  -V, --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("bottom 0.10.2 (Slate OS)");
        return 0;
    }

    let basic = args.iter().any(|a| a == "-b" || a == "--basic");
    let tree = args.iter().any(|a| a == "-T" || a == "--tree");

    if basic {
        println!("CPU: ████████████░░░░░░░░  58.3%  (4 cores)");
        println!("Mem: ██████░░░░░░░░░░░░░░  32.1%  (5.2G / 16.0G)");
        println!("Swp: █░░░░░░░░░░░░░░░░░░░   4.2%  (0.3G / 8.0G)");
        println!();
        println!("  PID  Name             CPU%   Mem%   R/s    W/s");
        println!("  201  cargo            28.5    3.2  4.1M   2.3M");
        println!("  180  browser           8.2    5.4  1.2M   0.1M");
        println!("  120  compositor        3.1    2.5  0.5M   0.2M");
        println!("  150  terminal          1.2    1.8  0.1M   0.0M");
        println!("   92  display-server    0.8    0.3  0.0M   0.0M");
    } else if tree {
        println!("btm 0.10.2 (Slate OS) — TUI launched");
        println!();
        println!("Process Tree:");
        println!("  PID  Name               CPU%   Mem%");
        println!("    1  init                0.0    0.1");
        println!("    ├─ 42  service-mgr     0.1    0.2");
        println!("    │  ├─ 85  netmgr       0.0    0.1");
        println!("    │  ├─ 92  display      0.8    0.3");
        println!("    │  └─ 98  audio        0.0    0.1");
        println!("    ├─ 150  terminal       1.2    1.8");
        println!("    └─ 180  browser        8.2    5.4");
    } else {
        println!("btm 0.10.2 (Slate OS) — TUI launched");
        println!("(Graphical terminal UI — simulated output)");
        println!();
        println!("CPU [████████████░░░░░░░░]  58.3%");
        println!("Mem [██████░░░░░░░░░░░░░░]  32.1%  5.2G/16.0G");
        println!("Net: RX 1.2 MB/s  TX 0.3 MB/s");
        println!("Disk: R 4.1 MB/s  W 2.3 MB/s");
        println!();
        println!("Temp:");
        println!("  CPU Package   52.0°C");
        println!("  GPU           48.0°C");
        println!("  NVMe SSD      38.0°C");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_btm(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_btm};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_btm(vec!["--help".to_string()]), 0);
        assert_eq!(run_btm(vec!["-h".to_string()]), 0);
        let _ = run_btm(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_btm(vec![]);
    }
}
