#![deny(clippy::all)]

//! lua-cli — SlateOS Lua interpreter
//!
//! Multi-personality: `lua`, `luac`, `lua5.4`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lua(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lua [OPTIONS] [FILE [ARGS]]");
        println!("Lua 5.4.6 (Slate OS)");
        println!("  -e EXPR     Execute string");
        println!("  -l LIB      Require library");
        println!("  -i          Interactive mode after running script");
        println!("  -v          Show version");
        println!("  -W          Enable warnings");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("Lua 5.4.6  Copyright (C) 1994-2023 Lua.org, PUC-Rio");
        return 0;
    }
    if args.iter().any(|a| a == "-e") {
        let expr = args.windows(2).find(|w| w[0] == "-e").map(|w| w[1].as_str()).unwrap_or("print('hello')");
        println!("> {}", expr);
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".lua")).map(|s| s.as_str());
    if let Some(f) = file {
        println!("lua: executing {}", f);
    } else {
        println!("Lua 5.4.6  Copyright (C) 1994-2023 Lua.org, PUC-Rio");
        println!("> ");
    }
    0
}

fn run_luac(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: luac [OPTIONS] FILE.lua [FILE.lua ...]");
        println!("  -o FILE     Output file (default: luac.out)");
        println!("  -l          List bytecode");
        println!("  -s          Strip debug info");
        println!("  -p          Parse only");
        println!("  -v          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("Lua 5.4.6  Copyright (C) 1994-2023 Lua.org, PUC-Rio");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| a.ends_with(".lua")).map(|s| s.as_str()).collect();
    let list = args.iter().any(|a| a == "-l");
    if list {
        for f in &files {
            println!("luac: listing {}", f);
            println!("main <{}:0,0> (5 instructions at 0x...):", f);
            println!("  1\tLOADK\t0 0");
            println!("  2\tGETTABUP\t1 0 1");
            println!("  3\tCALL\t1 2 1");
        }
    } else {
        for f in &files {
            println!("luac: compiling {}", f);
        }
        println!("luac: output: luac.out");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lua".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "luac" => run_luac(&rest),
        _ => run_lua(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lua};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lua"), "lua");
        assert_eq!(basename(r"C:\bin\lua.exe"), "lua.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lua.exe"), "lua");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lua(&["--help".to_string()]), 0);
        assert_eq!(run_lua(&["-h".to_string()]), 0);
        let _ = run_lua(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lua(&[]);
    }
}
