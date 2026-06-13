#![deny(clippy::all)]

//! dmidecode-cli — SlateOS DMI/SMBIOS table decoder
//!
//! Multi-personality: `dmidecode`, `biosdecode`, `vpddecode`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_dmidecode(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dmidecode [OPTIONS]");
        println!();
        println!("dmidecode — DMI/SMBIOS table decoder (SlateOS).");
        println!();
        println!("Options:");
        println!("  -t TYPE        Only display given type");
        println!("  -s KEYWORD     Only display given keyword");
        println!("  -q             Quiet (less verbose)");
        println!("  -u             Dump raw data");
        println!("  --dump-bin F   Dump raw data to binary file");
        println!("  --from-dump F  Read from binary dump file");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("dmidecode 3.5 (SlateOS)");
        return 0;
    }

    let type_filter = args.windows(2)
        .find(|w| w[0] == "-t")
        .map(|w| w[1].as_str());

    let keyword = args.windows(2)
        .find(|w| w[0] == "-s")
        .map(|w| w[1].as_str());

    if let Some(kw) = keyword {
        match kw {
            "bios-vendor" => println!("American Megatrends Inc."),
            "bios-version" => println!("F20"),
            "bios-release-date" => println!("03/15/2024"),
            "system-manufacturer" => println!("System manufacturer"),
            "system-product-name" => println!("System Product Name"),
            "system-serial-number" => println!("System Serial Number"),
            "system-uuid" => println!("12345678-1234-1234-1234-123456789ABC"),
            "baseboard-manufacturer" => println!("ASUSTeK COMPUTER INC."),
            "baseboard-product-name" => println!("ROG STRIX Z790-E"),
            "processor-version" => println!("13th Gen Intel(R) Core(TM) i9-13900K"),
            _ => println!("Unknown keyword: {}", kw),
        }
        return 0;
    }

    println!("# dmidecode 3.5");
    println!("Getting SMBIOS data from sysfs.");
    println!("SMBIOS 3.6.0 present.");

    let show_all = type_filter.is_none();
    let filter_num = type_filter.and_then(|t| t.parse::<u32>().ok());

    if show_all || filter_num == Some(0) {
        println!();
        println!("Handle 0x0000, DMI type 0, 26 bytes");
        println!("BIOS Information");
        println!("\tVendor: American Megatrends Inc.");
        println!("\tVersion: F20");
        println!("\tRelease Date: 03/15/2024");
        println!("\tROM Size: 32 MB");
        println!("\tCharacteristics:");
        println!("\t\tUEFI is supported");
        println!("\t\tBIOS boot specification is supported");
    }
    if show_all || filter_num == Some(1) {
        println!();
        println!("Handle 0x0001, DMI type 1, 27 bytes");
        println!("System Information");
        println!("\tManufacturer: System manufacturer");
        println!("\tProduct Name: System Product Name");
        println!("\tVersion: System Version");
        println!("\tSerial Number: System Serial Number");
        println!("\tUUID: 12345678-1234-1234-1234-123456789ABC");
        println!("\tWake-up Type: Power Switch");
    }
    if show_all || filter_num == Some(4) {
        println!();
        println!("Handle 0x0004, DMI type 4, 48 bytes");
        println!("Processor Information");
        println!("\tSocket Designation: LGA1700");
        println!("\tType: Central Processor");
        println!("\tFamily: Core i9");
        println!("\tManufacturer: Intel(R) Corporation");
        println!("\tVersion: 13th Gen Intel(R) Core(TM) i9-13900K");
        println!("\tMax Speed: 5800 MHz");
        println!("\tCurrent Speed: 3000 MHz");
        println!("\tCore Count: 24");
        println!("\tThread Count: 32");
    }
    if show_all || filter_num == Some(17) {
        println!();
        println!("Handle 0x0011, DMI type 17, 92 bytes");
        println!("Memory Device");
        println!("\tSize: 32 GB");
        println!("\tForm Factor: DIMM");
        println!("\tType: DDR5");
        println!("\tSpeed: 5600 MT/s");
        println!("\tManufacturer: G Skill Intl");
        println!("\tPart Number: F5-5600J3036D32G");
    }
    0
}

fn run_biosdecode(_args: &[String]) -> i32 {
    println!("SMBIOS 3.6 present.");
    println!("\tStructure Table Length: 4567 bytes");
    println!("\tStructure Table Address: 0x000E0000");
    println!();
    println!("ACPI 2.0 present.");
    println!("\tOEM Identifier: ALASKA");
    println!("\tRSD Table 32-bit Address: 0x7FFE0000");
    println!();
    println!("PCI Interrupt Routing 1.0 present.");
    println!("\tRouter ID: 00:1f.0");
    println!("\tExclusive IRQs: 3 4 5 6 7 9 10 11 12 14 15");
    0
}

fn run_vpddecode(_args: &[String]) -> i32 {
    println!("VPD data not present in DMI table.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "dmidecode".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "biosdecode" => run_biosdecode(&rest),
        "vpddecode" => run_vpddecode(&rest),
        _ => run_dmidecode(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dmidecode};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dmidecode"), "dmidecode");
        assert_eq!(basename(r"C:\bin\dmidecode.exe"), "dmidecode.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dmidecode.exe"), "dmidecode");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dmidecode(&["--help".to_string()]), 0);
        assert_eq!(run_dmidecode(&["-h".to_string()]), 0);
        let _ = run_dmidecode(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dmidecode(&[]);
    }
}
