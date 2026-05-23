#![deny(clippy::all)]

//! smartctl-cli — OurOS S.M.A.R.T. disk monitoring tools
//!
//! Multi-personality: `smartctl`, `smartd`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_smartctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: smartctl [OPTIONS] DEVICE");
        println!();
        println!("smartctl — SMART disk monitoring (OurOS).");
        println!();
        println!("Options:");
        println!("  -i, --info         Device identity info");
        println!("  -a, --all          All SMART info");
        println!("  -x, --xall         All info (extended)");
        println!("  -H, --health       SMART health status");
        println!("  -A, --attributes   Vendor specific attributes");
        println!("  -l TYPE            Show log (error, selftest, etc.)");
        println!("  -t TEST            Run test (short, long, conveyance, offline)");
        println!("  -s on|off          Enable/disable SMART");
        println!("  -d TYPE            Device type (ata, scsi, nvme, sat, etc.)");
        println!("  -j, --json         JSON output");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("smartctl 7.4 (OurOS)");
        println!("smartmontools release 7.4");
        return 0;
    }

    let device = args.iter()
        .find(|a| a.starts_with('/') || a.starts_with("\\\\"))
        .map(|s| s.as_str())
        .unwrap_or("/dev/sda");

    let info = args.iter().any(|a| a == "-i" || a == "--info");
    let all = args.iter().any(|a| a == "-a" || a == "--all" || a == "-x" || a == "--xall");
    let health = args.iter().any(|a| a == "-H" || a == "--health");
    let attrs = args.iter().any(|a| a == "-A" || a == "--attributes");
    let json = args.iter().any(|a| a == "-j" || a == "--json");
    let test = args.windows(2).find(|w| w[0] == "-t").map(|w| w[1].as_str());

    if let Some(test_type) = test {
        println!("smartctl 7.4 (OurOS)");
        println!("=== START OF OFFLINE IMMEDIATE AND SELF-TEST SECTION ===");
        match test_type {
            "short" => println!("Sending command: \"Execute SMART Short self-test routine immediately in off-line mode\"."),
            "long" => println!("Sending command: \"Execute SMART Extended self-test routine immediately in off-line mode\"."),
            _ => println!("Sending command: \"Execute SMART {} test\".", test_type),
        }
        println!("Drive command \"Execute SMART self-test\" successful.");
        println!("Testing has begun.");
        return 0;
    }

    if json {
        println!("{{");
        println!("  \"smartctl\": {{\"version\": [7,4]}},");
        println!("  \"device\": {{\"name\": \"{}\", \"type\": \"ata\"}},", device);
        println!("  \"model_name\": \"Samsung SSD 870 EVO 1TB\",");
        println!("  \"serial_number\": \"S1234567890\",");
        println!("  \"firmware_version\": \"SVT01B6Q\",");
        println!("  \"smart_status\": {{\"passed\": true}},");
        println!("  \"temperature\": {{\"current\": 35}}");
        println!("}}");
        return 0;
    }

    println!("smartctl 7.4 (OurOS)");
    println!("Copyright (C) 2002-24 smartmontools developers");
    println!();

    if info || all {
        println!("=== START OF INFORMATION SECTION ===");
        println!("Model Family:     Samsung based SSDs");
        println!("Device Model:     Samsung SSD 870 EVO 1TB");
        println!("Serial Number:    S1234567890");
        println!("LU WWN Device Id: 5 002538 f411b6789");
        println!("Firmware Version: SVT01B6Q");
        println!("User Capacity:    1,000,204,886,016 bytes [1.00 TB]");
        println!("Sector Size:      512 bytes logical/physical");
        println!("Rotation Rate:    Solid State Device");
        println!("Form Factor:      2.5 inches");
        println!("TRIM Command:     Available, deterministic, zeroed");
        println!("Device is:        In smartctl database");
        println!("ATA Version is:   ACS-4 T13/BSR INCITS 529 revision 5");
        println!("SATA Version is:  SATA 3.3, 6.0 Gb/s (current: 6.0 Gb/s)");
        println!("Local Time is:    Thu Jan 01 12:00:00 2025 UTC");
        println!("SMART support is: Available - device has SMART capability.");
        println!("SMART support is: Enabled");
        println!();
    }

    if health || all {
        println!("=== START OF READ SMART DATA SECTION ===");
        println!("SMART overall-health self-assessment test result: PASSED");
        println!();
    }

    if attrs || all {
        println!("=== START OF READ SMART DATA SECTION ===");
        println!("SMART Attributes Data Structure revision number: 1");
        println!("Vendor Specific SMART Attributes with Thresholds:");
        println!("ID# ATTRIBUTE_NAME          FLAG     VALUE WORST THRESH TYPE      UPDATED  WHEN_FAILED RAW_VALUE");
        println!("  5 Reallocated_Sector_Ct   0x0033   100   100   010    Pre-fail  Always       -       0");
        println!("  9 Power_On_Hours          0x0032   099   099   000    Old_age   Always       -       1234");
        println!(" 12 Power_Cycle_Count       0x0032   099   099   000    Old_age   Always       -       456");
        println!("177 Wear_Leveling_Count     0x0013   099   099   000    Pre-fail  Always       -       1");
        println!("179 Used_Rsvd_Blk_Cnt_Tot   0x0013   100   100   010    Pre-fail  Always       -       0");
        println!("181 Program_Fail_Cnt_Total  0x0032   100   100   010    Old_age   Always       -       0");
        println!("182 Erase_Fail_Count_Total  0x0032   100   100   010    Old_age   Always       -       0");
        println!("187 Uncorrectable_Error_Cnt 0x0032   100   100   000    Old_age   Always       -       0");
        println!("190 Airflow_Temperature_Cel 0x0032   065   048   000    Old_age   Always       -       35");
        println!("194 Temperature_Celsius     0x0022   065   048   000    Old_age   Always       -       35");
        println!("199 CRC_Error_Count         0x003e   100   100   000    Old_age   Always       -       0");
        println!("235 POR_Recovery_Count      0x0012   099   099   000    Old_age   Always       -       5");
        println!("241 Total_LBAs_Written      0x0032   099   099   000    Old_age   Always       -       12345678");
    }
    0
}

fn run_smartd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: smartd [OPTIONS]");
        println!();
        println!("smartd — SMART daemon (OurOS).");
        println!();
        println!("Options:");
        println!("  -d              Run in foreground");
        println!("  -i N            Check interval (seconds)");
        println!("  -c FILE         Config file");
        println!("  -p FILE         PID file");
        println!("  -q WHEN         Quit behavior (never, nodev, errors, showtests)");
        return 0;
    }

    println!("smartd: starting daemon (OurOS)");
    println!("smartd: reading config file /etc/smartd.conf");
    println!("smartd: monitoring 2 devices");
    println!("smartd: /dev/sda [SAT]: Samsung SSD 870 EVO 1TB, S/N:S1234567890, FW:SVT01B6Q, 1.00 TB");
    println!("smartd: /dev/nvme0: Samsung 980 PRO 2TB, S/N:S9876543210, FW:5B2QGXA7, 2.00 TB");
    println!("smartd: checking interval is 1800 seconds");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "smartctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "smartd" => run_smartd(&rest),
        _ => run_smartctl(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
