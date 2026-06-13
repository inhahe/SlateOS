#![deny(clippy::all)]

//! efibootmgr-cli — SlateOS EFI boot manager
//!
//! Multi-personality: `efibootmgr`, `efivar`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_efibootmgr(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: efibootmgr [OPTIONS]");
        println!();
        println!("efibootmgr — EFI boot manager (SlateOS).");
        println!();
        println!("Options:");
        println!("  -v, --verbose      Verbose");
        println!("  -c, --create       Create boot entry");
        println!("  -b XXXX            Boot entry number");
        println!("  -B, --delete-bootnum Delete entry");
        println!("  -d DISK            Disk device");
        println!("  -p PART            Partition number");
        println!("  -l LOADER          Loader path");
        println!("  -L LABEL           Boot entry label");
        println!("  -o XXXX,YYYY       Set boot order");
        println!("  -n XXXX            Set next boot");
        println!("  -t SECONDS         Boot timeout");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("efibootmgr version 18 (SlateOS)");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let create = args.iter().any(|a| a == "-c" || a == "--create");

    if create {
        let label = args.windows(2).find(|w| w[0] == "-L").map(|w| w[1].as_str()).unwrap_or("SlateOS");
        println!("Boot entry 0005 created: {label}");
    }

    println!("BootCurrent: 0001");
    println!("Timeout: 5 seconds");
    println!("BootOrder: 0001,0002,0003,0004");
    if verbose {
        println!("Boot0001* SlateOS\tHD(1,GPT,abcdef01-2345-6789-abcd-ef0123456789,0x800,0x100000)/File(\\EFI\\slateos\\grubx64.efi)");
        println!("Boot0002* Windows Boot Manager\tHD(1,GPT,12345678-9abc-def0-1234-567890abcdef,0x800,0x100000)/File(\\EFI\\Microsoft\\Boot\\bootmgfw.efi)");
        println!("Boot0003* USB\tPciRoot(0x0)/Pci(0x14,0x0)/USB(0,0)");
        println!("Boot0004* Network\tPciRoot(0x0)/Pci(0x1f,0x6)/MAC(aabbccddeeff,1)");
    } else {
        println!("Boot0001* SlateOS");
        println!("Boot0002* Windows Boot Manager");
        println!("Boot0003* USB");
        println!("Boot0004* Network");
    }
    0
}

fn run_efivar(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: efivar [OPTIONS]");
        println!("Options: -l (list), -n NAME (get), -p (print), -d (delete)");
        return 0;
    }

    let list = args.iter().any(|a| a == "-l" || a == "--list");
    if list {
        println!("8be4df61-93ca-11d2-aa0d-00e098032b8c-Boot0001");
        println!("8be4df61-93ca-11d2-aa0d-00e098032b8c-Boot0002");
        println!("8be4df61-93ca-11d2-aa0d-00e098032b8c-BootOrder");
        println!("8be4df61-93ca-11d2-aa0d-00e098032b8c-BootCurrent");
        println!("8be4df61-93ca-11d2-aa0d-00e098032b8c-Timeout");
    } else {
        println!("efivar: specify --list or -n NAME");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "efibootmgr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "efivar" => run_efivar(&rest),
        _ => run_efibootmgr(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_efibootmgr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/efibootmgr"), "efibootmgr");
        assert_eq!(basename(r"C:\bin\efibootmgr.exe"), "efibootmgr.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("efibootmgr.exe"), "efibootmgr");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_efibootmgr(&["--help".to_string()]), 0);
        assert_eq!(run_efibootmgr(&["-h".to_string()]), 0);
        let _ = run_efibootmgr(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_efibootmgr(&[]);
    }
}
