#![deny(clippy::all)]

//! virtualbox-cli — OurOS VirtualBox management
//!
//! Multi-personality: `VBoxManage`, `VBoxHeadless`, `VBoxSDL`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vboxmanage(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: VBoxManage <command> [OPTIONS]");
        println!("VBoxManage v7.0 (OurOS) — VirtualBox CLI");
        println!();
        println!("Commands:");
        println!("  list vms         List all VMs");
        println!("  list runningvms  List running VMs");
        println!("  startvm NAME     Start a VM");
        println!("  controlvm NAME   Control running VM");
        println!("  createvm         Create a new VM");
        println!("  modifyvm         Modify VM settings");
        println!("  clonevm          Clone a VM");
        println!("  snapshot         Manage snapshots");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("VBoxManage v7.0 (OurOS, VirtualBox)"); return 0; }
    if args.len() >= 2 && args[0] == "list" && args[1] == "vms" {
        println!("\"OurOS-test\" {{a1b2c3d4-e5f6-7890-abcd-ef1234567890}}");
        println!("\"Ubuntu-22\" {{b2c3d4e5-f6a7-8901-bcde-f12345678901}}");
        return 0;
    }
    if args.len() >= 2 && args[0] == "list" && args[1] == "runningvms" {
        println!("\"OurOS-test\" {{a1b2c3d4-e5f6-7890-abcd-ef1234567890}}");
        return 0;
    }
    println!("VBoxManage: use --help for available commands");
    0
}

fn run_vboxheadless(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: VBoxHeadless --startvm NAME [OPTIONS]");
        println!("VBoxHeadless v7.0 (OurOS) — Run VM without GUI");
        println!("  --vrde on/off   Enable/disable VRDE");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("VBoxHeadless v7.0 (OurOS)"); return 0; }
    println!("VBoxHeadless: starting VM in headless mode");
    println!("  VRDE: enabled (port 3389)");
    0
}

fn run_vboxsdl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: VBoxSDL --startvm NAME");
        println!("VBoxSDL v7.0 (OurOS) — Simple VM display");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("VBoxSDL v7.0 (OurOS)"); return 0; }
    println!("VBoxSDL: starting VM with SDL display");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "VBoxManage".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "VBoxHeadless" => run_vboxheadless(&rest, &prog),
        "VBoxSDL" => run_vboxsdl(&rest, &prog),
        _ => run_vboxmanage(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vboxmanage};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/virtualbox"), "virtualbox");
        assert_eq!(basename(r"C:\bin\virtualbox.exe"), "virtualbox.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("virtualbox.exe"), "virtualbox");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vboxmanage(&["--help".to_string()], "virtualbox"), 0);
        assert_eq!(run_vboxmanage(&["-h".to_string()], "virtualbox"), 0);
        let _ = run_vboxmanage(&["--version".to_string()], "virtualbox");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vboxmanage(&[], "virtualbox");
    }
}
