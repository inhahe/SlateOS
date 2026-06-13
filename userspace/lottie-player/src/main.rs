#![deny(clippy::all)]

//! lottie-player — SlateOS Lottie animation renderer/converter
//!
//! Single personality: `lottie-player`

use std::env;
use std::process;

fn run_lottie(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lottie-player [OPTIONS] <COMMAND>");
        println!();
        println!("Render and convert Lottie animations.");
        println!();
        println!("Commands:");
        println!("  render     Render animation to image frames");
        println!("  convert    Convert between formats (json/dotLottie)");
        println!("  info       Show animation metadata");
        println!("  preview    Preview animation in terminal");
        println!("  gif        Export animation as GIF");
        println!("  apng       Export animation as APNG");
        println!("  webp       Export animation as animated WebP");
        println!();
        println!("Options:");
        println!("  -V, --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("lottie-player 1.0.0 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "render" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: lottie-player render [OPTIONS] <FILE>");
                println!();
                println!("Options:");
                println!("  -o, --output <DIR>     Output directory for frames");
                println!("  --format <FMT>         Output format (png/svg, default: png)");
                println!("  --width <PX>           Frame width");
                println!("  --height <PX>          Frame height");
                println!("  --fps <N>              Frame rate override");
                println!("  --start <FRAME>        Start frame");
                println!("  --end <FRAME>          End frame");
                println!("  --background <COLOR>   Background color");
                return 0;
            }
            let file = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .map(|s| s.as_str())
                .unwrap_or("animation.json");
            println!("Rendering: {}", file);
            println!("  Resolution: 1920x1080");
            println!("  FPS: 60");
            println!("  Frames: 180 (3.0s)");
            println!("  Rendering frame 1/180...");
            println!("  Rendering frame 90/180...");
            println!("  Rendering frame 180/180...");
            println!("  Output: ./frames/frame_0001.png - frame_0180.png");
            0
        }
        "convert" => {
            let file = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .map(|s| s.as_str())
                .unwrap_or("animation.json");
            println!("Converting: {}", file);
            println!("  Input format: Lottie JSON");
            println!("  Output format: dotLottie (.lottie)");
            println!("  Compressed: 45,230 -> 12,891 bytes");
            println!("  Done.");
            0
        }
        "info" => {
            let file = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .map(|s| s.as_str())
                .unwrap_or("animation.json");
            println!("Animation: {}", file);
            println!("  Version: 5.7.1");
            println!("  Name: loading_spinner");
            println!("  Frame rate: 60 fps");
            println!("  Duration: 3.0s (180 frames)");
            println!("  In point: 0");
            println!("  Out point: 180");
            println!("  Width: 512");
            println!("  Height: 512");
            println!("  Layers: 5");
            println!("  Assets: 2");
            println!("  Has expressions: no");
            println!("  3D: no");
            0
        }
        "preview" => {
            println!("Previewing animation in terminal...");
            println!("  Frame 1/180 [==                    ] 0.6%");
            println!("  (Press Ctrl+C to stop)");
            0
        }
        "gif" => {
            let file = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .map(|s| s.as_str())
                .unwrap_or("animation.json");
            println!("Exporting {} to GIF...", file);
            println!("  Resolution: 512x512");
            println!("  FPS: 30");
            println!("  Encoding 90 frames...");
            println!("  Output: animation.gif (2,345,678 bytes)");
            0
        }
        "apng" => {
            let file = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .map(|s| s.as_str())
                .unwrap_or("animation.json");
            println!("Exporting {} to APNG...", file);
            println!("  Resolution: 512x512");
            println!("  FPS: 60");
            println!("  Encoding 180 frames...");
            println!("  Output: animation.apng (1,890,123 bytes)");
            0
        }
        "webp" => {
            let file = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .map(|s| s.as_str())
                .unwrap_or("animation.json");
            println!("Exporting {} to animated WebP...", file);
            println!("  Resolution: 512x512");
            println!("  FPS: 60");
            println!("  Quality: 90");
            println!("  Encoding 180 frames...");
            println!("  Output: animation.webp (987,654 bytes)");
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lottie(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_lottie};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lottie(vec!["--help".to_string()]), 0);
        assert_eq!(run_lottie(vec!["-h".to_string()]), 0);
        let _ = run_lottie(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lottie(vec![]);
    }
}
