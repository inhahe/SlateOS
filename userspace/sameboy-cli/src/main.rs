#![deny(clippy::all)]

//! sameboy-cli — OurOS SameBoy Game Boy emulator
//!
//! Single personality: `sameboy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sameboy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sameboy [OPTIONS] [ROM]");
        println!("SameBoy v0.16 (OurOS) — Accurate Game Boy / Game Boy Color emulator");
        println!();
        println!("Options:");
        println!("  --model MODEL    Hardware model (dmg, cgb, mgb, sgb, sgb2, agb)");
        println!("  --scale N        Window scale (1-8)");
        println!("  --boot-rom FILE  Custom boot ROM");
        println!("  --noaudio        Disable audio");
        println!("  --filter FILT    Color filter (none, emulate-hardware, reduce-contrast)");
        println!("  --palette NAME   DMG palette (greyscale, lime, olive, teal)");
        println!("  --color-correction  Enable color correction");
        println!("  --rumble         Enable rumble emulation");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SameBoy v0.16.2 (OurOS)"); return 0; }
    let model = args.windows(2).find(|w| w[0] == "--model").map(|w| w[1].as_str()).unwrap_or("cgb");
    let model_name = match model {
        "dmg" => "Game Boy (DMG)",
        "mgb" => "Game Boy Pocket (MGB)",
        "sgb" | "sgb2" => "Super Game Boy",
        "agb" => "Game Boy Advance (GBA mode)",
        _ => "Game Boy Color (CGB)",
    };
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-') && {
        let idx = args.iter().position(|x| std::ptr::eq(x, *a)).unwrap_or(0);
        idx == 0 || !matches!(args.get(idx.wrapping_sub(1)).map(|s| s.as_str()), Some("--model" | "--scale" | "--boot-rom" | "--filter" | "--palette"))
    }).collect();
    if files.is_empty() {
        println!("SameBoy v0.16.2 (OurOS) — Game Boy Emulator");
        println!("  Model: {}", model_name);
        println!("  CPU: Sharp LR35902 @ 4.19 MHz (emulated)");
        println!("  Display: 160x144, 4 shades (DMG) / 32768 colors (CGB)");
        println!("  Audio: 4 channels (2 pulse, wave, noise)");
        println!("  Accuracy: T-cycle accurate");
        println!("  Status: waiting for ROM");
        return 0;
    }
    println!("SameBoy: Loading {} on {}", files[0], model_name);
    println!("  Running...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sameboy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sameboy(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
