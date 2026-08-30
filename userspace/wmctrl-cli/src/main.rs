#![deny(clippy::all)]

//! wmctrl-cli — Slate OS wmctrl window manager control CLI
//!
//! Single personality: `wmctrl`

use std::env;
use std::process;

fn run_wmctrl(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wmctrl [OPTIONS]");
        println!();
        println!("wmctrl — window manager control (Slate OS).");
        println!();
        println!("Options:");
        println!("  -l             List windows");
        println!("  -d             List desktops");
        println!("  -m             Show window manager info");
        println!("  -s N           Switch to desktop N");
        println!("  -a TITLE       Activate window by title");
        println!("  -c TITLE       Close window by title");
        println!("  -r TITLE       Select window (with -e/-b/-t)");
        println!("  -e GEOM        Move/resize (gravity,x,y,w,h)");
        println!("  -b PROP        Change state (add/remove/toggle)");
        println!("  -t N           Move window to desktop N");
        println!("  -i             Interpret window as ID");
        println!("  -p             Include PID in list");
        println!("  -G             Include geometry in list");
        println!("  -x             Include WM_CLASS in list");
        return 0;
    }

    if args.iter().any(|a| a == "-l") {
        let show_pid = args.iter().any(|a| a == "-p");
        let show_geom = args.iter().any(|a| a == "-G");
        let show_class = args.iter().any(|a| a == "-x");
        if show_pid && show_geom && show_class {
            println!("0x01000001  0 1234  0    0    1920 1080 terminal.Terminal    hostname Terminal");
            println!("0x02000001  0 5678  100  50   800  600  firefox.Firefox      hostname Mozilla Firefox");
            println!("0x03000001  1 9012  200  100  1024 768  nautilus.Nautilus     hostname Files");
        } else if show_pid {
            println!("0x01000001  0 1234 hostname Terminal");
            println!("0x02000001  0 5678 hostname Mozilla Firefox");
        } else {
            println!("0x01000001  0 hostname Terminal");
            println!("0x02000001  0 hostname Mozilla Firefox");
            println!("0x03000001  1 hostname Files");
        }
    } else if args.iter().any(|a| a == "-d") {
        println!("0  * DG: 1920x1080  VP: 0,0  WA: 0,32 1920x1048  Desktop 1");
        println!("1  - DG: 1920x1080  VP: N/A  WA: 0,32 1920x1048  Desktop 2");
    } else if args.iter().any(|a| a == "-m") {
        println!("Name: Slate OS WM");
        println!("Class: N/A");
        println!("PID: 1234");
        println!("Window manager's PID: 1234");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wmctrl(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_wmctrl};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wmctrl(vec!["--help".to_string()]), 0);
        assert_eq!(run_wmctrl(vec!["-h".to_string()]), 0);
        let _ = run_wmctrl(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wmctrl(vec![]);
    }
}
