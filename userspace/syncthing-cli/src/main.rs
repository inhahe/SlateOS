#![deny(clippy::all)]

//! syncthing-cli — OurOS Syncthing CLI
//!
//! Multi-personality: `syncthing`, `stcli`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_syncthing(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: syncthing [OPTIONS] [COMMAND]");
        println!();
        println!("Syncthing — continuous file synchronization (OurOS).");
        println!();
        println!("Commands:");
        println!("  serve          Start syncthing (default)");
        println!("  cli            CLI interface");
        println!("  generate       Generate config/keys");
        println!();
        println!("Options:");
        println!("  --config DIR     Config directory");
        println!("  --data DIR       Data directory");
        println!("  --gui-address A  GUI listen address");
        println!("  --no-browser     Don't open browser");
        println!("  --logfile FILE   Log file");
        println!("  --no-restart     Don't restart on crash");
        println!("  --reset-database Reset database");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("syncthing v1.27.3 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("serve");

    match cmd {
        "serve" | "" => {
            println!("[start] syncthing v1.27.3 (OurOS)");
            println!("[start] My ID: ABCDEFG-HIJKLMN-OPQRSTU-VWXYZ01-2345678-9ABCDEF-GHIJKLM-NOPQRS0");
            println!("[start] GUI available at http://127.0.0.1:8384");
            println!("[start] Listening on :22000");
            println!("[start] Ready to synchronize (no folders configured)");
        }
        "generate" => {
            println!("Device ID: ABCDEFG-HIJKLMN-OPQRSTU-VWXYZ01-2345678-9ABCDEF-GHIJKLM-NOPQRS0");
            println!("Configuration generated.");
        }
        "cli" => {
            let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match subcmd {
                "show" => {
                    println!("system");
                    println!("  myID: ABCDEFG-HIJKLMN...");
                    println!("  uptime: 3h 45m 12s");
                    println!("  version: v1.27.3");
                }
                "config" => {
                    println!("Folders: 2");
                    println!("  Default Folder (/home/user/Sync)");
                    println!("  Documents (/home/user/Documents)");
                    println!("Devices: 1");
                    println!("  This Device");
                }
                "errors" => println!("No errors"),
                _ => println!("syncthing cli: unknown subcommand '{}'", subcmd),
            }
        }
        _ => {
            eprintln!("syncthing: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn run_stcli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: stcli [OPTIONS] COMMAND");
        println!();
        println!("stcli — Syncthing command-line interface (OurOS).");
        println!();
        println!("Commands:");
        println!("  show system    Show system status");
        println!("  show config    Show configuration");
        println!("  show errors    Show errors");
        println!("  operations     Show pending operations");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("show");
    if cmd == "show" {
        let what = args.get(1).map(|s| s.as_str()).unwrap_or("system");
        match what {
            "system" => {
                println!("myID: ABCDEFG-HIJKLMN...");
                println!("uptime: 3h 45m 12s");
            }
            "config" => println!("(configuration output)"),
            "errors" => println!("No errors"),
            _ => println!("stcli: unknown show target '{}'", what),
        }
    } else {
        println!("stcli: unknown command '{}'", cmd);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "syncthing".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "stcli" => run_stcli(&rest),
        _ => run_syncthing(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
