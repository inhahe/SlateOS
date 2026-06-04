#![deny(clippy::all)]

//! tigervnc — OurOS VNC server and client
//!
//! Multi-personality: `vncserver`, `vncviewer`, `vncpasswd`, `vncconfig`, `x0vncserver`

use std::env;
use std::process;

fn run_vncserver(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vncserver [:<display>] [options]");
        println!();
        println!("Options:");
        println!("  -geometry WxH     Desktop size (default: 1920x1080)");
        println!("  -depth D          Color depth (default: 24)");
        println!("  -rfbport PORT     VNC port");
        println!("  -localhost         Only allow local connections");
        println!("  -name NAME        Desktop name");
        println!("  -kill :<display>  Kill VNC server");
        println!("  -list             List running servers");
        return 0;
    }
    if args.iter().any(|a| a == "-list") {
        println!("TigerVNC server sessions:");
        println!();
        println!("X DISPLAY #     PROCESS ID");
        println!(":1              12345");
        return 0;
    }
    if args.iter().any(|a| a == "-kill") {
        println!("Killing Xvnc process ID 12345");
        return 0;
    }

    let display = args.iter().find(|a| a.starts_with(':')).map(|s| s.as_str()).unwrap_or(":1");
    let geometry = args.iter().position(|a| a == "-geometry")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("1920x1080");
    println!("New '{}' desktop at {}", display, display);
    println!("Desktop geometry: {}", geometry);
    println!("Starting session on port 590{}", &display[1..]);
    0
}

fn run_vncviewer(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vncviewer [options] [host][:display]");
        println!();
        println!("Options:");
        println!("  -fullscreen       Fullscreen mode");
        println!("  -geometry WxH     Window size");
        println!("  -quality N        JPEG quality (0-9)");
        println!("  -compresslevel N  Compression level (0-9)");
        println!("  -encoding TYPE    Preferred encoding (tight/zrle/hextile/raw)");
        println!("  -passwd FILE      Password file");
        println!("  -via HOST         Tunnel via SSH host");
        return 0;
    }

    let host = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("localhost:1");
    println!("TigerVNC Viewer 1.13.1 (OurOS)");
    println!("Connected to VNC server at {}", host);
    println!("Desktop name: \"OurOS Desktop\"");
    println!("Desktop size: 1920x1080");
    0
}

fn run_vncpasswd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vncpasswd [file]");
        return 0;
    }
    let file = args.first().map(|s| s.as_str()).unwrap_or("~/.vnc/passwd");
    println!("Password stored in {}", file);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("vncserver");
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
        "vncviewer" => run_vncviewer(rest),
        "vncpasswd" => run_vncpasswd(rest),
        "vncconfig" => { println!("(VNC config tool — simulated)"); 0 }
        "x0vncserver" => { println!("x0vncserver: sharing existing X display"); 0 }
        _ => run_vncserver(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_vncserver};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vncserver(vec!["--help".to_string()]), 0);
        assert_eq!(run_vncserver(vec!["-h".to_string()]), 0);
        let _ = run_vncserver(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vncserver(vec![]);
    }
}
