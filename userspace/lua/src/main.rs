#![deny(clippy::all)]

//! lua — Slate OS Lua scripting language
//!
//! Multi-personality: `lua`, `luac`, `luarocks`

use std::env;
use std::process;

fn run_lua(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lua [options] [script [args]]");
        println!();
        println!("Options:");
        println!("  -e stat   Execute string 'stat'");
        println!("  -i        Enter interactive mode after running script");
        println!("  -l name   Require library 'name'");
        println!("  -v        Show version");
        println!("  -E        Ignore environment variables");
        println!("  --        Stop handling options");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("Lua 5.4.7 (Slate OS)  Copyright (C) 1994-2025 Lua.org, PUC-Rio");
        return 0;
    }

    let exec_str = args.iter().position(|a| a == "-e")
        .and_then(|i| args.get(i + 1));
    if let Some(code) = exec_str {
        println!("(executing: {})", code);
        return 0;
    }

    let script = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(file) = script {
        println!("(running {})", file);
    } else {
        println!("Lua 5.4.7 (Slate OS)  Copyright (C) 1994-2025 Lua.org, PUC-Rio");
        println!("> (interactive mode — simulated)");
    }
    0
}

fn run_luac(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: luac [options] [filenames]");
        println!("  -l       List (use -l -l for full listing)");
        println!("  -o name  Output file (default: luac.out)");
        println!("  -p       Parse only");
        println!("  -s       Strip debug info");
        println!("  -v       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("Lua 5.4.7 (Slate OS)");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    for f in &files {
        println!("Compiling {}...", f);
    }
    println!("(compilation complete — simulated)");
    0
}

fn run_luarocks(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: luarocks <command> [args]");
        println!();
        println!("Commands:");
        println!("  install <rock>    Install a rock");
        println!("  remove <rock>     Remove a rock");
        println!("  list              List installed rocks");
        println!("  search <query>    Search rocks");
        println!("  show <rock>       Show rock info");
        println!("  make              Build rock from rockspec");
        println!("  path              Show LUA_PATH/LUA_CPATH");
        println!("  --version         Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "--version" => println!("luarocks 3.11.1 (Slate OS)"),
        "list" => {
            println!("Rocks installed for Lua 5.4");
            println!("---------------------------");
            println!("luasocket  3.1.0-1");
            println!("lpeg       1.1.0-1");
            println!("luafilesystem 1.8.0-1");
        }
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("");
            println!("Search results for '{}': (simulated)", query);
        }
        "path" => {
            println!("export LUA_PATH='./?.lua;./?/init.lua;/usr/local/share/lua/5.4/?.lua'");
            println!("export LUA_CPATH='./?.so;/usr/local/lib/lua/5.4/?.so'");
        }
        "install" | "remove" | "show" | "make" => println!("({} — simulated)", cmd),
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("lua");
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
        "luac" => run_luac(rest),
        "luarocks" => run_luarocks(rest),
        _ => run_lua(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_lua};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lua(vec!["--help".to_string()]), 0);
        assert_eq!(run_lua(vec!["-h".to_string()]), 0);
        let _ = run_lua(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lua(vec![]);
    }
}
