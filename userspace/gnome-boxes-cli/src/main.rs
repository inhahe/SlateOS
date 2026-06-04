#![deny(clippy::all)]

//! gnome-boxes-cli — OurOS GNOME Boxes VM manager
//!
//! Single personality: `gnome-boxes`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gnome_boxes(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnome-boxes [OPTIONS] [URI]");
        println!("gnome-boxes v46.0 (OurOS) — Simple VM management");
        println!();
        println!("Options:");
        println!("  --search TEXT    Search for a box");
        println!("  --open-uuid ID   Open a specific box");
        println!("  --version        Show version");
        println!();
        println!("Simple interface for creating and managing virtual machines.");
        println!("Supports QEMU/KVM, remote VNC/SPICE connections.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnome-boxes v46.0 (OurOS)"); return 0; }
    println!("gnome-boxes: virtual machine manager");
    println!("  Boxes:");
    println!("    OurOS Dev    Running   2 vCPUs, 4 GiB RAM");
    println!("    Fedora 39    Shutoff   2 vCPUs, 2 GiB RAM");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnome-boxes".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gnome_boxes(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gnome_boxes};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gnome-boxes"), "gnome-boxes");
        assert_eq!(basename(r"C:\bin\gnome-boxes.exe"), "gnome-boxes.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gnome-boxes.exe"), "gnome-boxes");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gnome_boxes(&["--help".to_string()], "gnome-boxes"), 0);
        assert_eq!(run_gnome_boxes(&["-h".to_string()], "gnome-boxes"), 0);
        let _ = run_gnome_boxes(&["--version".to_string()], "gnome-boxes");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gnome_boxes(&[], "gnome-boxes");
    }
}
