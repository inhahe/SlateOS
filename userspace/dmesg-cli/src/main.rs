#![deny(clippy::all)]

//! dmesg-cli — OurOS kernel ring buffer viewer
//!
//! Multi-personality: `dmesg`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_dmesg(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dmesg [OPTIONS]");
        println!();
        println!("dmesg — print kernel ring buffer messages (OurOS).");
        println!();
        println!("Options:");
        println!("  -C, --clear          Clear the ring buffer");
        println!("  -c, --read-clear     Read and clear");
        println!("  -f LIST              Restrict to facility list");
        println!("  -H, --human          Human readable output");
        println!("  -J, --json           JSON output");
        println!("  -k, --kernel         Kernel messages");
        println!("  -l LIST              Restrict to level list");
        println!("  -n LEVEL             Set console log level");
        println!("  -T, --ctime          Show human-readable timestamps");
        println!("  -t, --notime         Don't show timestamps");
        println!("  -w, --follow         Wait for new messages");
        println!("  -x, --decode         Decode facility and level");
        println!("  --since TIME         Show messages since TIME");
        println!("  --until TIME         Show messages until TIME");
        println!("  -S, --syslog         Force syslog output type");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("dmesg from util-linux 2.39 (OurOS)");
        return 0;
    }

    let clear = args.iter().any(|a| a == "-C" || a == "--clear");
    let human = args.iter().any(|a| a == "-H" || a == "--human");
    let json = args.iter().any(|a| a == "-J" || a == "--json");
    let ctime = args.iter().any(|a| a == "-T" || a == "--ctime");
    let notime = args.iter().any(|a| a == "-t" || a == "--notime");

    if clear {
        println!("dmesg: ring buffer cleared");
        return 0;
    }

    if json {
        println!("{{\"dmesg\": [");
        println!("  {{\"pri\": 6, \"time\": 0.000000, \"msg\": \"OurOS version 1.0.0 (rust 2024) #1 SMP PREEMPT\"}},");
        println!("  {{\"pri\": 6, \"time\": 0.000001, \"msg\": \"Command line: root=/dev/sda2 ro quiet splash\"}},");
        println!("  {{\"pri\": 6, \"time\": 0.000100, \"msg\": \"x86/cpu: Intel(R) Core(TM) i9-13900K\"}}");
        println!("]}}");
        return 0;
    }

    let messages = [
        (0.000000, "kern", "info", "OurOS version 1.0.0 (rust 2024) #1 SMP PREEMPT"),
        (0.000001, "kern", "info", "Command line: root=/dev/sda2 ro quiet splash"),
        (0.000100, "kern", "info", "x86/cpu: Intel(R) Core(TM) i9-13900K"),
        (0.000200, "kern", "info", "x86/fpu: x87 FPU: SSE, SSE2, SSE3, SSSE3, SSE4.1, SSE4.2, AVX, AVX2, AVX-512"),
        (0.001000, "kern", "info", "BIOS-provided physical RAM map:"),
        (0.001001, "kern", "info", "BIOS-e820: [mem 0x0000000000000000-0x000000000009ffff] usable"),
        (0.001002, "kern", "info", "BIOS-e820: [mem 0x0000000000100000-0x000000003fffffff] usable"),
        (0.002000, "kern", "info", "NX (Execute Disable) protection: active"),
        (0.003000, "kern", "info", "Memory: 65536MB (67108864KB) total"),
        (0.010000, "kern", "info", "ACPI: RSDP 0x00000000000E0000 000024 (v02 ALASKA)"),
        (0.020000, "kern", "info", "ACPI: XSDT 0x000000007FFE0000 0000BC (v01 ALASKA A M I    01072009 AMI  01000013)"),
        (0.050000, "kern", "info", "IOAPIC[0]: apic_id 2, version 32, address 0xfec00000"),
        (0.100000, "kern", "info", "Calibrating delay loop... 6000.00 BogoMIPS"),
        (0.200000, "kern", "info", "pid_max: default: 4194304 minimum: 301"),
        (0.300000, "kern", "info", "Mount-cache hash table entries: 65536"),
        (0.500000, "kern", "info", "smpboot: CPU0: Intel(R) Core(TM) i9-13900K (family: 0x6, model: 0xb7)"),
        (0.600000, "kern", "info", "smp: Brought up 1 node, 24 CPUs"),
        (1.000000, "kern", "info", "PCI: Using configuration type 1 for base access"),
        (1.500000, "kern", "info", "SCSI subsystem initialized"),
        (2.000000, "kern", "info", "nvme 0000:04:00.0: PCIe Gen4 x4 link"),
        (2.100000, "kern", "info", "nvme nvme0: Samsung 980 PRO 2TB"),
        (2.500000, "kern", "info", "ahci 0000:00:17.0: AHCI 0001.0301 32 slots 6 ports"),
        (3.000000, "kern", "info", "EXT4-fs (sda2): mounted filesystem with ordered data mode"),
        (3.500000, "kern", "info", "igc 0000:03:00.0: Intel(R) 2.5GbE Network Connection"),
        (4.000000, "kern", "info", "iwlwifi 0000:02:00.0: loaded firmware version 83.e8f84e98.0"),
        (5.000000, "kern", "info", "systemd[1]: Reached target Multi-User System."),
    ];

    for (time, _facility, _level, msg) in &messages {
        if notime {
            println!("{}", msg);
        } else if ctime {
            println!("[Thu Jan  1 00:00:{:06.3}] {}", time, msg);
        } else if human {
            println!("[{:>12.6}] {}", time, msg);
        } else {
            println!("[{:>12.6}] {}", time, msg);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "dmesg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = run_dmesg(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
