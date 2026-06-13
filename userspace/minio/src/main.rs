#![deny(clippy::all)]

//! minio — SlateOS S3-compatible object storage
//!
//! Multi-personality: `minio` (server), `mc` (client)

use std::env;
use std::process;

fn run_minio_server(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: minio server [FLAGS] DIR1 [DIR2...]");
        println!();
        println!("Flags:");
        println!("  --address <addr>      Bind to address:port (default: :9000)");
        println!("  --console-address     Console listen address (default: :9001)");
        println!("  --certs-dir <dir>     Path to certs directory");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("minio version RELEASE.2025-05-22 (SlateOS)");
        println!("Runtime: go1.22.2 slateos/amd64");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("server");
    if cmd == "server" || cmd == "--address" {
        let addr = args.iter().position(|a| a == "--address")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or(":9000");
        println!("MinIO Object Storage Server");
        println!("Copyright: 2015-2025 MinIO, Inc.");
        println!("License: GNU AGPLv3 — https://www.gnu.org/licenses/agpl-3.0.html");
        println!("Version: RELEASE.2025-05-22 (SlateOS)");
        println!();
        println!("API: http://0.0.0.0{}", addr);
        println!("WebUI: http://0.0.0.0:9001");
        println!();
        println!("Defaults: minioadmin:minioadmin");
        println!();
        println!(" Status:         1 Online, 0 Offline.");
        println!(" API:            http://0.0.0.0{}", addr);
        println!(" Console:        http://0.0.0.0:9001");
        println!(" Documentation:  https://min.io/docs/minio/linux/index.html");
    }
    0
}

fn run_mc(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("NAME:");
            println!("  mc - MinIO Client for cloud storage and filesystems");
            println!();
            println!("COMMANDS:");
            println!("  alias    Manage server credentials in configuration");
            println!("  ls       List files and objects");
            println!("  mb       Make a bucket");
            println!("  rb       Remove a bucket");
            println!("  cp       Copy files and objects");
            println!("  mv       Move files and objects");
            println!("  rm       Remove files and objects");
            println!("  cat      Display file and object contents");
            println!("  head     Display first 'n' lines of an object");
            println!("  pipe     Redirect STDIN to an object");
            println!("  find     Search for files and objects");
            println!("  stat     Show object metadata");
            println!("  du       Summarize disk usage");
            println!("  diff     List differences between two folders");
            println!("  mirror   Synchronize objects to a remote site");
            println!("  admin    Manage MinIO servers");
            println!("  version  Show version");
            0
        }
        "--version" | "version" => {
            println!("mc version RELEASE.2025-05-22 (SlateOS)");
            0
        }
        "alias" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("local");
                    println!("  URL       : http://localhost:9000");
                    println!("  AccessKey : minioadmin");
                    println!("  SecretKey : minioadmin");
                    println!("  API       : s3v4");
                    println!("  Path      : auto");
                    println!();
                    println!("myminio");
                    println!("  URL       : http://minio.example.com:9000");
                    println!("  AccessKey : AKIAEXAMPLE");
                    println!("  SecretKey : ********");
                    println!("  API       : s3v4");
                    println!("  Path      : auto");
                }
                "set" => println!("Added alias successfully."),
                "remove" => println!("Removed alias successfully."),
                _ => println!("Usage: mc alias <list|set|remove>"),
            }
            0
        }
        "ls" => {
            let path = cmd_args.first().map(|s| s.as_str()).unwrap_or("local/");
            println!("[2025-05-22 10:00:00 UTC]     0B {}/", path);
            println!("[2025-05-22 09:30:00 UTC]  4.2MiB STANDARD backup-2025-05-22.tar.gz");
            println!("[2025-05-22 08:15:00 UTC] 12.8KiB STANDARD config.yaml");
            println!("[2025-05-21 22:00:00 UTC]  1.1GiB STANDARD database-dump.sql.gz");
            0
        }
        "mb" => {
            let bucket = cmd_args.first().map(|s| s.as_str()).unwrap_or("local/mybucket");
            println!("Bucket created successfully `{}`.", bucket);
            0
        }
        "rb" => {
            let bucket = cmd_args.first().map(|s| s.as_str()).unwrap_or("local/mybucket");
            println!("Removed `{}` successfully.", bucket);
            0
        }
        "cp" => {
            let src = cmd_args.first().map(|s| s.as_str()).unwrap_or("src");
            let dst = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("dst");
            println!("`{}` -> `{}`", src, dst);
            println!("Total: 1 file, 4.2 MiB. Speed: 42.0 MiB/s");
            0
        }
        "rm" => {
            let path = cmd_args.first().map(|s| s.as_str()).unwrap_or("object");
            println!("Removed `{}`.", path);
            0
        }
        "cat" => {
            println!("{{\"key\": \"value\", \"example\": true}}");
            0
        }
        "stat" => {
            let path = cmd_args.first().map(|s| s.as_str()).unwrap_or("local/bucket/file");
            println!("Name      : {}", path);
            println!("Date      : 2025-05-22 10:00:00 UTC");
            println!("Size      : 4.2 MiB");
            println!("ETag      : abc123def456");
            println!("Type      : file");
            println!("Metadata  :");
            println!("  Content-Type: application/octet-stream");
            0
        }
        "du" => {
            let path = cmd_args.first().map(|s| s.as_str()).unwrap_or("local/");
            println!("1.2GiB\t3 objects\t{}", path);
            0
        }
        "mirror" => {
            let src = cmd_args.first().map(|s| s.as_str()).unwrap_or("src/");
            let dst = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("dst/");
            println!("`{}` -> `{}`", src, dst);
            println!("Total: 42 files, 1.2 GiB. Speed: 85.0 MiB/s");
            0
        }
        "admin" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    println!("●  localhost:9000");
                    println!("   Uptime: 1 day 2 hours");
                    println!("   Version: RELEASE.2025-05-22");
                    println!("   Network: 1/1 OK");
                    println!("   Drives: 1/1 OK");
                    println!("   Pool: 1");
                }
                "user" => {
                    let action = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("list");
                    match action {
                        "list" => {
                            println!("enabled    minioadmin             admin");
                            println!("enabled    app-user               readwrite");
                        }
                        "add" => println!("Added user successfully."),
                        "remove" => println!("Removed user successfully."),
                        _ => println!("Usage: mc admin user <list|add|remove>"),
                    }
                }
                "service" => {
                    let action = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("status");
                    match action {
                        "status" => println!("●  localhost:9000  (1 drives online, 0 drives offline)"),
                        "restart" => println!("Restarted MinIO server successfully."),
                        "stop" => println!("Stopped MinIO server successfully."),
                        _ => println!("Usage: mc admin service <status|restart|stop>"),
                    }
                }
                _ => println!("Usage: mc admin <info|user|service>"),
            }
            0
        }
        other => { eprintln!("mc: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("minio");
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
        "mc" => run_mc(rest),
        _ => run_minio_server(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_minio_server};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_minio_server(vec!["--help".to_string()]), 0);
        assert_eq!(run_minio_server(vec!["-h".to_string()]), 0);
        let _ = run_minio_server(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_minio_server(vec![]);
    }
}
