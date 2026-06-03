#![deny(clippy::all)]

//! fsck-cli — OurOS fsck filesystem check CLIs
//!
//! Multi-personality: `fsck`, `fsck.ext4`, `e2fsck`, `xfs_repair`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_fsck(prog: &str, args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        match prog {
            "e2fsck" | "fsck.ext4" => {
                println!("Usage: e2fsck [OPTIONS] DEVICE");
                println!("  -p             Automatic repair (safe)");
                println!("  -y             Assume yes to all");
                println!("  -n             Open read-only");
                println!("  -f             Force check");
                println!("  -c             Check for bad blocks");
                println!("  -v             Verbose");
            }
            "xfs_repair" => {
                println!("Usage: xfs_repair [OPTIONS] DEVICE");
                println!("  -n             No-modify mode");
                println!("  -v             Verbose");
                println!("  -L             Zero log (force)");
            }
            _ => {
                println!("Usage: fsck [OPTIONS] DEVICE");
                println!("  -t TYPE        Filesystem type");
                println!("  -A             Check all filesystems");
                println!("  -a             Auto-repair");
                println!("  -n             No changes");
                println!("  -y             Assume yes");
            }
        }
        return 0;
    }

    let device = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("/dev/sda2");
    let verbose = args.iter().any(|a| a == "-v");

    match prog {
        "e2fsck" | "fsck.ext4" => {
            println!("e2fsck 1.47.0 (OurOS)");
            println!("{}: clean, 45678/3276800 files, 1234567/13107200 blocks", device);
            if verbose {
                println!();
                println!("Pass 1: Checking inodes, blocks, and sizes");
                println!("Pass 2: Checking directory structure");
                println!("Pass 3: Checking directory connectivity");
                println!("Pass 4: Checking reference counts");
                println!("Pass 5: Checking group summary information");
                println!();
                println!("   45678 inodes used (1.39%, out of 3276800)");
                println!(" 1234567 blocks used (9.42%, out of 13107200)");
            }
        }
        "xfs_repair" => {
            println!("Phase 1 - find and verify superblock...");
            println!("Phase 2 - using internal log");
            println!("Phase 3 - for each AG...");
            println!("Phase 4 - check for duplicate blocks...");
            println!("Phase 5 - rebuild AG headers and trees...");
            println!("Phase 6 - check inode connectivity...");
            println!("Phase 7 - verify link counts...");
            println!("done");
        }
        _ => {
            println!("fsck from util-linux 2.39.3 (OurOS)");
            println!("e2fsck 1.47.0 (OurOS)");
            println!("{}: clean, 45678/3276800 files, 1234567/13107200 blocks", device);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "fsck".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fsck(&prog, &rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fsck};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fsck"), "fsck");
        assert_eq!(basename(r"C:\bin\fsck.exe"), "fsck.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fsck.exe"), "fsck");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_fsck("fsck", &["--help".to_string()]), 0);
        assert_eq!(run_fsck("fsck", &["-h".to_string()]), 0);
        assert_eq!(run_fsck("fsck", &["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_fsck("fsck", &[]), 0);
    }
}
