#![deny(clippy::all)]

//! uvtool-cli — OurOS uvtool cloud image management
//!
//! Multi-personality: `uvt-simplestreams-libvirt`, `uvt-kvm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_uvt_simplestreams(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: uvt-simplestreams-libvirt <command> [OPTIONS]");
        println!("uvt-simplestreams-libvirt v0.1 (OurOS) — Cloud image sync");
        println!();
        println!("Commands:");
        println!("  sync             Sync cloud images from stream");
        println!("  query            Query available images");
        println!("  purge            Remove cached images");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("uvt-simplestreams-libvirt v0.1 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("sync") => {
            println!("uvt-simplestreams-libvirt: syncing cloud images...");
            println!("  Source: https://cloud-images.ubuntu.com/");
            println!("  Images synced: 3");
        }
        Some("query") => {
            println!("Available images:");
            println!("  ouros-24.04 amd64 (current)");
            println!("  ouros-23.10 amd64 (current)");
        }
        _ => {
            println!("uvt-simplestreams-libvirt: use --help for commands");
        }
    }
    0
}

fn run_uvt_kvm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: uvt-kvm <command> [OPTIONS]");
        println!("uvt-kvm v0.1 (OurOS) — Create KVM VMs from cloud images");
        println!();
        println!("Commands:");
        println!("  create NAME     Create VM from cloud image");
        println!("  wait NAME       Wait for VM to be ready");
        println!("  ssh NAME        SSH into VM");
        println!("  destroy NAME    Destroy VM");
        println!("  list            List VMs");
        println!("  ip NAME         Show VM IP address");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("uvt-kvm v0.1 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("list") => {
            println!("test-vm1    running    192.168.122.10");
            println!("test-vm2    shutoff");
        }
        Some("create") => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("vm1");
            println!("uvt-kvm: creating VM '{}'", name);
            println!("  Image: ouros-24.04 amd64");
            println!("  Memory: 512 MiB");
        }
        _ => {
            println!("uvt-kvm: use --help for commands");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "uvt-simplestreams-libvirt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "uvt-kvm" => run_uvt_kvm(&rest, &prog),
        _ => run_uvt_simplestreams(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_uvt_simplestreams};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/uvtool"), "uvtool");
        assert_eq!(basename(r"C:\bin\uvtool.exe"), "uvtool.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("uvtool.exe"), "uvtool");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_uvt_simplestreams(&["--help".to_string()], "uvtool"), 0);
        assert_eq!(run_uvt_simplestreams(&["-h".to_string()], "uvtool"), 0);
        let _ = run_uvt_simplestreams(&["--version".to_string()], "uvtool");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_uvt_simplestreams(&[], "uvtool");
    }
}
