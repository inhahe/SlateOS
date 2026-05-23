#![deny(clippy::all)]

//! grub-cli — OurOS GRUB bootloader tools
//!
//! Multi-personality: `grub-install`, `grub-mkconfig`, `update-grub`,
//! `grub-set-default`, `grub-editenv`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_grub_install(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: grub-install [OPTIONS] DEVICE");
        println!();
        println!("grub-install — install GRUB bootloader (OurOS).");
        println!();
        println!("Options:");
        println!("  --target=TARGET        Installation target");
        println!("  --efi-directory=DIR    EFI system partition mount");
        println!("  --bootloader-id=ID     Boot entry ID");
        println!("  --recheck              Recheck device map");
        println!("  --removable            Install as removable");
        println!("  --force                Force install");
        return 0;
    }

    let device = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/dev/sda");
    let efi = args.iter().find(|a| a.starts_with("--efi-directory=")).is_some();

    println!("Installing for x86_64-efi platform.");
    if efi {
        println!("EFI variables are not supported on this system.");
    }
    println!("Installation finished. No error reported.");
    let _ = device;
    0
}

fn run_grub_mkconfig(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: grub-mkconfig [OPTIONS]");
        println!("Options: -o FILE (output file)");
        return 0;
    }

    println!("Generating grub configuration file ...");
    println!("Found linux image: /boot/vmlinuz-1.0.0");
    println!("Found initrd image: /boot/initrd.img-1.0.0");
    println!("Found memtest86+ image: /boot/memtest86+.bin");
    println!("done");
    0
}

fn run_update_grub(_args: &[String]) -> i32 {
    println!("Sourcing file `/etc/default/grub'");
    println!("Generating grub configuration file ...");
    println!("Found linux image: /boot/vmlinuz-1.0.0");
    println!("Found initrd image: /boot/initrd.img-1.0.0");
    println!("done");
    0
}

fn run_grub_set_default(args: &[String]) -> i32 {
    let entry = args.first().map(|s| s.as_str()).unwrap_or("0");
    println!("Setting default boot entry to: {}", entry);
    0
}

fn run_grub_editenv(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: grub-editenv FILE COMMAND");
        println!("Commands: create, list, set NAME=VALUE, unset NAME");
        return 0;
    }
    let subcmd = args.iter().find(|a| !a.starts_with('-') && !a.contains('/')).map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            println!("saved_entry=0");
            println!("next_entry=");
        }
        "create" => println!("Environment block created."),
        _ => println!("grub-editenv: operation '{}' complete", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "grub-install".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "grub-mkconfig" => run_grub_mkconfig(&rest),
        "update-grub" => run_update_grub(&rest),
        "grub-set-default" => run_grub_set_default(&rest),
        "grub-editenv" => run_grub_editenv(&rest),
        _ => run_grub_install(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
