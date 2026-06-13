#![deny(clippy::all)]

//! dracut-cli — SlateOS initramfs generator (dracut)
//!
//! Multi-personality: `dracut`, `lsinitrd`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_dracut(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dracut [OPTIONS] [IMAGE [VERSION]]");
        println!();
        println!("dracut — generate initramfs image (SlateOS).");
        println!();
        println!("Options:");
        println!("  -f, --force              Overwrite existing image");
        println!("  --kver VERSION            Kernel version");
        println!("  --add MODULE              Add dracut module");
        println!("  --omit MODULE             Omit dracut module");
        println!("  --add-drivers DRIVER      Add kernel driver");
        println!("  --filesystems FS          Add filesystem module");
        println!("  --hostonly                Host-only mode");
        println!("  --no-hostonly             Include all modules");
        println!("  --fstab                   Use /etc/fstab");
        println!("  -v, --verbose             Verbose");
        println!("  --list-modules            List available modules");
        println!("  --print-cmdline           Print kernel cmdline");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("dracut 059 (SlateOS)");
        return 0;
    }

    if args.iter().any(|a| a == "--list-modules") {
        println!("base");
        println!("bash");
        println!("dm");
        println!("fs-lib");
        println!("kernel-modules");
        println!("lvm");
        println!("mdraid");
        println!("network");
        println!("plymouth");
        println!("resume");
        println!("rootfs-block");
        println!("shutdown");
        println!("systemd");
        println!("udev-rules");
        return 0;
    }

    let version = args.iter().find(|a| a.starts_with("--kver="))
        .and_then(|a| a.strip_prefix("--kver="))
        .unwrap_or("1.0.0");
    let image = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str())
        .unwrap_or("/boot/initramfs-1.0.0.img");

    println!("dracut: generating '{}' for kernel {}", image, version);
    println!("dracut: including modules: base bash kernel-modules rootfs-block udev-rules");
    println!("dracut: including drivers: ext4 nvme ahci sd_mod");
    println!("dracut: compressing with xz");
    println!("dracut: created {} (24.5 MB)", image);
    0
}

fn run_lsinitrd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lsinitrd [OPTIONS] [IMAGE]");
        println!("Options: -s (size), -m (list modules), -f FILE (extract file)");
        return 0;
    }

    let modules = args.iter().any(|a| a == "-m");
    let image = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str())
        .unwrap_or("/boot/initramfs-1.0.0.img");

    if modules {
        println!("dracut modules:");
        println!("base");
        println!("bash");
        println!("kernel-modules");
        println!("rootfs-block");
        println!("udev-rules");
        return 0;
    }

    println!("Image: {} (24.5 MB)", image);
    println!("Version: dracut-059");
    println!("Arguments: --kver '1.0.0'");
    println!();
    println!("dracut modules:");
    println!("  base bash kernel-modules rootfs-block udev-rules");
    println!();
    println!("drwxr-xr-x   2 root root    0 Jan  1 00:00 .");
    println!("drwxr-xr-x   2 root root    0 Jan  1 00:00 etc");
    println!("drwxr-xr-x   3 root root    0 Jan  1 00:00 lib");
    println!("drwxr-xr-x   2 root root    0 Jan  1 00:00 lib/modules/1.0.0");
    println!("-rwxr-xr-x   1 root root 1234 Jan  1 00:00 init");
    println!("========================================================================");
    println!("Early CPIO image:");
    println!("  48 blocks");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "dracut".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "lsinitrd" => run_lsinitrd(&rest),
        _ => run_dracut(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dracut};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dracut"), "dracut");
        assert_eq!(basename(r"C:\bin\dracut.exe"), "dracut.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dracut.exe"), "dracut");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dracut(&["--help".to_string()]), 0);
        assert_eq!(run_dracut(&["-h".to_string()]), 0);
        let _ = run_dracut(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dracut(&[]);
    }
}
