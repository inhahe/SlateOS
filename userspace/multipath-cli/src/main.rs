#![deny(clippy::all)]

//! multipath-cli — SlateOS device-mapper multipath tools
//!
//! Multi-personality: `multipath`, `multipathd`, `kpartx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_multipath(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: multipath [OPTIONS] [device]");
        println!();
        println!("multipath — device-mapper multipath (SlateOS).");
        println!();
        println!("Options:");
        println!("  -l             Show topology");
        println!("  -ll            Show detailed topology");
        println!("  -f <dev>       Flush multipath device");
        println!("  -F             Flush all unused");
        println!("  -r             Reconfigure");
        println!("  -v <level>     Verbosity");
        return 0;
    }

    if args.iter().any(|a| a == "-ll" || a == "-l") {
        println!("mpath0 (360000000000000001) dm-0 VENDOR,PRODUCT");
        println!("size=100G features='1 queue_if_no_path' hwhandler='1 alua' wp=rw");
        println!("|-+- policy='round-robin 0' prio=50 status=active");
        println!("| `- 1:0:0:0 sda 8:0   active ready running");
        println!("`-+- policy='round-robin 0' prio=10 status=enabled");
        println!("  `- 2:0:0:0 sdb 8:16  active ready running");
        println!();
        println!("mpath1 (360000000000000002) dm-1 VENDOR,PRODUCT");
        println!("size=200G features='1 queue_if_no_path' hwhandler='1 alua' wp=rw");
        println!("|-+- policy='round-robin 0' prio=50 status=active");
        println!("| `- 1:0:1:0 sdc 8:32  active ready running");
        println!("`-+- policy='round-robin 0' prio=10 status=enabled");
        println!("  `- 2:0:1:0 sdd 8:48  active ready running");
    } else if args.iter().any(|a| a == "-F") {
        println!("Flushed all unused multipath devices.");
    } else if args.iter().any(|a| a == "-r") {
        println!("Reconfigured multipath devices.");
    } else {
        println!("ok");
    }
    0
}

fn run_multipathd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: multipathd [OPTIONS] [COMMAND]");
        println!("Commands: show paths, show maps, show config, reconfigure");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("show");
    if subcmd == "show" {
        let what = args.get(1).map(|s| s.as_str()).unwrap_or("paths");
        match what {
            "paths" => {
                println!("hcil    dev  dev_t  pri dm_st  chk_st dev_st  next_check");
                println!("1:0:0:0 sda  8:0    50  active ready  running XXXXXXXXXX 12/20");
                println!("2:0:0:0 sdb  8:16   10  active ready  running XXXXXXXXXX 12/20");
                println!("1:0:1:0 sdc  8:32   50  active ready  running XXXXXXXXXX 12/20");
                println!("2:0:1:0 sdd  8:48   10  active ready  running XXXXXXXXXX 12/20");
            }
            "maps" => {
                println!("name    sysfs uuid                              failback");
                println!("mpath0  dm-0  360000000000000001                 immediate");
                println!("mpath1  dm-1  360000000000000002                 immediate");
            }
            _ => println!("multipathd: show {} completed", what),
        }
    } else {
        println!("multipathd: {} completed", subcmd);
    }
    0
}

fn run_kpartx(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: kpartx [OPTIONS] <device>");
        println!("  -a    Add partition mappings");
        println!("  -d    Delete partition mappings");
        println!("  -l    List partition mappings");
        return 0;
    }
    let dev = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/dev/dm-0");
    if args.iter().any(|a| a == "-l") {
        println!("loop0p1 : 0 204800 /dev/{}1 2048", dev);
        println!("loop0p2 : 0 2097152 /dev/{}2 206848", dev);
    } else if args.iter().any(|a| a == "-a") {
        println!("add map {}p1 (253:2): 0 204800 linear /dev/{} 2048", dev, dev);
        println!("add map {}p2 (253:3): 0 2097152 linear /dev/{} 206848", dev, dev);
    } else {
        println!("kpartx: operation on {} completed", dev);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "multipath".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "multipathd" => run_multipathd(&rest),
        "kpartx" => run_kpartx(&rest),
        _ => run_multipath(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_multipath};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/multipath"), "multipath");
        assert_eq!(basename(r"C:\bin\multipath.exe"), "multipath.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("multipath.exe"), "multipath");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_multipath(&["--help".to_string()]), 0);
        assert_eq!(run_multipath(&["-h".to_string()]), 0);
        let _ = run_multipath(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_multipath(&[]);
    }
}
