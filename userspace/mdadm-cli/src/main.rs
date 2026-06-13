#![deny(clippy::all)]

//! mdadm-cli — SlateOS mdadm RAID management CLI
//!
//! Single personality: `mdadm`

use std::env;
use std::process;

fn run_mdadm(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mdadm [MODE] DEVICE [OPTIONS]");
        println!();
        println!("mdadm — manage MD (Software RAID) devices (Slate OS).");
        println!();
        println!("Modes:");
        println!("  --create, -C           Create array");
        println!("  --assemble, -A         Assemble array");
        println!("  --detail, -D           Show array detail");
        println!("  --examine, -E          Examine superblock");
        println!("  --stop, -S             Stop array");
        println!("  --add                  Add device to array");
        println!("  --remove               Remove device from array");
        println!("  --grow, -G             Grow/reshape array");
        println!("  --scan                 Scan for arrays");
        println!();
        println!("Create options:");
        println!("  --level=N, -l N        RAID level (0, 1, 5, 6, 10)");
        println!("  --raid-devices=N, -n N Number of devices");
        println!("  --spare-devices=N      Number of spares");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("mdadm - v4.3 (Slate OS)");
        return 0;
    }

    let create = args.iter().any(|a| a == "--create" || a == "-C");
    let detail = args.iter().any(|a| a == "--detail" || a == "-D");
    let examine = args.iter().any(|a| a == "--examine" || a == "-E");
    let stop = args.iter().any(|a| a == "--stop" || a == "-S");
    let scan = args.iter().any(|a| a == "--scan");

    let device = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("/dev/md0");

    if create {
        let level = args.windows(2).find(|w| w[0] == "--level" || w[0] == "-l")
            .map(|w| w[1].as_str()).unwrap_or("1");
        let num = args.windows(2).find(|w| w[0] == "--raid-devices" || w[0] == "-n")
            .map(|w| w[1].as_str()).unwrap_or("2");
        println!("mdadm: array {} started.", device);
        println!("  RAID level: {}", level);
        println!("  Raid devices: {}", num);
    } else if detail {
        println!("{}:", device);
        println!("           Version : 1.2");
        println!("     Creation Time : Mon Jan 15 08:00:00 2024");
        println!("        Raid Level : raid1");
        println!("        Array Size : 524156928 (499.87 GiB)");
        println!("     Used Dev Size : 524156928 (499.87 GiB)");
        println!("      Raid Devices : 2");
        println!("     Total Devices : 2");
        println!("             State : clean");
        println!("    Active Devices : 2");
        println!("   Working Devices : 2");
        println!("    Failed Devices : 0");
        println!("     Spare Devices : 0");
        println!();
        println!("    Number   Major   Minor   RaidDevice State");
        println!("       0       8       16        0      active sync   /dev/sdb");
        println!("       1       8       32        1      active sync   /dev/sdc");
    } else if examine {
        println!("  Magic : a92b4efc");
        println!("  Version : 1.2");
        println!("  Array UUID : abcdef12:34567890:abcdef12:34567890");
        println!("  Name : slateos:0");
        println!("  Raid Level : raid1");
        println!("  Raid Devices : 2");
    } else if stop {
        println!("mdadm: stopped {}.", device);
    } else if scan {
        println!("ARRAY /dev/md0 metadata=1.2 name=slateos:0 UUID=abcdef12:34567890:abcdef12:34567890");
    } else {
        eprintln!("mdadm: no mode specified. See --help.");
        return 1;
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mdadm(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mdadm};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mdadm(vec!["--help".to_string()]), 0);
        assert_eq!(run_mdadm(vec!["-h".to_string()]), 0);
        let _ = run_mdadm(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mdadm(vec![]);
    }
}
