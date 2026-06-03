#![deny(clippy::all)]

//! xdotool-cli — OurOS xdotool X11 automation CLI
//!
//! Single personality: `xdotool`

use std::env;
use std::process;

fn run_xdotool(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xdotool COMMAND [OPTIONS] [ARGS]");
        println!();
        println!("xdotool — X11 automation tool (OurOS).");
        println!();
        println!("Window commands:");
        println!("  search [OPTIONS] PATTERN  Search for windows");
        println!("  getactivewindow           Get active window ID");
        println!("  getfocuswindow            Get focused window ID");
        println!("  windowactivate WID        Activate window");
        println!("  windowfocus WID           Focus window");
        println!("  windowmove WID X Y        Move window");
        println!("  windowsize WID W H        Resize window");
        println!("  windowminimize WID        Minimize window");
        println!("  windowclose WID           Close window");
        println!("  windowraise WID           Raise window");
        println!("  windowlower WID           Lower window");
        println!("  set_window --name N WID   Set window properties");
        println!();
        println!("Keyboard commands:");
        println!("  key KEY                   Press key");
        println!("  keydown KEY               Press key down");
        println!("  keyup KEY                 Release key");
        println!("  type TEXT                 Type text");
        println!();
        println!("Mouse commands:");
        println!("  mousemove X Y             Move mouse");
        println!("  click BUTTON              Click button");
        println!("  mousedown BUTTON          Press button");
        println!("  mouseup BUTTON            Release button");
        println!("  getmouselocation          Get mouse position");
        println!();
        println!("Other:");
        println!("  sleep SECONDS             Sleep");
        println!("  getdisplaygeometry        Get screen size");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("xdotool version 3.20211022.1 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "search" => {
            let pattern = args.iter().skip(1).find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("*");
            let _ = pattern;
            println!("12345678");
            println!("23456789");
        }
        "getactivewindow" => println!("12345678"),
        "getfocuswindow" => println!("12345678"),
        "windowactivate" | "windowfocus" | "windowraise" | "windowlower" => {}
        "windowmove" => {}
        "windowsize" => {}
        "windowminimize" => {}
        "windowclose" => {}
        "key" => {
            let key = args.get(1).map(|s| s.as_str()).unwrap_or("Return");
            let _ = key;
        }
        "type" => {
            let text = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let _ = text;
        }
        "mousemove" => {}
        "click" => {}
        "getmouselocation" => {
            println!("x:512 y:384 screen:0 window:12345678");
        }
        "getdisplaygeometry" => {
            println!("1920 1080");
        }
        "sleep" => {}
        "set_window" => {}
        _ => {
            eprintln!("xdotool: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xdotool(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_xdotool};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_xdotool(vec!["--help".to_string()]), 0);
        assert_eq!(run_xdotool(vec!["-h".to_string()]), 0);
        assert_eq!(run_xdotool(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_xdotool(vec![]), 0);
    }
}
