#![deny(clippy::all)]

//! mkinitcpio-cli — Slate OS initramfs generator (Arch-style)
//!
//! Multi-personality: `mkinitcpio`, `update-initramfs`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_mkinitcpio(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mkinitcpio [OPTIONS]");
        println!();
        println!("mkinitcpio — generate initramfs images (Slate OS).");
        println!();
        println!("Options:");
        println!("  -g FILE        Generate image to FILE");
        println!("  -k VERSION     Kernel version");
        println!("  -c CONFIG      Config file");
        println!("  -p PRESET      Build from preset");
        println!("  -P             Build all presets");
        println!("  -L             List available hooks");
        println!("  -H HOOK        Help for hook");
        println!("  -n             Dry run");
        println!("  -v             Verbose");
        println!("  -S HOOKS       Skip hooks");
        println!("  -A HOOKS       Add hooks");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("mkinitcpio 37 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "-L") {
        println!("Available hooks:");
        println!("  base         Essential init files");
        println!("  udev         udev device manager");
        println!("  autodetect   Auto-detect needed modules");
        println!("  modconf      Module configuration");
        println!("  block        Block device support");
        println!("  filesystems  Filesystem support");
        println!("  keyboard     Keyboard input support");
        println!("  fsck         Filesystem check");
        println!("  encrypt      LUKS encryption");
        println!("  lvm2         LVM support");
        println!("  resume       Suspend/resume");
        println!("  shutdown     Shutdown hooks");
        return 0;
    }

    let version = args.windows(2).find(|w| w[0] == "-k").map(|w| w[1].as_str()).unwrap_or("1.0.0");
    let output = args.windows(2).find(|w| w[0] == "-g").map(|w| w[1].as_str())
        .unwrap_or("/boot/initramfs-1.0.0.img");

    println!("==> Building image from preset: /etc/mkinitcpio.d/slateos.preset: 'default'");
    println!("  -> -k {} -g {}", version, output);
    println!("==> Starting build: {}", version);
    println!("  -> Running build hook: [base]");
    println!("  -> Running build hook: [udev]");
    println!("  -> Running build hook: [autodetect]");
    println!("  -> Running build hook: [modconf]");
    println!("  -> Running build hook: [block]");
    println!("  -> Running build hook: [filesystems]");
    println!("  -> Running build hook: [keyboard]");
    println!("  -> Running build hook: [fsck]");
    println!("==> Generating module dependencies");
    println!("==> Creating xz-compressed initcpio image: {}", output);
    println!("==> Image generation successful");
    0
}

fn run_update_initramfs(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: update-initramfs [OPTIONS]");
        println!();
        println!("update-initramfs — generate initramfs for given kernel (Slate OS).");
        println!();
        println!("Options:");
        println!("  -c    Create initramfs");
        println!("  -u    Update initramfs");
        println!("  -d    Delete initramfs");
        println!("  -k VERSION   Kernel version (default: current)");
        println!("  -v    Verbose");
        return 0;
    }

    let create = args.iter().any(|a| a == "-c");
    let update = args.iter().any(|a| a == "-u");
    let delete = args.iter().any(|a| a == "-d");
    let version = args.windows(2).find(|w| w[0] == "-k").map(|w| w[1].as_str()).unwrap_or("1.0.0");

    if create {
        println!("update-initramfs: Generating /boot/initrd.img-{}", version);
    } else if update {
        println!("update-initramfs: Updating /boot/initrd.img-{}", version);
    } else if delete {
        println!("update-initramfs: Deleting /boot/initrd.img-{}", version);
    } else {
        println!("update-initramfs: specify -c, -u, or -d");
        return 1;
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "mkinitcpio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "update-initramfs" => run_update_initramfs(&rest),
        _ => run_mkinitcpio(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mkinitcpio};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mkinitcpio"), "mkinitcpio");
        assert_eq!(basename(r"C:\bin\mkinitcpio.exe"), "mkinitcpio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mkinitcpio.exe"), "mkinitcpio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mkinitcpio(&["--help".to_string()]), 0);
        assert_eq!(run_mkinitcpio(&["-h".to_string()]), 0);
        let _ = run_mkinitcpio(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mkinitcpio(&[]);
    }
}
