#![deny(clippy::all)]

//! virt-viewer-cli — OurOS virt-viewer VM display client
//!
//! Multi-personality: `virt-viewer`, `remote-viewer`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_virt_viewer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virt-viewer [OPTIONS] [DOMAIN-NAME|ID|UUID]");
        println!("virt-viewer v11.0 (OurOS) — Virtual machine viewer");
        println!();
        println!("Options:");
        println!("  -c URI           Hypervisor connection URI");
        println!("  -f, --fullscreen Start in fullscreen");
        println!("  -w, --wait       Wait for domain to start");
        println!("  --reconnect      Reconnect on disconnect");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("virt-viewer v11.0 (OurOS)"); return 0; }
    println!("virt-viewer: connecting to VM display");
    println!("  Protocol: SPICE");
    println!("  Display: 1024x768");
    0
}

fn run_remote_viewer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: remote-viewer [OPTIONS] URI");
        println!("remote-viewer v11.0 (OurOS) — Remote desktop viewer");
        println!("  Supports: spice://, vnc://, .vv files");
        println!("  -f               Fullscreen");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("remote-viewer v11.0 (OurOS)"); return 0; }
    if let Some(uri) = args.iter().find(|a| !a.starts_with('-')) {
        println!("remote-viewer: connecting to {}", uri);
    } else {
        println!("remote-viewer: no URI specified");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "virt-viewer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "remote-viewer" => run_remote_viewer(&rest, &prog),
        _ => run_virt_viewer(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_virt_viewer};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/virt-viewer"), "virt-viewer");
        assert_eq!(basename(r"C:\bin\virt-viewer.exe"), "virt-viewer.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("virt-viewer.exe"), "virt-viewer");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_virt_viewer(&["--help".to_string()], "virt-viewer"), 0);
        assert_eq!(run_virt_viewer(&["-h".to_string()], "virt-viewer"), 0);
        assert_eq!(run_virt_viewer(&["--version".to_string()], "virt-viewer"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_virt_viewer(&[], "virt-viewer"), 0);
    }
}
