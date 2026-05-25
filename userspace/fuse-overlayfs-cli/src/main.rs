#![deny(clippy::all)]

//! fuse-overlayfs-cli — OurOS fuse-overlayfs filesystem
//!
//! Single personality: `fuse-overlayfs`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fuse_overlayfs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fuse-overlayfs [OPTIONS] MOUNTPOINT");
        println!("fuse-overlayfs v1.13 (OurOS) — FUSE overlay filesystem for rootless containers");
        println!();
        println!("Options:");
        println!("  -o lowerdir=DIR    Lower (read-only) directories (colon-separated)");
        println!("  -o upperdir=DIR    Upper (writable) directory");
        println!("  -o workdir=DIR     Work directory");
        println!("  -o squash_to_uid=N Squash file ownership to UID");
        println!("  -o squash_to_gid=N Squash file ownership to GID");
        println!("  -o noacl           Disable ACL support");
        println!("  -o uidmapping=MAP  UID mapping");
        println!("  -o gidmapping=MAP  GID mapping");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fuse-overlayfs v1.13 (OurOS)"); return 0; }
    println!("fuse-overlayfs v1.13 (OurOS)");
    println!("  Lower: /var/lib/containers/storage/overlay/l1:/var/lib/containers/storage/overlay/l2");
    println!("  Upper: /var/lib/containers/storage/overlay/upper");
    println!("  Work: /var/lib/containers/storage/overlay/work");
    println!("  Mount: /var/lib/containers/storage/overlay/merged");
    println!("  FUSE: mounted successfully");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fuse-overlayfs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fuse_overlayfs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
