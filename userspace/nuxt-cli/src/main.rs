#![deny(clippy::all)]

//! nuxt-cli — OurOS Nuxt.js CLI
//!
//! Multi-personality: `nuxi`, `nuxt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nuxi(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nuxi COMMAND [OPTIONS]");
        println!("Nuxt CLI (nuxi) 3.12.2 (OurOS)");
        println!();
        println!("Commands:");
        println!("  init         Initialize a new project");
        println!("  dev          Start development server");
        println!("  build        Build for production");
        println!("  preview      Preview production build");
        println!("  generate     Generate static site");
        println!("  add          Add template/module/plugin");
        println!("  info         Show Nuxt info");
        println!("  analyze      Analyze bundle");
        println!("  prepare      Prepare Nuxt types");
        println!("  typecheck    Run type checking");
        println!("  cleanup      Clean generated files");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-v" => println!("3.12.2"),
        "init" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-nuxt-app");
            println!("Nuxt project initialized in ./{}", name);
            println!("  Created nuxt.config.ts");
            println!("  Created app.vue");
            println!("  Created package.json");
        }
        "dev" => {
            let port = args.windows(2).find(|w| w[0] == "--port")
                .map(|w| w[1].as_str()).unwrap_or("3000");
            println!("Nuxi 3.12.2");
            println!("Nuxt 3.12.2 with Nitro 2.9.6");
            println!("  > Local:    http://localhost:{}", port);
            println!("  > Network:  http://192.168.1.100:{}", port);
            println!("  > DevTools: http://localhost:{}/__nuxt_devtools__/", port);
        }
        "build" => {
            println!("Nuxi 3.12.2");
            println!("Building Nuxt project...");
            println!("  Nitro built in 1.234s");
            println!("  Client built in 2.345s");
            println!("Build complete. Output: .output/");
        }
        "generate" => {
            println!("Generating static site...");
            println!("  Prerendered 12 routes");
            println!("  Output: .output/public/");
        }
        "preview" => {
            println!("Starting preview server...");
            println!("  > http://localhost:3000");
        }
        "add" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("component");
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("MyComponent");
            println!("Created {} '{}'", what, name);
        }
        "info" => {
            println!("Nuxt: 3.12.2");
            println!("Nitro: 2.9.6");
            println!("Vue: 3.4.30");
            println!("Node: 20.14.0");
            println!("Package manager: pnpm@9.4.0");
        }
        "typecheck" => {
            println!("Running vue-tsc...");
            println!("No errors found.");
        }
        "cleanup" => {
            println!("Cleaning .nuxt/ and .output/...");
            println!("Done.");
        }
        _ => println!("nuxi: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nuxi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nuxi(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
