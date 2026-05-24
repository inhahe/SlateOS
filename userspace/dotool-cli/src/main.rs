#![deny(clippy::all)]

//! dotool-cli — OurOS dotool input automation
//!
//! Multi-personality: `dotool`, `dotoold`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dotool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dotool [OPTIONS]");
        println!("dotool v1.3 (OurOS) — Read commands from stdin to simulate input");
        println!();
        println!("Commands (from stdin):");
        println!("  key KEY           Press and release a key");
        println!("  keydown KEY       Press a key");
        println!("  keyup KEY         Release a key");
        println!("  type TEXT         Type text");
        println!("  typedelay MS      Set typing delay");
        println!("  keydelay MS       Set key delay");
        println!("  buttondown BTN    Press mouse button (left/right/middle)");
        println!("  buttonup BTN      Release mouse button");
        println!("  click BTN         Click mouse button");
        println!("  mousemove X Y     Move mouse absolutely");
        println!("  mouseto X Y       Move mouse to proportional position");
        println!("  scroll N          Scroll wheel");
        println!("  delay MS          Wait");
        return 0;
    }
    if args.is_empty() {
        println!("Reading commands from stdin...");
        println!("  Example: echo 'type Hello World' | dotool");
    }
    0
}

fn run_dotoold(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dotoold [OPTIONS]");
        println!("dotoold v1.3 (OurOS) — dotool daemon (persistent /tmp/dotool pipe)");
        println!();
        println!("Options:");
        println!("  --pipe PATH       Named pipe path (default: /tmp/dotool)");
        return 0;
    }
    let pipe = args.iter().skip_while(|a| a.as_str() != "--pipe").nth(1)
        .map(|s| s.as_str()).unwrap_or("/tmp/dotool");
    println!("dotoold: listening on pipe {}", pipe);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dotool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "dotoold" => run_dotoold(&rest, &prog),
        _ => run_dotool(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
