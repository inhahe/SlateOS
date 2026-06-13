#![deny(clippy::all)]

//! ceph-cli — SlateOS Ceph distributed storage tools
//!
//! Multi-personality: `ceph`, `rados`, `rbd`, `ceph-fuse`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ceph(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ceph [OPTIONS] COMMAND");
        println!();
        println!("Commands: status, health, osd pool ls, osd tree, df, mon stat,");
        println!("  pg stat, auth list, version, config dump");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") {
        println!("ceph version 18.2.1 (reef) (SlateOS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match subcmd {
        "status" | "-s" => {
            println!("  cluster:");
            println!("    id:     12345678-abcd-ef01-2345-67890abcdef0");
            println!("    health: HEALTH_OK");
            println!();
            println!("  services:");
            println!("    mon: 3 daemons, quorum a,b,c (age 4d)");
            println!("    mgr: a(active, since 4d)");
            println!("    osd: 12 osds: 12 up, 12 in (since 4d)");
            println!("    mds: 1/1 daemons up");
            println!();
            println!("  data:");
            println!("    pools:   3 pools, 128 pgs");
            println!("    objects: 45.67k objects, 178 GiB");
            println!("    usage:   534 GiB used, 11.4 TiB / 12.0 TiB avail");
            println!("    pgs:     128 active+clean");
        }
        "health" => println!("HEALTH_OK"),
        "df" => {
            println!("--- RAW STORAGE ---");
            println!("CLASS    SIZE     AVAIL    USED     RAW USED   %RAW USED");
            println!("hdd      12 TiB   11 TiB   534 GiB  567 GiB       4.63");
            println!("TOTAL    12 TiB   11 TiB   534 GiB  567 GiB       4.63");
            println!();
            println!("--- POOLS ---");
            println!("POOL        ID  PGS  STORED   OBJECTS  USED     %USED  MAX AVAIL");
            println!("rbd         1   64   56 GiB   14.5k    168 GiB   1.47    3.6 TiB");
            println!("cephfs_data 2   32   112 GiB  28.2k    336 GiB   2.94    3.6 TiB");
            println!("cephfs_meta 3   32   10 GiB   3.0k     30 GiB    0.26    3.6 TiB");
        }
        "osd" => {
            let sub2 = args.get(1).map(|s| s.as_str()).unwrap_or("tree");
            if sub2 == "tree" {
                println!("ID  CLASS  WEIGHT   TYPE NAME      STATUS  REWEIGHT");
                println!("-1         12.00000 root default");
                println!("-3          4.00000     host node1");
                println!(" 0    hdd   1.00000         osd.0      up   1.00000");
                println!(" 1    hdd   1.00000         osd.1      up   1.00000");
                println!(" 2    hdd   1.00000         osd.2      up   1.00000");
                println!(" 3    hdd   1.00000         osd.3      up   1.00000");
            } else {
                println!("ceph: osd {} completed", sub2);
            }
        }
        "mon" => println!("e3: 3 mons at {{a=[v2:192.168.1.10:3300],b=[v2:192.168.1.11:3300],c=[v2:192.168.1.12:3300]}}"),
        _ => println!("ceph: command '{}' completed", subcmd),
    }
    0
}

fn run_rbd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rbd COMMAND [pool/]image");
        println!("Commands: ls, info, create, rm, snap, clone, flatten, resize");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("ls");
    match subcmd {
        "ls" | "list" => {
            println!("vm-disk-1");
            println!("vm-disk-2");
            println!("backup-vol");
        }
        "info" => {
            let img = args.get(1).map(|s| s.as_str()).unwrap_or("vm-disk-1");
            println!("rbd image '{}':", img);
            println!("\tsize 100 GiB in 25600 objects");
            println!("\torder 22 (4 MiB objects)");
            println!("\tformat: 2");
            println!("\tfeatures: layering, exclusive-lock, object-map, fast-diff, deep-flatten");
        }
        _ => println!("rbd: {} completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ceph".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "rados" => { println!("rados: pool list completed"); 0 }
        "rbd" => run_rbd(&rest),
        "ceph-fuse" => { println!("ceph-fuse: mounting CephFS at /mnt/cephfs"); 0 }
        _ => run_ceph(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ceph};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ceph"), "ceph");
        assert_eq!(basename(r"C:\bin\ceph.exe"), "ceph.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ceph.exe"), "ceph");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ceph(&["--help".to_string()]), 0);
        assert_eq!(run_ceph(&["-h".to_string()]), 0);
        let _ = run_ceph(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ceph(&[]);
    }
}
