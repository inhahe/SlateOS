#![deny(clippy::all)]

//! blender-cli — OurOS Blender 3D CLI
//!
//! Single personality: `blender`

use std::env;
use std::process;

fn run_blender(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: blender [OPTIONS] [FILE]");
        println!();
        println!("Blender — 3D creation suite (OurOS).");
        println!();
        println!("Render options:");
        println!("  -b, --background     Background mode (no GUI)");
        println!("  -a, --render-anim    Render animation");
        println!("  -f, --render-frame N Render frame N");
        println!("  -s, --frame-start N  Start frame");
        println!("  -e, --frame-end N    End frame");
        println!("  -o, --render-output P Output path");
        println!("  -F, --render-format F Format (PNG/JPEG/EXR/FFMPEG)");
        println!("  -E, --engine NAME    Render engine (CYCLES/BLENDER_EEVEE)");
        println!("  -t, --threads N      Number of threads");
        println!();
        println!("General options:");
        println!("  -P, --python FILE    Run Python script");
        println!("  --python-expr EXPR   Evaluate Python expression");
        println!("  --addons LIST        Enable add-ons");
        println!("  --factory-startup    Use factory settings");
        println!("  -noaudio             Disable audio");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Blender 4.0.2 (OurOS)");
        println!("  build date: 2024-01-15");
        println!("  build hash: abc123def");
        return 0;
    }

    let background = args.iter().any(|a| a == "-b" || a == "--background");
    let render_anim = args.iter().any(|a| a == "-a" || a == "--render-anim");
    let render_frame = args.windows(2).find(|w| w[0] == "-f" || w[0] == "--render-frame").map(|w| w[1].as_str());
    let engine = args.windows(2).find(|w| w[0] == "-E" || w[0] == "--engine").map(|w| w[1].as_str()).unwrap_or("CYCLES");
    let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--render-output").map(|w| w[1].as_str());
    let python = args.windows(2).find(|w| w[0] == "-P" || w[0] == "--python").map(|w| w[1].as_str());

    let file = args.iter().rfind(|a| !a.starts_with('-') && a.ends_with(".blend")).map(|s| s.as_str());

    if background {
        println!("Blender 4.0.2 (sub 0)");
        if let Some(f) = file {
            println!("Read blend: {}", f);
        }
        if let Some(py) = python {
            println!("Running Python script: {}", py);
        }
        if render_anim {
            println!("Render engine: {}", engine);
            if let Some(out) = output {
                println!("Output: {}", out);
            }
            println!("Rendering frame 1...");
            println!("Rendering frame 2...");
            println!("Rendering done.");
        } else if let Some(frame) = render_frame {
            println!("Render engine: {}", engine);
            println!("Rendering frame {}...", frame);
            println!("Saved: /tmp/frame{}.png", frame);
        }
    } else if let Some(f) = file {
        println!("Blender 4.0.2 — opening '{}'", f);
    } else {
        println!("Blender 4.0.2 (OurOS)");
        println!("Starting Blender GUI...");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_blender(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_blender};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_blender(vec!["--help".to_string()]), 0);
        assert_eq!(run_blender(vec!["-h".to_string()]), 0);
        assert_eq!(run_blender(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_blender(vec![]), 0);
    }
}
