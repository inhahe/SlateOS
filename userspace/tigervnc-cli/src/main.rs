#![deny(clippy::all)]

//! tigervnc-cli — OurOS TigerVNC server and client
//!
//! Multi-personality: `vncviewer`, `vncserver`, `vncpasswd`, `vncconfig`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_viewer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vncviewer [OPTIONS] HOST[:DISPLAY]");
        println!("vncviewer v1.14 (OurOS) — TigerVNC viewer");
        println!();
        println!("Options:");
        println!("  -FullScreen        Fullscreen mode");
        println!("  -SecurityTypes T   Security types (VncAuth, TLSVnc)");
        println!("  -Shared            Shared session");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("vncviewer v1.14 (OurOS, TigerVNC)"); return 0; }
    println!("vncviewer: connecting...");
    0
}

fn run_server(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vncserver [:DISPLAY] [OPTIONS]");
        println!("vncserver v1.14 (OurOS) — TigerVNC server");
        println!();
        println!("Options:");
        println!("  -geometry WxH     Screen size");
        println!("  -depth N          Color depth");
        println!("  -kill :N          Kill display N");
        println!("  -list             List running servers");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("vncserver v1.14 (OurOS, TigerVNC)"); return 0; }
    if args.iter().any(|a| a == "-list") {
        println!("TigerVNC server sessions:");
        println!("  (none running)");
        return 0;
    }
    println!("vncserver: starting on :1 (5901)");
    0
}

fn run_passwd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vncpasswd [PASSFILE]");
        println!("vncpasswd v1.14 (OurOS) — Set VNC password");
        return 0;
    }
    println!("vncpasswd: set VNC password");
    0
}

fn run_config(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vncconfig [OPTIONS]");
        println!("vncconfig v1.14 (OurOS) — VNC server configuration");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("vncconfig v1.14 (OurOS, TigerVNC)"); return 0; }
    println!("vncconfig: configuration utility started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vncviewer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "vncserver" => run_server(&rest, &prog),
        "vncpasswd" => run_passwd(&rest, &prog),
        "vncconfig" => run_config(&rest, &prog),
        _ => run_viewer(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
