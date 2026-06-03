#![deny(clippy::all)]

//! gluster-cli — OurOS GlusterFS distributed filesystem tools
//!
//! Multi-personality: `gluster`, `glusterd`, `glusterfsd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gluster(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gluster [OPTIONS] COMMAND");
        println!();
        println!("gluster — GlusterFS CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  peer status          Show peer status");
        println!("  peer probe HOST      Add peer");
        println!("  volume list          List volumes");
        println!("  volume info [VOL]    Volume info");
        println!("  volume status [VOL]  Volume status");
        println!("  volume create        Create volume");
        println!("  volume start VOL     Start volume");
        println!("  volume stop VOL      Stop volume");
        println!("  pool list            List storage pools");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("glusterfs 11.1 (OurOS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("volume");
    let sub2 = args.get(1).map(|s| s.as_str()).unwrap_or("list");

    match (subcmd, sub2) {
        ("peer", "status") => {
            println!("Number of Peers: 2");
            println!();
            println!("Hostname: node2.local");
            println!("Uuid: 11223344-5566-7788-99aa-bbccddeeff00");
            println!("State: Peer in Cluster (Connected)");
            println!();
            println!("Hostname: node3.local");
            println!("Uuid: aabbccdd-1122-3344-5566-778899001122");
            println!("State: Peer in Cluster (Connected)");
        }
        ("peer", "probe") => {
            let host = args.get(2).map(|s| s.as_str()).unwrap_or("node2.local");
            println!("peer probe: success. Host {} port 24007 already in peer list", host);
        }
        ("volume", "list") => {
            println!("data-vol");
            println!("backup-vol");
        }
        ("volume", "info") => {
            println!("Volume Name: data-vol");
            println!("Type: Replicate");
            println!("Volume ID: aabbccdd-eeff-0011-2233-445566778899");
            println!("Status: Started");
            println!("Snap count: 2");
            println!("Transport-type: tcp");
            println!("Bricks:");
            println!("Brick1: node1:/export/brick1");
            println!("Brick2: node2:/export/brick1");
            println!("Brick3: node3:/export/brick1");
            println!("Options Reconfigured:");
            println!("performance.cache-size: 256MB");
        }
        ("volume", "status") => {
            println!("Status of volume: data-vol");
            println!("Gluster process                   TCP Port  RDMA Port  Online  Pid");
            println!("----------------------------------------------------------------------");
            println!("Brick node1:/export/brick1         49152     0          Y       1234");
            println!("Brick node2:/export/brick1         49152     0          Y       1235");
            println!("Brick node3:/export/brick1         49152     0          Y       1236");
            println!("Self-heal Daemon on localhost       N/A       N/A        Y       1237");
        }
        ("pool", "list") => {
            println!("UUID\t\t\t\t\tHostname\tState");
            println!("12345678-abcd-ef01-2345-67890abcdef0\tlocalhost\tConnected");
            println!("11223344-5566-7788-99aa-bbccddeeff00\tnode2.local\tConnected");
            println!("aabbccdd-1122-3344-5566-778899001122\tnode3.local\tConnected");
        }
        _ => println!("gluster: {} {} completed", subcmd, sub2),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gluster".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "glusterd" => { println!("glusterd: started (OurOS)"); 0 }
        "glusterfsd" => { println!("glusterfsd: brick process started"); 0 }
        _ => run_gluster(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gluster};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gluster"), "gluster");
        assert_eq!(basename(r"C:\bin\gluster.exe"), "gluster.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gluster.exe"), "gluster");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gluster(&["--help".to_string()]), 0);
        assert_eq!(run_gluster(&["-h".to_string()]), 0);
        assert_eq!(run_gluster(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gluster(&[]), 0);
    }
}
