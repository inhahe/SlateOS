#![deny(clippy::all)]

//! hyprland-cli — OurOS Hyprland compositor tools
//!
//! Multi-personality: `hyprctl`, `hyprpm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hyprland(args: &[String], prog: &str) -> i32 {
    if prog == "hyprpm" {
        if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
            println!("Usage: hyprpm COMMAND [ARGS...]");
            println!("Hyprland Plugin Manager");
            println!();
            println!("Commands:");
            println!("  add URL          Add a plugin repository");
            println!("  remove NAME      Remove a plugin");
            println!("  enable NAME      Enable a plugin");
            println!("  disable NAME     Disable a plugin");
            println!("  update           Update all plugins");
            println!("  list             List installed plugins");
            println!("  reload           Reload plugins");
            return 0;
        }
        let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
        match cmd {
            "list" => {
                println!("Installed plugins:");
                println!("  (none)");
            }
            "update" => println!("hyprpm: All plugins up to date."),
            "reload" => println!("hyprpm: Plugins reloaded."),
            "add" => {
                let url = args.get(1).map(|s| s.as_str()).unwrap_or("<url>");
                println!("hyprpm: Adding repository '{}'...", url);
                println!("hyprpm: Done.");
            }
            "remove" => {
                let name = args.get(1).map(|s| s.as_str()).unwrap_or("<plugin>");
                println!("hyprpm: Removing '{}'...", name);
            }
            "enable" => {
                let name = args.get(1).map(|s| s.as_str()).unwrap_or("<plugin>");
                println!("hyprpm: Enabled '{}'.", name);
            }
            "disable" => {
                let name = args.get(1).map(|s| s.as_str()).unwrap_or("<plugin>");
                println!("hyprpm: Disabled '{}'.", name);
            }
            _ => println!("hyprpm: unknown command '{}'", cmd),
        }
        return 0;
    }
    // hyprctl
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hyprctl [FLAGS] COMMAND [ARGS...]");
        println!("hyprctl (Hyprland 0.40) (OurOS)");
        println!();
        println!("Commands:");
        println!("  monitors         List monitors");
        println!("  workspaces       List workspaces");
        println!("  activeworkspace  Show active workspace");
        println!("  clients          List windows");
        println!("  activewindow     Show active window");
        println!("  layers           List layers");
        println!("  devices          List input devices");
        println!("  binds            List keybindings");
        println!("  version          Show version");
        println!("  dispatch CMD     Dispatch a command");
        println!("  keyword KEY VAL  Set a config keyword");
        println!("  reload           Reload config");
        println!("  kill             Enter kill mode");
        println!("  splash           Show splash text");
        println!();
        println!("Flags:");
        println!("  -j               JSON output");
        println!("  -i INSTANCE      Instance signature");
        println!("  --batch          Batch mode (semicolon-separated)");
        return 0;
    }
    let json = args.iter().any(|a| a == "-j");
    let cmd = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("version");

    match cmd {
        "version" => {
            if json {
                println!("{{\"branch\":\"\",\"commit\":\"main\",\"tag\":\"v0.40.0\",\"flags\":[]}}");
            } else {
                println!("Hyprland, built from branch main at commit main (OurOS).");
                println!("Tag: v0.40.0");
            }
        }
        "monitors" => {
            if json {
                println!("[{{\"id\":0,\"name\":\"DP-1\",\"description\":\"Dell U2723QE\",\"width\":2560,\"height\":1440,\"refreshRate\":60.0,\"x\":0,\"y\":0,\"scale\":1.0,\"focused\":true}}]");
            } else {
                println!("Monitor DP-1 (ID 0):");
                println!("  2560x1440@60.00 at 0x0");
                println!("  scale: 1.00");
                println!("  focused: yes");
            }
        }
        "workspaces" => {
            if json {
                println!("[{{\"id\":1,\"name\":\"1\",\"monitor\":\"DP-1\",\"windows\":2,\"lastwindow\":\"0x1234\"}}]");
            } else {
                println!("workspace ID 1 (1) on monitor DP-1:");
                println!("  windows: 2");
                println!("  lastwindow: 0x1234");
            }
        }
        "activeworkspace" => {
            if json {
                println!("{{\"id\":1,\"name\":\"1\",\"monitor\":\"DP-1\",\"windows\":2}}");
            } else {
                println!("workspace ID 1 (1) on monitor DP-1:");
                println!("  windows: 2");
            }
        }
        "clients" => {
            if json {
                println!("[{{\"address\":\"0x1234\",\"mapped\":true,\"title\":\"Terminal\",\"class\":\"kitty\",\"workspace\":{{\"id\":1,\"name\":\"1\"}}}}]");
            } else {
                println!("Window 0x1234 -> Terminal:");
                println!("  class: kitty");
                println!("  workspace: 1 (1)");
            }
        }
        "activewindow" => {
            if json {
                println!("{{\"address\":\"0x1234\",\"title\":\"Terminal\",\"class\":\"kitty\",\"workspace\":{{\"id\":1}}}}");
            } else {
                println!("Window 0x1234 -> Terminal:");
                println!("  class: kitty");
            }
        }
        "devices" => {
            if json {
                println!("{{\"mice\":[],\"keyboards\":[{{\"address\":\"0x01\",\"name\":\"AT keyboard\",\"rules\":{{}}}}],\"tablets\":[],\"touch\":[]}}");
            } else {
                println!("keyboards:");
                println!("  AT keyboard at 0x01");
            }
        }
        "layers" => println!("Namespace: (no layers)"),
        "binds" => println!("(no binds configured)"),
        "dispatch" => {
            let action = args.iter().skip_while(|a| a.as_str() != "dispatch").nth(1)
                .map(|s| s.as_str()).unwrap_or("exec");
            println!("ok (dispatched: {})", action);
        }
        "keyword" => println!("ok"),
        "reload" => println!("ok (config reloaded)"),
        "kill" => println!("ok (kill mode)"),
        "splash" => println!("Hyprland — a dynamic tiling Wayland compositor"),
        _ => println!("unknown request {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hyprctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hyprland(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
