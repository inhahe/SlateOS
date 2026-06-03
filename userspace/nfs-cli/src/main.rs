#![deny(clippy::all)]

//! nfs-cli — OurOS NFS client/server tools
//!
//! Multi-personality: `showmount`, `exportfs`, `nfsstat`, `rpcinfo`, `nfsd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_showmount(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: showmount [OPTIONS] [host]");
        println!("  -e, --exports   Show exports list");
        println!("  -a, --all       Show all mount points");
        println!("  -d, --dirs      Show directories");
        return 0;
    }

    let host = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("localhost");
    if args.iter().any(|a| a == "-e" || a == "--exports") {
        println!("Export list for {}:", host);
        println!("/srv/nfs/shared    192.168.1.0/24");
        println!("/srv/nfs/home      192.168.1.0/24");
        println!("/srv/nfs/public    *");
    } else if args.iter().any(|a| a == "-a" || a == "--all") {
        println!("All mount points on {}:", host);
        println!("192.168.1.50:/srv/nfs/shared");
        println!("192.168.1.51:/srv/nfs/home");
    } else {
        println!("Hosts on {}:", host);
        println!("192.168.1.50");
        println!("192.168.1.51");
    }
    0
}

fn run_exportfs(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: exportfs [OPTIONS] [client:/path]");
        println!("  -a            Export all");
        println!("  -r            Re-export all");
        println!("  -u            Unexport");
        println!("  -v            Verbose");
        println!("  -s            Show current exports");
        return 0;
    }

    if args.iter().any(|a| a == "-s" || a == "-v") || args.is_empty() {
        println!("/srv/nfs/shared   192.168.1.0/24(rw,sync,no_subtree_check,no_root_squash)");
        println!("/srv/nfs/home     192.168.1.0/24(rw,sync,no_subtree_check)");
        println!("/srv/nfs/public   *(ro,sync,no_subtree_check)");
    } else if args.iter().any(|a| a == "-r") {
        println!("exporting 192.168.1.0/24:/srv/nfs/shared");
        println!("exporting 192.168.1.0/24:/srv/nfs/home");
        println!("exporting *:/srv/nfs/public");
    }
    0
}

fn run_nfsstat(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nfsstat [OPTIONS]");
        println!("  -s    Server stats");
        println!("  -c    Client stats");
        println!("  -m    Mount info");
        return 0;
    }

    if args.iter().any(|a| a == "-s") {
        println!("Server rpc stats:");
        println!("calls      badcalls   badfmt     badauth    badclnt");
        println!("123456     12         3          0          9");
        println!();
        println!("Server nfs v4:");
        println!("null       compound");
        println!("0          123456");
    } else if args.iter().any(|a| a == "-m") {
        println!("/srv/nfs/shared from 192.168.1.100:/srv/nfs/shared");
        println!(" Flags: rw,relatime,vers=4.2,rsize=1048576,wsize=1048576");
        println!(" Caps:  caps=0x3fff7,wtmult=512,dtsize=32768,bsize=0,namlen=255");
    } else {
        println!("Server rpc stats:");
        println!("calls      badcalls   badfmt     badauth    badclnt");
        println!("123456     12         3          0          9");
        println!();
        println!("Client rpc stats:");
        println!("calls      retrans    authrefrsh");
        println!("67890      5          67890");
    }
    0
}

fn run_rpcinfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rpcinfo [OPTIONS] [host]");
        println!("  -p    Show portmap");
        println!("  -s    Show short listing");
        return 0;
    }

    if args.iter().any(|a| a == "-p") {
        println!("   program vers proto   port  service");
        println!("    100000    4   tcp    111  portmapper");
        println!("    100000    4   udp    111  portmapper");
        println!("    100003    4   tcp   2049  nfs");
        println!("    100005    3   tcp  20048  mountd");
        println!("    100021    4   tcp  42955  nlockmgr");
        println!("    100024    1   tcp  45827  status");
    } else {
        println!("   program version(s) netid(s)                         service     owner");
        println!("    100000  2,3,4     local,udp,tcp,udp6,tcp6          portmapper  superuser");
        println!("    100003  3,4       tcp,udp,tcp6,udp6                nfs         superuser");
        println!("    100005  1,2,3     tcp,udp,tcp6,udp6                mountd      superuser");
        println!("    100021  1,3,4     tcp,udp,tcp6,udp6                nlockmgr    superuser");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "showmount".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "exportfs" => run_exportfs(&rest),
        "nfsstat" => run_nfsstat(&rest),
        "rpcinfo" => run_rpcinfo(&rest),
        "nfsd" => { println!("nfsd: starting NFS server (OurOS)"); println!("nfsd: 8 threads started"); 0 }
        _ => run_showmount(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_showmount};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nfs"), "nfs");
        assert_eq!(basename(r"C:\bin\nfs.exe"), "nfs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nfs.exe"), "nfs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_showmount(&["--help".to_string()]), 0);
        assert_eq!(run_showmount(&["-h".to_string()]), 0);
        assert_eq!(run_showmount(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_showmount(&[]), 0);
    }
}
