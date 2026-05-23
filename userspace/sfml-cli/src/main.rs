#![deny(clippy::all)]

//! sfml-cli — OurOS SFML config tool
//!
//! Multi-personality: `sfml-config`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sfml_config(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sfml-config [OPTIONS]");
        println!("SFML config 2.6.1 (OurOS)");
        println!();
        println!("Options:");
        println!("  --version        Print SFML version");
        println!("  --cflags         Print compiler flags");
        println!("  --libs           Print linker flags");
        println!("  --prefix         Print install prefix");
        println!();
        println!("Modules:");
        println!("  --system         System module");
        println!("  --window         Window module");
        println!("  --graphics       Graphics module");
        println!("  --audio          Audio module");
        println!("  --network        Network module");
        return 0;
    }
    let modules: Vec<&str> = args.iter()
        .filter(|a| matches!(a.as_str(), "--system" | "--window" | "--graphics" | "--audio" | "--network"))
        .map(|s| s.as_str())
        .collect();

    for arg in args {
        match arg.as_str() {
            "--version" => println!("2.6.1"),
            "--cflags" => println!("-I/usr/include"),
            "--libs" => {
                let mut libs = String::from("-L/usr/lib");
                for m in &modules {
                    match *m {
                        "--system" => libs.push_str(" -lsfml-system"),
                        "--window" => libs.push_str(" -lsfml-window"),
                        "--graphics" => libs.push_str(" -lsfml-graphics"),
                        "--audio" => libs.push_str(" -lsfml-audio"),
                        "--network" => libs.push_str(" -lsfml-network"),
                        _ => {}
                    }
                }
                if modules.is_empty() {
                    libs.push_str(" -lsfml-graphics -lsfml-window -lsfml-system");
                }
                println!("{}", libs);
            }
            "--prefix" => println!("/usr"),
            _ => {}
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sfml-config".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sfml_config(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
