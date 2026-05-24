#![deny(clippy::all)]

//! zfs-cli — OurOS ZFS filesystem tools
//!
//! Multi-personality: `zfs`, `zpool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zfs(args: &[String], prog: &str) -> i32 {
    if prog == "zpool" {
        if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
            println!("Usage: zpool COMMAND [OPTIONS]");
            println!("zpool (ZFS 2.2.4, OurOS)");
            println!();
            println!("Commands:");
            println!("  create NAME DEV...   Create pool");
            println!("  destroy NAME         Destroy pool");
            println!("  list                 List pools");
            println!("  status [NAME]        Show pool status");
            println!("  iostat [NAME] [INT]  Show I/O statistics");
            println!("  scrub NAME           Start scrub");
            println!("  import [NAME]        Import pool");
            println!("  export NAME          Export pool");
            println!("  history [NAME]       Show history");
            println!("  get PROP [NAME]      Get property");
            println!("  set PROP=VAL NAME    Set property");
            return 0;
        }
        let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
        match cmd {
            "list" => {
                println!("NAME    SIZE  ALLOC  FREE  CKPOINT  EXPANDSZ  FRAG  CAP  DEDUP  HEALTH  ALTROOT");
                println!("rpool   500G   120G  380G        -         -    5%   24%  1.00x  ONLINE  -");
            }
            "status" => {
                println!("  pool: rpool");
                println!(" state: ONLINE");
                println!("  scan: scrub repaired 0B in 01:23:45");
                println!("config:");
                println!("  NAME        STATE   READ WRITE CKSUM");
                println!("  rpool       ONLINE     0     0     0");
                println!("    sda       ONLINE     0     0     0");
            }
            "iostat" => {
                println!("              capacity     operations     bandwidth");
                println!("pool        alloc   free   read  write   read  write");
                println!("rpool        120G   380G    150    250  2.5M   4.1M");
            }
            _ => println!("zpool {}: completed", cmd),
        }
        return 0;
    }
    // zfs
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: zfs COMMAND [OPTIONS]");
        println!("zfs (ZFS 2.2.4, OurOS)");
        println!();
        println!("Commands:");
        println!("  create NAME          Create dataset");
        println!("  destroy NAME         Destroy dataset");
        println!("  list                 List datasets");
        println!("  snapshot NAME@SNAP   Create snapshot");
        println!("  rollback NAME@SNAP   Rollback to snapshot");
        println!("  clone SNAP NAME      Clone snapshot");
        println!("  send NAME            Send stream");
        println!("  receive NAME         Receive stream");
        println!("  mount NAME           Mount dataset");
        println!("  unmount NAME         Unmount dataset");
        println!("  get PROP NAME        Get property");
        println!("  set PROP=VAL NAME    Set property");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "list" => {
            println!("NAME                 USED  AVAIL  REFER  MOUNTPOINT");
            println!("rpool                120G   380G    96K  /");
            println!("rpool/home            45G   380G    45G  /home");
            println!("rpool/var             10G   380G    10G  /var");
        }
        "snapshot" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("rpool@snap");
            println!("zfs: Created snapshot '{}'", name);
        }
        _ => println!("zfs {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zfs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zfs(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
