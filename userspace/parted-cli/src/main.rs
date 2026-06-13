#![deny(clippy::all)]

//! parted-cli — Slate OS GNU Parted CLI
//!
//! Single personality: `parted`

use std::env;
use std::process;

fn run_parted(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: parted [OPTIONS] [DEVICE [COMMAND [ARGS...]]]");
        println!();
        println!("GNU Parted — partition editor (Slate OS).");
        println!();
        println!("Options:");
        println!("  -l, --list             List all block devices");
        println!("  -s, --script           Never prompt");
        println!("  -a, --align ALIGN      Alignment (none, cylinder, minimal, optimal)");
        println!("  -m, --machine          Machine-readable output");
        println!();
        println!("Commands:");
        println!("  mklabel TYPE           Create partition table (gpt, msdos)");
        println!("  mkpart TYPE START END  Create partition");
        println!("  rm NUMBER              Delete partition");
        println!("  print                  Print partition table");
        println!("  name NUMBER NAME       Name partition");
        println!("  set NUMBER FLAG on/off Set partition flag (boot, esp, lvm, raid)");
        println!("  resizepart NUM END     Resize partition");
        println!("  unit UNIT              Set display unit (s, B, kB, MB, GB, TB, %)");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("parted (GNU parted) 3.6 (Slate OS)");
        return 0;
    }

    let list = args.iter().any(|a| a == "-l" || a == "--list");
    let machine = args.iter().any(|a| a == "-m" || a == "--machine");

    if list {
        if machine {
            println!("/dev/sda:500GB:scsi:512:512:gpt:Samsung SSD 870:;");
            println!("1:1049kB:538MB:537MB:fat32:EFI System Partition:boot, esp;");
            println!("2:538MB:54.2GB:53.7GB:ext4::;");
            println!("3:54.2GB:500GB:446GB:ext4::;");
        } else {
            println!("Model: Samsung SSD 870 (scsi)");
            println!("Disk /dev/sda: 500GB");
            println!("Sector size (logical/physical): 512B/512B");
            println!("Partition Table: gpt");
            println!("Disk Flags: ");
            println!();
            println!("Number  Start   End     Size    File system  Name                  Flags");
            println!(" 1      1049kB  538MB   537MB   fat32        EFI System Partition   boot, esp");
            println!(" 2      538MB   54.2GB  53.7GB  ext4");
            println!(" 3      54.2GB  500GB   446GB   ext4");
        }
    } else {
        let device = args.iter().find(|a| !a.starts_with('-'))
            .map(|s| s.as_str()).unwrap_or("/dev/sda");
        let cmd = args.iter().filter(|a| !a.starts_with('-'))
            .nth(1).map(|s| s.as_str());

        match cmd {
            Some("print") | None => {
                println!("Model: Samsung SSD 870 (scsi)");
                println!("Disk {}: 500GB", device);
                println!("Sector size (logical/physical): 512B/512B");
                println!("Partition Table: gpt");
                println!();
                println!("Number  Start   End     Size    File system  Name  Flags");
                println!(" 1      1049kB  538MB   537MB   fat32              boot, esp");
                println!(" 2      538MB   54.2GB  53.7GB  ext4");
                println!(" 3      54.2GB  500GB   446GB   ext4");
            }
            Some("mklabel") => println!("Warning: This will destroy all data on {}. Continue?", device),
            Some("mkpart") => println!("Information: Partition created."),
            Some("rm") => println!("Information: Partition deleted."),
            Some(other) => println!("parted: {}: see parted --help.", other),
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_parted(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_parted};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_parted(vec!["--help".to_string()]), 0);
        assert_eq!(run_parted(vec!["-h".to_string()]), 0);
        let _ = run_parted(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_parted(vec![]);
    }
}
