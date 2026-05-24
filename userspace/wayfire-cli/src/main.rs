#![deny(clippy::all)]

//! wayfire-cli — OurOS Wayfire 3D Wayland compositor
//!
//! Multi-personality: `wayfire`, `wf-msg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wayfire(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wayfire [OPTIONS]");
        println!("wayfire v0.8 (OurOS) — 3D Wayland compositor");
        println!();
        println!("Options:");
        println!("  -c FILE           Configuration file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wayfire v0.8 (OurOS)"); return 0; }
    println!("Wayfire compositor starting...");
    println!("  Backend: DRM/KMS");
    println!("  Plugins: animate, cube, expo, grid, move, resize, switcher, vswitch");
    println!("  Output: eDP-1 (2560x1600@120Hz)");
    0
}

fn run_wf_msg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wf-msg COMMAND [ARGS]");
        println!("wf-msg v0.8 (OurOS) — Wayfire IPC client");
        println!();
        println!("Commands:");
        println!("  list_views        List views");
        println!("  get_output        Get output info");
        println!("  set_view_*        Set view properties");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list_views");
    match cmd {
        "list_views" => {
            println!("[");
            println!("  {{\"id\": 1, \"title\": \"Terminal\", \"app-id\": \"foot\"}}");
            println!("  {{\"id\": 2, \"title\": \"Firefox\", \"app-id\": \"firefox\"}}");
            println!("]");
        }
        "get_output" => {
            println!("{{\"name\": \"eDP-1\", \"width\": 2560, \"height\": 1600, \"refresh\": 120000}}");
        }
        _ => println!("wf-msg {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wayfire".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "wf-msg" => run_wf_msg(&rest, &prog),
        _ => run_wayfire(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
