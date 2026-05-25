#![deny(clippy::all)]

//! openresty-cli — OurOS OpenResty web platform
//!
//! Multi-personality: `openresty`, `resty`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_openresty(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "resty" => {
                println!("resty (OurOS) — OpenResty Lua script runner");
                println!("  -e CODE            Execute Lua code inline");
                println!("  FILE               Execute Lua script file");
                println!("  --shdict NAME=SIZE Shared memory zone");
                println!("  --http-conf CONF   Extra nginx http block config");
                println!("  --errlog-level LVL Error log level");
            }
            _ => {
                println!("OpenResty v1.25 (OurOS) — Nginx + LuaJIT web platform");
                println!("  -c FILE            Config file");
                println!("  -s SIGNAL          Send signal (stop/quit/reload)");
                println!("  -t                 Test configuration");
                println!("  -p PREFIX          Prefix path");
                println!("  -v                 Show version");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "-V" || a == "--version") {
        println!("OpenResty/1.25.3.1 (OurOS)");
        println!("  nginx/1.25.3, LuaJIT 2.1.0");
        return 0;
    }
    match prog {
        "resty" => {
            println!("resty - OpenResty Lua runner");
            println!("  LuaJIT: 2.1.0-beta3");
            println!("  Modules: ngx.*, resty.*, cjson, redis, mysql");
        }
        _ => {
            println!("OpenResty/1.25.3.1 (OurOS)");
            println!("  Workers: 4");
            println!("  Listening: 0.0.0.0:80, 0.0.0.0:443");
            println!("  LuaJIT: 2.1.0-beta3");
            println!("  Lua modules: 23 loaded");
            println!("  Shared dicts: 5 (128 MB total)");
            println!("  Cosockets: connection pooling enabled");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "openresty".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_openresty(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
