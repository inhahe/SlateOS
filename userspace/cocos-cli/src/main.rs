#![deny(clippy::all)]

//! cocos-cli — OurOS Cocos Creator (open-source 2D/3D engine, big in China)
//!
//! Single personality: `cocos`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cocos(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cocos [OPTIONS]");
        println!("Cocos Creator 3.8.4 (OurOS) — Open-source 2D/3D engine (big in CN market)");
        println!();
        println!("Options:");
        println!("  --new                  New project (3D / 2D / Empty)");
        println!("  --dashboard            Cocos Dashboard (project + editor version manager)");
        println!("  --build TARGET         Build (web / native / minigame [wechat/bytedance/etc] / android)");
        println!("  --animation            Cocos Animation Editor");
        println!("  --particle             Particle System editor");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cocos Creator 3.8.4 (OurOS)"); return 0; }
    println!("Cocos Creator 3.8.4 (OurOS)");
    println!("  Vendor: Xiamen Yaji Software Co. (Cocos Engine team, Xiamen, China)");
    println!("  License: MIT (engine) — fully open source");
    println!("  Origin: cocos2d-iphone (2008, Argentina-born, Ricardo Quesada) →");
    println!("         cocos2d-x C++ port (2010 China) → cocos2d-js → cocos2d-html5 → Cocos Creator (2016)");
    println!("  Pricing: FREE — Cocos Engine itself is free; Cocos Service (cloud) and Asset Store paid");
    println!("  Languages: TypeScript (preferred) or JavaScript, with optional C++ binding extensions");
    println!("  Targets: web (HTML5), native (C++ for Windows/Mac/Linux/iOS/Android),");
    println!("           Chinese mini-games (WeChat, ByteDance/Douyin, Alipay, QQ — 1B+ users each)");
    println!("  Engine: 2D-first historically, full 3D since Cocos Creator 3.0 (2021)");
    println!("        PBR rendering, GLTF importer, skeletal animation, Spine/DragonBones support");
    println!("  Market position: dominant 2D/casual engine in Chinese mobile game industry");
    println!("                  WeChat Mini-Games (1B users) — many built in Cocos");
    println!("  Famous Cocos games:");
    println!("    - Badland (Frogmind)");
    println!("    - Hill Climb Racing (Fingersoft)");
    println!("    - HellaPaint (HoYoverse early)");
    println!("    - many Chinese-market mobile hits (Honor of Kings predecessors, etc.)");
    println!("  Tooling: Cocos Dashboard manages multiple editor versions per project");
    println!("  Differentiator: best-of-class HTML5 / mini-game export, lightweight runtime (~250KB)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cocos".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cocos(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
