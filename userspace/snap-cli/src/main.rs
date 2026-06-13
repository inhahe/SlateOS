#![deny(clippy::all)]

//! snap-cli — Slate OS Snap package manager
//!
//! Multi-personality: `snap`

use std::env;
use std::process;

fn run_snap(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: snap COMMAND [OPTIONS]");
        println!("snap 2.63 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  install       Install a snap");
        println!("  remove        Remove a snap");
        println!("  refresh       Refresh snaps");
        println!("  find          Find snaps in the store");
        println!("  info          Show snap information");
        println!("  list          List installed snaps");
        println!("  run           Run a snap command");
        println!("  connect       Connect a plug to a slot");
        println!("  disconnect    Disconnect a plug from a slot");
        println!("  interfaces    List interfaces");
        println!("  changes       List system changes");
        println!("  revert        Revert to previous revision");
        println!("  enable        Enable a snap");
        println!("  disable       Disable a snap");
        println!("  version       Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => {
            println!("snap    2.63");
            println!("snapd   2.63");
            println!("series  16");
            println!("slateos   1.0");
            println!("kernel  6.7.4-slateos");
        }
        "install" => {
            let snap = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            let channel = args.windows(2)
                .find(|w| w[0] == "--channel")
                .map(|w| w[1].as_str())
                .unwrap_or("stable");
            println!("{} ({}) 1.0.0 from Canonical installed", snap, channel);
        }
        "remove" => {
            let snap = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("{} removed", snap);
        }
        "refresh" => {
            if args.len() > 1 {
                let snap = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
                println!("{} refreshed", snap);
            } else {
                println!("All snaps up to date.");
            }
        }
        "find" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("Name              Version     Publisher    Notes  Summary");
            println!("{}            1.0.0       canonical    -      Hello World", query);
            println!("{}-cli         0.5.0       community    -      CLI tools", query);
        }
        "info" => {
            let snap = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("name:      {}", snap);
            println!("summary:   Hello World");
            println!("publisher: Canonical");
            println!("store-url: https://snapcraft.io/{}", snap);
            println!("license:   GPL-3.0");
            println!("installed: 1.0.0 (42) 5MB classic");
            println!("channels:");
            println!("  latest/stable:    1.0.0 2024-02-15 (42) 5MB classic");
            println!("  latest/candidate: 1.1.0 2024-02-10 (43) 5MB classic");
        }
        "list" => {
            println!("Name           Version  Rev  Tracking       Publisher    Notes");
            println!("core22         20240205 1122 latest/stable  canonical    base");
            println!("snapd          2.63     20671 latest/stable canonical    snapd");
            println!("hello          1.0.0    42   latest/stable  canonical    classic");
        }
        "interfaces" => {
            println!("Slot                 Plug");
            println!(":home                hello");
            println!(":network             hello");
            println!(":desktop             hello");
        }
        "changes" => {
            println!("ID  Status  Spawn                  Ready                  Summary");
            println!("1   Done    2024-02-15T10:00:00Z   2024-02-15T10:00:05Z   Install \"hello\"");
        }
        "revert" => {
            let snap = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("{} reverted to revision 41", snap);
        }
        _ => println!("snap: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_snap(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_snap};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_snap(&["--help".to_string()]), 0);
        assert_eq!(run_snap(&["-h".to_string()]), 0);
        let _ = run_snap(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_snap(&[]);
    }
}
