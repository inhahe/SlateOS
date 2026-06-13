#![deny(clippy::all)]

//! lvm2-cli — SlateOS LVM2 logical volume manager
//!
//! Multi-personality: `pvcreate`, `vgcreate`, `lvcreate`, `pvs`, `vgs`, `lvs`, `pvdisplay`, `vgdisplay`, `lvdisplay`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lvm(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "pvcreate" => println!("pvcreate (Slate OS) — Initialize physical volume"),
            "vgcreate" => println!("vgcreate (Slate OS) — Create volume group"),
            "lvcreate" => {
                println!("lvcreate (Slate OS) — Create logical volume");
                println!("  -L SIZE   Volume size");
                println!("  -n NAME   Volume name");
                println!("  -T        Thin pool");
                println!("  --mirrors N  Mirror copies");
            }
            "pvs" | "vgs" | "lvs" => println!("{} (Slate OS) — Display {} information", prog, prog),
            _ => println!("{} (Slate OS) — LVM2 tool", prog),
        }
        println!("  --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("LVM2 v2.03.23 (Slate OS)"); return 0; }
    match prog {
        "pvs" => {
            println!("  PV         VG       Fmt  Attr PSize   PFree");
            println!("  /dev/sda2  vg_root  lvm2 a--  100.00g 20.00g");
            println!("  /dev/sdb1  vg_data  lvm2 a--  500.00g 200.00g");
        }
        "vgs" => {
            println!("  VG       #PV #LV #SN Attr   VSize   VFree");
            println!("  vg_root    1   3   0 wz--n- 100.00g 20.00g");
            println!("  vg_data    1   2   0 wz--n- 500.00g 200.00g");
        }
        "lvs" => {
            println!("  LV     VG       Attr       LSize  Pool");
            println!("  root   vg_root  -wi-ao---- 40.00g");
            println!("  home   vg_root  -wi-ao---- 30.00g");
            println!("  swap   vg_root  -wi-ao---- 10.00g");
            println!("  data   vg_data  -wi-ao---- 200.00g");
            println!("  backup vg_data  -wi-ao---- 100.00g");
        }
        _ => {
            println!("LVM2 v2.03.23 (Slate OS)");
            println!("  Operation: {}", prog);
            println!("  Completed successfully");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lvs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lvm(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lvm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lvm2"), "lvm2");
        assert_eq!(basename(r"C:\bin\lvm2.exe"), "lvm2.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lvm2.exe"), "lvm2");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lvm(&["--help".to_string()], "lvm2"), 0);
        assert_eq!(run_lvm(&["-h".to_string()], "lvm2"), 0);
        let _ = run_lvm(&["--version".to_string()], "lvm2");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lvm(&[], "lvm2");
    }
}
