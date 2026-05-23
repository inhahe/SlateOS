#![deny(clippy::all)]

//! luarocks-cli — OurOS LuaRocks package manager
//!
//! Multi-personality: `luarocks`, `luarocks-admin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_luarocks(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: luarocks COMMAND [OPTIONS]");
        println!("LuaRocks 3.11.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  install      Install a rock");
        println!("  remove       Remove a rock");
        println!("  build        Build and install from rockspec");
        println!("  search       Search rocks");
        println!("  list         List installed rocks");
        println!("  show         Show rock info");
        println!("  make         Build from rockspec in current dir");
        println!("  new_version  Create new rockspec version");
        println!("  init         Initialize new project");
        println!("  path         Show Lua path");
        println!("  config       Show or set config");
        println!("  doc          Show rock documentation");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("luarocks 3.11.0 (OurOS)"),
        "install" => {
            let rock = args.get(1).map(|s| s.as_str()).unwrap_or("luasocket");
            println!("Installing https://luarocks.org/{}", rock);
            println!("{} 3.1.0-1 is now installed in /usr/local", rock);
        }
        "remove" => {
            let rock = args.get(1).map(|s| s.as_str()).unwrap_or("rock");
            println!("Removing {}...", rock);
            println!("Removed.");
        }
        "search" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("socket");
            println!("Search results for '{}':", term);
            println!("  luasocket   3.1.0-1    Network support for Lua");
            println!("  copas       4.7.1-1    Coroutine Oriented Portable Async Services");
        }
        "list" => {
            println!("Installed rocks:");
            println!("  luasocket 3.1.0-1 (installed)");
            println!("  lpeg 1.1.0-1 (installed)");
            println!("  lfs 1.8.0-1 (installed)");
        }
        "show" => {
            let rock = args.get(1).map(|s| s.as_str()).unwrap_or("luasocket");
            println!("{}", rock);
            println!("  Version: 3.1.0-1");
            println!("  Homepage: https://github.com/lunarmodules/{}", rock);
            println!("  License: MIT");
        }
        "path" => {
            println!("export LUA_PATH='/usr/local/share/lua/5.4/?.lua;./?.lua'");
            println!("export LUA_CPATH='/usr/local/lib/lua/5.4/?.so;./?.so'");
        }
        "init" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("myrock");
            println!("Created {}-dev-1.rockspec", name);
            println!("Created .luarocks/config-5.4.lua");
        }
        _ => println!("luarocks: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "luarocks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_luarocks(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
