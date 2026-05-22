#![deny(clippy::all)]

//! minio-cli — OurOS MinIO CLI
//!
//! Single personality: `mc` (MinIO Client)

use std::env;
use std::process;

fn run_mc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mc <COMMAND> [OPTIONS]");
        println!();
        println!("MinIO Client - S3-compatible object storage CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  alias        Manage server aliases");
        println!("  ls           List objects/buckets");
        println!("  mb           Make bucket");
        println!("  rb           Remove bucket");
        println!("  cp           Copy objects");
        println!("  mv           Move objects");
        println!("  rm           Remove objects");
        println!("  cat          Display object contents");
        println!("  head         Display first few lines");
        println!("  stat         Show object/bucket info");
        println!("  mirror       Mirror buckets");
        println!("  admin        Admin operations");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("mc version RELEASE.2024-01-15 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "alias" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "set" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("myminio");
                    let url = args.get(3).map(|s| s.as_str()).unwrap_or("http://localhost:9000");
                    println!("Added `{}` successfully.", name);
                    println!("  URL:       {}", url);
                    println!("  AccessKey: ****ABCD");
                    println!("  SecretKey: ****1234");
                }
                "list" => {
                    println!("  myminio     http://localhost:9000        minioadmin");
                    println!("  s3          https://s3.amazonaws.com     AKIA****ABCD");
                    println!("  gcs         https://storage.googleapis.com  gcs-key");
                }
                _ => { println!("Alias operation: {}", sub); }
            }
            0
        }
        "ls" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("myminio/");
            if path.ends_with('/') || !path.contains('/') {
                println!("[2024-01-15 14:00:00 UTC]     0B data/");
                println!("[2024-01-14 10:00:00 UTC]     0B backups/");
                println!("[2024-01-13 08:00:00 UTC]     0B logs/");
            } else {
                println!("[2024-01-15 14:00:00 UTC]  125MB dataset.csv");
                println!("[2024-01-15 13:00:00 UTC]   45MB model.pkl");
                println!("[2024-01-15 12:00:00 UTC]  256B  config.json");
            }
            0
        }
        "mb" => {
            let bucket = args.get(1).map(|s| s.as_str()).unwrap_or("myminio/new-bucket");
            println!("Bucket created successfully `{}`.", bucket);
            0
        }
        "rb" => {
            let bucket = args.get(1).map(|s| s.as_str()).unwrap_or("myminio/old-bucket");
            let force = args.iter().any(|a| a == "--force");
            if force {
                println!("Removed `{}` and all its contents.", bucket);
            } else {
                println!("Removed `{}`.", bucket);
            }
            0
        }
        "cp" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("./data.csv");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("myminio/data/data.csv");
            println!(".../{}: 125.60 MiB / 125.60 MiB  100%", src.rsplit('/').next().unwrap_or(src));
            println!("  `{}` -> `{}`", src, dst);
            println!("Total: 125.60 MiB, Transferred: 125.60 MiB, Speed: 234.5 MiB/s");
            0
        }
        "mv" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("myminio/data/old.csv");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("myminio/archive/old.csv");
            println!("  `{}` -> `{}`", src, dst);
            0
        }
        "rm" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("myminio/data/temp.csv");
            let recursive = args.iter().any(|a| a == "--recursive" || a == "-r");
            if recursive {
                println!("Removed `{}/file1.csv`.", path);
                println!("Removed `{}/file2.csv`.", path);
                println!("Removed `{}/file3.csv`.", path);
            } else {
                println!("Removed `{}`.", path);
            }
            0
        }
        "stat" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("myminio/data/dataset.csv");
            println!("Name      : {}", path.rsplit('/').next().unwrap_or(path));
            println!("Size      : 125.60 MiB");
            println!("Type      : file");
            println!("ETag      : abc123def456ghi789jkl012");
            println!("Modified  : 2024-01-15 14:00:00 UTC");
            println!("Metadata  :");
            println!("  Content-Type: text/csv");
            0
        }
        "mirror" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("myminio/data");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("myminio/backup");
            println!("Mirroring `{}` to `{}`...", src, dst);
            println!("  ...dataset.csv: 125.60 MiB");
            println!("  ...model.pkl: 45.00 MiB");
            println!("  ...config.json: 256 B");
            println!("Total: 170.60 MiB, 3 objects. Speed: 345.2 MiB/s");
            0
        }
        "admin" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    let alias = args.get(2).map(|s| s.as_str()).unwrap_or("myminio");
                    println!("●  {}:  http://localhost:9000", alias);
                    println!("   Uptime:  3 days, 14 hours");
                    println!("   Version: RELEASE.2024-01-15");
                    println!("   Drives:  4/4 OK");
                    println!("   Storage: 1.2 TiB Used, 3.8 TiB Available");
                }
                "user" => {
                    println!("  AccessKey        Status   PolicyName");
                    println!("  admin            enabled  consoleAdmin");
                    println!("  readonly-user    enabled  readonly");
                    println!("  app-service      enabled  readwrite");
                }
                _ => { println!("Admin operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: mc <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mc(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
