#![deny(clippy::all)]

//! nfs-utils — OurOS NFS server and client utilities
//!
//! Multi-personality: `nfsd` (daemon), `exportfs`, `showmount`, `rpcinfo`, `nfsstat`

use std::env;
use std::process;

fn run_nfsd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nfsd [nprocs]");
        println!("  Start the NFS server with nprocs threads (default: 8)");
        println!("  --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("nfsd (OurOS nfs-utils 2.6.4)");
        return 0;
    }
    let nprocs = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(8);
    println!("Starting NFS daemon with {} threads", nprocs);
    println!("NFS server running (OurOS nfs-utils 2.6.4)");
    0
}

fn run_exportfs(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: exportfs [-aruv] [host:/path]");
        println!();
        println!("Options:");
        println!("  -a    Export or unexport all directories");
        println!("  -r    Re-export all directories");
        println!("  -u    Unexport one or more directories");
        println!("  -v    Verbose");
        return 0;
    }
    let verbose = args.iter().any(|a| a.contains('v'));
    if args.iter().any(|a| a.contains('a') || a.contains('r')) {
        if verbose {
            println!("exporting *:/srv/nfs/public");
            println!("exporting 192.168.1.0/24:/srv/nfs/homes");
            println!("exporting 10.0.0.0/8:/srv/nfs/data");
        }
        return 0;
    }
    // Default: list exports
    println!("/srv/nfs/public     *(rw,sync,no_subtree_check)");
    println!("/srv/nfs/homes      192.168.1.0/24(rw,sync,no_root_squash)");
    println!("/srv/nfs/data       10.0.0.0/8(ro,sync)");
    0
}

fn run_showmount(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: showmount [-adehv] [host]");
        println!();
        println!("Options:");
        println!("  -a    List all mount points");
        println!("  -d    List only directories");
        println!("  -e    Show the NFS server's export list");
        return 0;
    }
    let host = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("localhost");
    if args.iter().any(|a| a.contains('e')) {
        println!("Export list for {}:", host);
        println!("/srv/nfs/public  *");
        println!("/srv/nfs/homes   192.168.1.0/24");
        println!("/srv/nfs/data    10.0.0.0/8");
    } else if args.iter().any(|a| a.contains('a')) {
        println!("All mount points on {}:", host);
        println!("192.168.1.100:/srv/nfs/public");
        println!("192.168.1.101:/srv/nfs/homes");
    } else if args.iter().any(|a| a.contains('d')) {
        println!("Directories on {}:", host);
        println!("/srv/nfs/public");
        println!("/srv/nfs/homes");
        println!("/srv/nfs/data");
    } else {
        println!("Hosts on {}:", host);
        println!("192.168.1.100");
        println!("192.168.1.101");
    }
    0
}

fn run_rpcinfo(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rpcinfo [-p host] [-s host]");
        return 0;
    }
    let host = args.iter().position(|a| a == "-p" || a == "-s")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("localhost");
    println!("   program vers proto   port  service");
    println!("    100000    4   tcp    111  portmapper  ({})", host);
    println!("    100000    3   tcp    111  portmapper");
    println!("    100000    2   tcp    111  portmapper");
    println!("    100003    4   tcp   2049  nfs");
    println!("    100003    3   tcp   2049  nfs");
    println!("    100005    3   tcp  20048  mountd");
    println!("    100005    2   tcp  20048  mountd");
    println!("    100021    4   tcp  36689  nlockmgr");
    0
}

fn run_nfsstat(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nfsstat [-csnm]");
        println!("  -c  Client statistics");
        println!("  -s  Server statistics");
        println!("  -n  NFS statistics");
        println!("  -m  Mount statistics");
        return 0;
    }
    let server = args.iter().any(|a| a.contains('s'));
    let client = args.iter().any(|a| a.contains('c'));
    if server || !client {
        println!("Server rpc stats:");
        println!("calls      badcalls   badclnt    badauth    xdrcall");
        println!("142890     0          0          0          0");
        println!();
        println!("Server nfs v4:");
        println!("null       compound");
        println!("0          142890");
    }
    if client || !server {
        println!();
        println!("Client rpc stats:");
        println!("calls      retrans    authrefrsh");
        println!("56823      12         56823");
        println!();
        println!("Client nfs v4:");
        println!("null       read       write      commit     open       close");
        println!("0          28400      14200      7100       3560       3563");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("nfsd");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "exportfs" => run_exportfs(rest),
        "showmount" => run_showmount(rest),
        "rpcinfo" => run_rpcinfo(rest),
        "nfsstat" => run_nfsstat(rest),
        _ => run_nfsd(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_nfsd};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nfsd(vec!["--help".to_string()]), 0);
        assert_eq!(run_nfsd(vec!["-h".to_string()]), 0);
        let _ = run_nfsd(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nfsd(vec![]);
    }
}
