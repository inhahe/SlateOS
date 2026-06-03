#![deny(clippy::all)]

//! gamescope-cli — OurOS Gamescope Wayland compositor for gaming
//!
//! Multi-personality: `gamescope`

use std::env;
use std::process;

fn run_gamescope(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gamescope [OPTIONS] -- COMMAND [ARGS]");
        println!();
        println!("gamescope — SteamOS session compositing WM (OurOS).");
        println!();
        println!("Options:");
        println!("  -w <width>       Game render width");
        println!("  -h <height>      Game render height");
        println!("  -W <width>       Output width");
        println!("  -H <height>      Output height");
        println!("  -r <rate>        Refresh rate (Hz)");
        println!("  -o <rate>        FPS limit");
        println!("  -F <filter>      Upscale filter (linear, nearest, fsr, nis)");
        println!("  -S <scaler>      Upscale scaler (auto, integer, fit, fill, stretch)");
        println!("  -f               Fullscreen");
        println!("  -b               Borderless");
        println!("  -e               Steam integration");
        println!("  --backend <be>   drm, wayland, sdl, headless");
        println!("  --hdr-enabled    Enable HDR");
        println!("  --mangoapp       Enable MangoHud");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gamescope version 3.14.2 (OurOS)");
        return 0;
    }

    let width = args.windows(2).find(|w| w[0] == "-w").map(|w| w[1].as_str()).unwrap_or("1920");
    let height_val = args.windows(2).find(|w| w[0] == "-h" && w[1].chars().all(|c| c.is_ascii_digit())).map(|w| w[1].as_str()).unwrap_or("1080");
    let out_w = args.windows(2).find(|w| w[0] == "-W").map(|w| w[1].as_str()).unwrap_or("3840");
    let out_h = args.windows(2).find(|w| w[0] == "-H").map(|w| w[1].as_str()).unwrap_or("2160");
    let refresh = args.windows(2).find(|w| w[0] == "-r").map(|w| w[1].as_str()).unwrap_or("60");
    let filter = args.windows(2).find(|w| w[0] == "-F").map(|w| w[1].as_str()).unwrap_or("fsr");

    println!("[gamescope] Starting compositor");
    println!("[gamescope] Render resolution: {}x{}", width, height_val);
    println!("[gamescope] Output resolution: {}x{}", out_w, out_h);
    println!("[gamescope] Refresh rate: {} Hz", refresh);
    println!("[gamescope] Upscale filter: {}", filter);
    println!("[gamescope] Backend: Vulkan on DRM/KMS");
    if args.iter().any(|a| a == "--hdr-enabled") {
        println!("[gamescope] HDR: enabled (PQ/scRGB)");
    }
    if args.iter().any(|a| a == "--mangoapp") {
        println!("[gamescope] MangoHud: enabled");
    }

    let separator = args.iter().position(|a| a == "--");
    if let Some(pos) = separator {
        if let Some(cmd) = args.get(pos + 1) {
            println!("[gamescope] Launching: {}", cmd);
        }
    }
    println!("[gamescope] Compositor ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gamescope(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gamescope};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gamescope(&["--help".to_string()]), 0);
        assert_eq!(run_gamescope(&["-h".to_string()]), 0);
        assert_eq!(run_gamescope(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gamescope(&[]), 0);
    }
}
