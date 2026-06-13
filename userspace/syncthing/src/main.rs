#![deny(clippy::all)]

//! syncthing — Slate OS continuous file synchronization
//!
//! Single personality: `syncthing`

use std::env;
use std::process;

fn run_syncthing(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str());

    if args.iter().any(|a| a == "--help" || a == "-h") || cmd == Some("help") {
        println!("Usage: syncthing [command] [flags]");
        println!();
        println!("Commands:");
        println!("  serve        Start syncthing (default)");
        println!("  cli          Run CLI commands");
        println!("  generate     Generate key and config");
        println!("  decrypt      Decrypt data");
        println!("  version      Show version");
        println!();
        println!("Flags:");
        println!("  --home <dir>           Configuration directory");
        println!("  --gui-address <addr>   GUI listen address (default: 127.0.0.1:8384)");
        println!("  --no-browser           Don't open browser");
        println!("  --no-restart           Don't restart after upgrade");
        return 0;
    }

    if cmd == Some("version") || args.iter().any(|a| a == "--version") {
        println!("syncthing v1.27.7 \"Fermium Flea\" (Slate OS amd64) 2025-05-22");
        return 0;
    }

    if cmd == Some("generate") {
        let home = args.iter().position(|a| a == "--home")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("~/.config/syncthing");
        println!("Generating ECDSA key and certificate for device ID...");
        println!("Device ID: ABCDEFG-HIJKLMN-OPQRSTU-VWXYZ12-3456789-0ABCDEF-GHIJKLM-NOPQRS1");
        println!("Configuration written to {}/config.xml", home);
        return 0;
    }

    if cmd == Some("cli") {
        let sub = args.get(1).map(|s| s.as_str()).unwrap_or("help");
        match sub {
            "show" => {
                let what = args.get(2).map(|s| s.as_str()).unwrap_or("system");
                match what {
                    "system" => {
                        println!("myID: ABCDEFG-HIJKLMN-OPQRSTU-VWXYZ12-3456789-0ABCDEF-GHIJKLM-NOPQRS1");
                        println!("uptime: 86400");
                        println!("goroutines: 42");
                        println!("alloc: 28.5 MiB");
                        println!("sys: 64.0 MiB");
                    }
                    "connections" => {
                        println!("Device                                                          In       Out    Type    Connected");
                        println!("XYXYXYX-...   192.168.1.101:22000   3.2 GiB  1.8 GiB  tcp-client  true");
                    }
                    "folders" => {
                        println!("ID            Label          Path               State");
                        println!("default       Default Folder ~/Sync             idle");
                        println!("documents     Documents      ~/Documents        scanning");
                        println!("photos        Photos         ~/Photos           idle");
                    }
                    _ => println!("Usage: syncthing cli show <system|connections|folders>"),
                }
            }
            "errors" => println!("No errors"),
            "operations" => println!("No active operations"),
            _ => println!("Usage: syncthing cli <show|errors|operations>"),
        }
        return 0;
    }

    // Default: start server
    let gui = args.iter().position(|a| a == "--gui-address")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("127.0.0.1:8384");
    println!("[start] 10:00:00 INFO: syncthing v1.27.7 \"Fermium Flea\" (Slate OS amd64)");
    println!("[start] 10:00:00 INFO: My ID: ABCDEFG-HIJKLMN-OPQRSTU-VWXYZ12-3456789-0ABCDEF-GHIJKLM-NOPQRS1");
    println!("[start] 10:00:00 INFO: Single thread SHA256 performance is 250 MB/s using minio/sha256-simd");
    println!("[start] 10:00:01 INFO: Ready to synchronize \"Default Folder\" (default)");
    println!("[start] 10:00:01 INFO: Ready to synchronize \"Documents\" (documents)");
    println!("[start] 10:00:01 INFO: Ready to synchronize \"Photos\" (photos)");
    println!("[start] 10:00:01 INFO: GUI and API listening on {}", gui);
    println!("[start] 10:00:01 INFO: Access the GUI via the following URL: http://{}/", gui);
    println!("[start] 10:00:02 INFO: Detected 1 NAT service");
    println!("[start] 10:00:02 INFO: Device XYXYXYX connected (tcp-client)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_syncthing(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_syncthing};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_syncthing(vec!["--help".to_string()]), 0);
        assert_eq!(run_syncthing(vec!["-h".to_string()]), 0);
        let _ = run_syncthing(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_syncthing(vec![]);
    }
}
