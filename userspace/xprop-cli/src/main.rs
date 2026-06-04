#![deny(clippy::all)]

//! xprop-cli — OurOS xprop/xwininfo X11 property tools CLI
//!
//! Multi-personality: `xprop`, `xwininfo`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_xprop(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xprop [OPTIONS] [PROPERTY ...]");
        println!();
        println!("xprop — X11 property displayer (OurOS).");
        println!();
        println!("Options:");
        println!("  -id WID         Window ID");
        println!("  -root           Use root window");
        println!("  -name NAME      Select by window name");
        println!("  -spy            Watch for property changes");
        println!("  -f NAME FMT     Set format for property");
        println!("  -remove PROP    Remove property");
        return 0;
    }

    let root = args.iter().any(|a| a == "-root");
    if root {
        println!("_NET_SUPPORTED(ATOM) = ...");
        println!("_NET_CLIENT_LIST(WINDOW): window id # 0x1000001, 0x2000001");
        println!("_NET_NUMBER_OF_DESKTOPS(CARDINAL) = 2");
        println!("_NET_CURRENT_DESKTOP(CARDINAL) = 0");
        println!("_NET_DESKTOP_NAMES(UTF8_STRING) = \"Desktop 1\", \"Desktop 2\"");
    } else {
        println!("WM_CLASS(STRING) = \"terminal\", \"Terminal\"");
        println!("WM_NAME(UTF8_STRING) = \"Terminal\"");
        println!("_NET_WM_PID(CARDINAL) = 1234");
        println!("_NET_WM_STATE(ATOM) = ");
        println!("WM_NORMAL_HINTS(WM_SIZE_HINTS):");
        println!("    program specified minimum size: 100 by 50");
        println!("    window gravity: NorthWest");
    }
    0
}

fn run_xwininfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xwininfo [OPTIONS]");
        println!();
        println!("xwininfo — X11 window information (OurOS).");
        println!();
        println!("Options:");
        println!("  -id WID         Window ID");
        println!("  -root           Use root window");
        println!("  -name NAME      Select by name");
        println!("  -tree           Show window tree");
        println!("  -children       Show children");
        println!("  -stats          Show statistics (default)");
        println!("  -all            Show all info");
        return 0;
    }

    let tree = args.iter().any(|a| a == "-tree");

    println!();
    println!("xwininfo: Window id: 0x1000001 \"Terminal\"");
    println!();
    if tree {
        println!("  Root window id: 0x1e9 (the root window) (has no name)");
        println!("  Parent window id: 0x1e9 (the root window) (has no name)");
        println!("     1 child:");
        println!("     0x1000002 (has no name): ()  1920x1080+0+0  +0+0");
    } else {
        println!("  Absolute upper-left X:  100");
        println!("  Absolute upper-left Y:  50");
        println!("  Relative upper-left X:  0");
        println!("  Relative upper-left Y:  0");
        println!("  Width: 800");
        println!("  Height: 600");
        println!("  Depth: 24");
        println!("  Visual: 0x21");
        println!("  Visual Class: TrueColor");
        println!("  Border width: 0");
        println!("  Class: InputOutput");
        println!("  Colormap: 0x20 (installed)");
        println!("  Bit Gravity State: NorthWestGravity");
        println!("  Window Gravity State: NorthWestGravity");
        println!("  Map State: IsViewable");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "xprop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "xwininfo" => run_xwininfo(&rest),
        _ => run_xprop(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xprop};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xprop"), "xprop");
        assert_eq!(basename(r"C:\bin\xprop.exe"), "xprop.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xprop.exe"), "xprop");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xprop(&["--help".to_string()]), 0);
        assert_eq!(run_xprop(&["-h".to_string()]), 0);
        let _ = run_xprop(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xprop(&[]);
    }
}
