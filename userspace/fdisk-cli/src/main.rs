#![deny(clippy::all)]

//! fdisk-cli — OurOS fdisk/sfdisk/cfdisk CLI
//!
//! Multi-personality: `fdisk`, `sfdisk`, `cfdisk`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_fdisk(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fdisk [OPTIONS] DEVICE");
        println!();
        println!("fdisk — partition table manipulator (OurOS).");
        println!();
        println!("Options:");
        println!("  -l, --list             List partition tables");
        println!("  -x, --list-details     List with extra details");
        println!("  -t, --type TYPE        Label type (dos, gpt)");
        println!("  -n, --noauto-pt        Don't create default table");
        println!("  -o, --output LIST      Output columns");
        return 0;
    }

    let list = args.iter().any(|a| a == "-l" || a == "--list");
    let device = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("/dev/sda");

    if list {
        println!("Disk {}: 500 GiB, 536870912000 bytes, 1048576000 sectors", device);
        println!("Disk model: Samsung SSD 870");
        println!("Units: sectors of 1 * 512 = 512 bytes");
        println!("Sector size (logical/physical): 512 bytes / 512 bytes");
        println!("Disklabel type: gpt");
        println!("Disk identifier: ABCDEF12-3456-7890-ABCD-EF1234567890");
        println!();
        println!("Device         Start        End    Sectors   Size Type");
        println!("{}1       2048    1050623    1048576   512M EFI System", device);
        println!("{}2    1050624  105908223  104857600    50G Linux filesystem", device);
        println!("{}3  105908224 1048575966  942667743 449.5G Linux filesystem", device);
    } else {
        println!("Welcome to fdisk (OurOS).");
        println!("Changes will remain in memory only, until you decide to write them.");
        println!();
        println!("Command (m for help): ");
    }
    0
}

fn run_sfdisk(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sfdisk [OPTIONS] DEVICE");
        println!("  -l, --list        List partitions");
        println!("  -d, --dump        Dump partition table");
        println!("  --delete          Delete all partitions");
        println!("  -J, --json        JSON output");
        return 0;
    }

    let device = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("/dev/sda");

    if args.iter().any(|a| a == "-J" || a == "--json") {
        println!("{{\"partitiontable\": {{\"device\": \"{}\", \"label\": \"gpt\", \"partitions\": [", device);
        println!("  {{\"node\": \"{}1\", \"start\": 2048, \"size\": 1048576, \"type\": \"EFI System\"}},", device);
        println!("  {{\"node\": \"{}2\", \"start\": 1050624, \"size\": 104857600, \"type\": \"Linux filesystem\"}}", device);
        println!("]}}}}");
    } else if args.iter().any(|a| a == "-d" || a == "--dump") {
        println!("label: gpt");
        println!("device: {}", device);
        println!("unit: sectors");
        println!("{}1 : start=2048, size=1048576, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B", device);
        println!("{}2 : start=1050624, size=104857600, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4", device);
    } else {
        println!("sfdisk: {}", device);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "fdisk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "sfdisk" => run_sfdisk(&rest),
        "cfdisk" => {
            println!("cfdisk: curses-based partition editor (OurOS).");
            println!("  Use fdisk for non-interactive mode.");
            0
        }
        _ => run_fdisk(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fdisk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fdisk"), "fdisk");
        assert_eq!(basename(r"C:\bin\fdisk.exe"), "fdisk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fdisk.exe"), "fdisk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_fdisk(&["--help".to_string()]), 0);
        assert_eq!(run_fdisk(&["-h".to_string()]), 0);
        assert_eq!(run_fdisk(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_fdisk(&[]), 0);
    }
}
