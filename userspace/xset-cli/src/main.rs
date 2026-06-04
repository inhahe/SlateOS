#![deny(clippy::all)]

//! xset-cli — OurOS xset/xsetroot/xrdb X11 settings CLI
//!
//! Multi-personality: `xset`, `xsetroot`, `xrdb`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_xset(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xset [OPTIONS]");
        println!();
        println!("xset — X11 user preference utility (OurOS).");
        println!();
        println!("Options:");
        println!("  q               Query current settings");
        println!("  r [on|off]      Keyboard auto-repeat");
        println!("  b [on|off|VOL]  Bell settings");
        println!("  s [N|on|off]    Screen saver timeout");
        println!("  dpms [N N N]    DPMS timeouts");
        println!("  +dpms / -dpms   Enable/disable DPMS");
        println!("  fp PATH         Font path");
        println!("  led [on|off]    Keyboard LED");
        println!("  m ACC THR       Mouse settings");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("q");

    if cmd == "q" {
        println!("Keyboard Control:");
        println!("  auto repeat:  on    key click percent:  0    LED mask:  00000000");
        println!("  XKB indicators:");
        println!("    00: Caps Lock:   off    01: Num Lock:    off    02: Scroll Lock: off");
        println!("  auto repeat delay:  660    repeat rate:  25");
        println!();
        println!("Pointer Control:");
        println!("  acceleration:  2/1    threshold:  4");
        println!();
        println!("Screen Saver:");
        println!("  prefer blanking:  yes    allow exposures:  yes");
        println!("  timeout:  600    cycle:  600");
        println!();
        println!("DPMS (Energy Star):");
        println!("  Standby: 600    Suspend: 600    Off: 600");
        println!("  DPMS is Enabled");
    }
    0
}

fn run_xsetroot(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xsetroot [OPTIONS]");
        println!();
        println!("xsetroot — X11 root window settings (OurOS).");
        println!();
        println!("Options:");
        println!("  -solid COLOR    Solid background color");
        println!("  -cursor_name N  Set root cursor");
        println!("  -name TEXT      Set root window name");
        println!("  -def            Reset to defaults");
        println!("  -bitmap FILE    Set bitmap background");
        println!("  -mod X Y        Set mod pattern");
        return 0;
    }
    // All commands are silent on success
    0
}

fn run_xrdb(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xrdb [OPTIONS] [FILE]");
        println!();
        println!("xrdb — X server resource database (OurOS).");
        println!();
        println!("Options:");
        println!("  -query         Show current resources");
        println!("  -load          Load resources (replace)");
        println!("  -merge         Merge resources");
        println!("  -remove        Remove resources");
        println!("  -symbols       Show preprocessor symbols");
        println!("  -cpp PROG      C preprocessor to use");
        return 0;
    }

    if args.iter().any(|a| a == "-query") {
        println!("*foreground: #ffffff");
        println!("*background: #1a1a2e");
        println!("*cursorColor: #e0e0e0");
        println!("*font: monospace:size=11");
        println!("Xft.dpi: 96");
        println!("Xft.antialias: true");
        println!("Xft.hinting: true");
        println!("Xft.hintstyle: hintslight");
    } else if args.iter().any(|a| a == "-symbols") {
        println!("SERVERHOST=localhost");
        println!("DISPLAY_NUM=0");
        println!("SCREEN_NUM=0");
        println!("BITS_PER_RGB=8");
        println!("WIDTH=1920");
        println!("HEIGHT=1080");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "xset".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "xsetroot" => run_xsetroot(&rest),
        "xrdb" => run_xrdb(&rest),
        _ => run_xset(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xset};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xset"), "xset");
        assert_eq!(basename(r"C:\bin\xset.exe"), "xset.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xset.exe"), "xset");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xset(&["--help".to_string()]), 0);
        assert_eq!(run_xset(&["-h".to_string()]), 0);
        let _ = run_xset(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xset(&[]);
    }
}
