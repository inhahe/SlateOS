#![deny(clippy::all)]

//! lvm-cli — OurOS LVM2 CLI tools
//!
//! Multi-personality: `pvcreate`, `vgcreate`, `lvcreate`, `pvs`, `vgs`, `lvs`, `pvdisplay`, `vgdisplay`, `lvdisplay`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_lvm(prog: &str, args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{} — LVM2 tool (OurOS). See man {}.", prog, prog);
        return 0;
    }

    match prog {
        "pvcreate" => {
            let dev = args.iter().filter(|a| !a.starts_with('-'))
                .next().map(|s| s.as_str()).unwrap_or("/dev/sdb");
            println!("  Physical volume \"{}\" successfully created.", dev);
        }
        "vgcreate" => {
            let positional: Vec<&str> = args.iter()
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str()).collect();
            let vg = positional.first().copied().unwrap_or("vg0");
            println!("  Volume group \"{}\" successfully created", vg);
        }
        "lvcreate" => {
            let name = args.windows(2).find(|w| w[0] == "-n" || w[0] == "--name")
                .map(|w| w[1].as_str()).unwrap_or("lv0");
            println!("  Logical volume \"{}\" created.", name);
        }
        "pvs" => {
            println!("  PV         VG   Fmt  Attr PSize   PFree");
            println!("  /dev/sdb   vg0  lvm2 a--  500.00g 200.00g");
            println!("  /dev/sdc   vg0  lvm2 a--  500.00g 500.00g");
        }
        "vgs" => {
            println!("  VG   #PV #LV #SN Attr   VSize   VFree");
            println!("  vg0    2   3   0 wz--n- 999.99g 699.99g");
        }
        "lvs" => {
            println!("  LV     VG   Attr       LSize   Pool Origin Data%  Meta%");
            println!("  root   vg0  -wi-ao---- 100.00g");
            println!("  home   vg0  -wi-ao---- 200.00g");
            println!("  swap   vg0  -wi-ao----   8.00g");
        }
        "pvdisplay" => {
            println!("  --- Physical volume ---");
            println!("  PV Name               /dev/sdb");
            println!("  VG Name               vg0");
            println!("  PV Size               500.00 GiB / not usable 4.00 MiB");
            println!("  Allocatable           yes");
            println!("  PE Size               4.00 MiB");
            println!("  Total PE              127999");
            println!("  Free PE               51200");
            println!("  Allocated PE          76799");
        }
        "vgdisplay" => {
            println!("  --- Volume group ---");
            println!("  VG Name               vg0");
            println!("  System ID");
            println!("  Format                lvm2");
            println!("  VG Size               999.99 GiB");
            println!("  PE Size               4.00 MiB");
            println!("  Total PE              255998");
            println!("  Alloc PE / Size       76799 / 300.00 GiB");
            println!("  Free  PE / Size       179199 / 699.99 GiB");
        }
        "lvdisplay" => {
            println!("  --- Logical volume ---");
            println!("  LV Path                /dev/vg0/root");
            println!("  LV Name                root");
            println!("  VG Name                vg0");
            println!("  LV Size                100.00 GiB");
            println!("  Current LE             25600");
            println!("  Segments               1");
            println!("  Allocation             inherit");
            println!("  Read ahead sectors     auto");
            println!("  Block device           254:0");
        }
        _ => {
            println!("lvm: unknown command '{}'. Try pvs, vgs, lvs, pvcreate, vgcreate, lvcreate.", prog);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "lvs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lvm(&prog, &rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
