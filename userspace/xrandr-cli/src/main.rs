#![deny(clippy::all)]

//! xrandr-cli — OurOS xrandr display configuration CLI
//!
//! Single personality: `xrandr`

use std::env;
use std::process;

fn run_xrandr(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xrandr [OPTIONS]");
        println!();
        println!("xrandr — display configuration (OurOS).");
        println!();
        println!("Options:");
        println!("  --output NAME       Select output");
        println!("  --mode WxH          Set mode");
        println!("  --rate HZ           Set refresh rate");
        println!("  --pos XxY           Set position");
        println!("  --rotate DIR        Rotate (normal/left/right/inverted)");
        println!("  --reflect DIR       Reflect (normal/x/y/xy)");
        println!("  --auto              Auto-configure");
        println!("  --off               Turn off output");
        println!("  --primary            Set as primary");
        println!("  --same-as NAME      Clone output");
        println!("  --right-of NAME     Position right of");
        println!("  --left-of NAME      Position left of");
        println!("  --above NAME        Position above");
        println!("  --below NAME        Position below");
        println!("  --scale XxY         Scale output");
        println!("  --dpi DPI           Set DPI");
        println!("  --brightness N      Set brightness (0.0-1.0)");
        println!("  --gamma R:G:B       Set gamma");
        println!("  --query             Query current config");
        println!("  --listmonitors      List monitors");
        println!("  --listproviders     List providers");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("xrandr program version 1.5.2 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "--listmonitors") {
        println!("Monitors: 1");
        println!(" 0: +*eDP-1 1920/344x1080/194+0+0  eDP-1");
        return 0;
    }

    if args.iter().any(|a| a == "--listproviders") {
        println!("Providers: number : 1");
        println!("Provider 0: id: 0x45 cap: 0xb, Source Output, Sink Output, Sink Offload crtcs: 4 outputs: 3 associated providers: 0 name:modesetting");
        return 0;
    }

    // If --output is specified with settings, apply silently
    if args.iter().any(|a| a == "--output") {
        return 0;
    }

    // Default: query mode
    println!("Screen 0: minimum 320 x 200, current 1920 x 1080, maximum 16384 x 16384");
    println!("eDP-1 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 344mm x 194mm");
    println!("   1920x1080     60.00*+  48.00");
    println!("   1680x1050     60.00");
    println!("   1280x1024     60.02");
    println!("   1280x800      60.00");
    println!("   1024x768      60.00");
    println!("   800x600       60.32");
    println!("   640x480       59.94");
    println!("HDMI-1 disconnected (normal left inverted right x axis y axis)");
    println!("DP-1 disconnected (normal left inverted right x axis y axis)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xrandr(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_xrandr};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xrandr(vec!["--help".to_string()]), 0);
        assert_eq!(run_xrandr(vec!["-h".to_string()]), 0);
        let _ = run_xrandr(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xrandr(vec![]);
    }
}
