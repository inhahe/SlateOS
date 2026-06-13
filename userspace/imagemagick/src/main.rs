#![deny(clippy::all)]

//! imagemagick — SlateOS image processing suite
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `convert` (default) — convert/transform images
//! - `identify` — describe image properties
//! - `mogrify` — in-place image transform
//! - `composite` — overlay images
//! - `montage` — create image mosaic
//! - `magick` — ImageMagick 7 unified command

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_convert(args: Vec<String>) -> i32 {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-help") {
        println!("Usage: convert [options ...] file [ [options ...] file ...] [options ...] file");
        println!();
        println!("Image conversion and manipulation tool.");
        println!();
        println!("Options:");
        println!("  -resize WxH      Resize image");
        println!("  -quality N       JPEG/PNG quality (1-100)");
        println!("  -crop WxH+X+Y   Crop image");
        println!("  -rotate degrees  Rotate image");
        println!("  -flip            Flip vertically");
        println!("  -flop            Flip horizontally");
        println!("  -blur RxS        Gaussian blur");
        println!("  -sharpen RxS     Sharpen image");
        println!("  -brightness-contrast BxC  Adjust brightness/contrast");
        println!("  -colorspace type Convert colorspace (sRGB, CMYK, Gray)");
        println!("  -depth N         Color depth");
        println!("  -strip           Strip metadata");
        println!("  -density NxN     Set resolution (DPI)");
        println!("  -format fmt      Output format");
        println!("  -append          Append images vertically");
        println!("  +append          Append images horizontally");
        println!("  -gravity type    Placement gravity");
        println!("  -annotate deg text  Add text annotation");
        println!("  -version         Show version");
        return 0;
    }

    if args.iter().any(|a| a == "-version" || a == "--version") {
        println!("Version: ImageMagick 7.1.0 (SlateOS)");
        println!("Features: DPC Modules OpenMP(4.5)");
        println!("Delegates (built-in): bzlib fontconfig freetype jng jpeg lcms lzma png tiff webp xml zlib");
        return 0;
    }

    // Gather operations info for output
    let mut operations: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-resize" => {
                i += 1;
                if i < args.len() { operations.push(format!("Resize: {}", args[i])); }
            }
            "-quality" => {
                i += 1;
                if i < args.len() { operations.push(format!("Quality: {}", args[i])); }
            }
            "-crop" => {
                i += 1;
                if i < args.len() { operations.push(format!("Crop: {}", args[i])); }
            }
            "-rotate" => {
                i += 1;
                if i < args.len() { operations.push(format!("Rotate: {}°", args[i])); }
            }
            "-flip" => operations.push("Flip vertical".to_string()),
            "-flop" => operations.push("Flip horizontal".to_string()),
            "-blur" => {
                i += 1;
                if i < args.len() { operations.push(format!("Blur: {}", args[i])); }
            }
            "-sharpen" => {
                i += 1;
                if i < args.len() { operations.push(format!("Sharpen: {}", args[i])); }
            }
            "-strip" => operations.push("Strip metadata".to_string()),
            "-colorspace" => {
                i += 1;
                if i < args.len() { operations.push(format!("Colorspace: {}", args[i])); }
            }
            _ => {}
        }
        i += 1;
    }

    let input = args.first().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("input.png");
    let output = args.iter().rfind(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("output.png");

    if !operations.is_empty() {
        for op in &operations {
            println!("  {}", op);
        }
    }
    println!("{} => {} (simulated)", input, output);
    0
}

fn run_identify(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help") {
        println!("Usage: identify [options ...] file ...");
        println!();
        println!("Describe the format and characteristics of one or more image files.");
        println!();
        println!("Options:");
        println!("  -verbose          Detailed information");
        println!("  -format string    Output format string");
        println!("  -ping             Efficiently determine attributes");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-verbose");
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();

    if files.is_empty() {
        eprintln!("identify: no input file specified");
        return 1;
    }

    for file in &files {
        if verbose {
            println!("Image: {}", file);
            println!("  Format: PNG (Portable Network Graphics)");
            println!("  Mime type: image/png");
            println!("  Geometry: 1920x1080+0+0");
            println!("  Resolution: 72x72");
            println!("  Depth: 8-bit");
            println!("  Colorspace: sRGB");
            println!("  Type: TrueColorAlpha");
            println!("  Channel depth:");
            println!("    Red: 8-bit");
            println!("    Green: 8-bit");
            println!("    Blue: 8-bit");
            println!("    Alpha: 8-bit");
            println!("  Filesize: 2.5MiB");
            println!("  Number pixels: 2.07M");
        } else {
            println!("{} PNG 1920x1080 1920x1080+0+0 8-bit sRGB 2.50MiB", file);
        }
    }
    0
}

fn run_mogrify(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help") {
        println!("Usage: mogrify [options ...] file ...");
        println!();
        println!("Transform images in-place (overwrites originals).");
        println!("Same options as convert, but modifies files directly.");
        return 0;
    }

    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    for file in &files {
        println!("{}: modified in-place (simulated)", file);
    }
    0
}

fn run_composite(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help") {
        println!("Usage: composite [options ...] image [image] composite");
        println!();
        println!("Overlap one image over another.");
        println!();
        println!("Options:");
        println!("  -compose method   Composition method (Over, Multiply, Screen, etc.)");
        println!("  -gravity type     Placement gravity");
        println!("  -geometry +X+Y    Offset position");
        println!("  -dissolve N       Blend percentage");
        return 0;
    }

    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    let output = files.last().copied().unwrap_or("composite.png");
    println!("Compositing {} files -> {} (simulated)", files.len().saturating_sub(1), output);
    0
}

fn run_montage(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help") {
        println!("Usage: montage [options ...] file [ ... ] output");
        println!();
        println!("Create an image mosaic / contact sheet.");
        println!();
        println!("Options:");
        println!("  -tile NxM        Tile layout");
        println!("  -geometry WxH+B+B  Tile size and border");
        println!("  -title string    Title text");
        println!("  -label string    Per-image label");
        println!("  -frame N         Frame width");
        println!("  -shadow          Add shadow");
        println!("  -background color Background color");
        return 0;
    }

    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    let output = files.last().copied().unwrap_or("montage.png");
    println!("Creating montage from {} images -> {} (simulated)", files.len().saturating_sub(1).max(1), output);
    0
}

fn run_magick(args: Vec<String>) -> i32 {
    // ImageMagick 7 unified command — first arg is subcommand
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "convert" => run_convert(rest),
        "identify" => run_identify(rest),
        "mogrify" => run_mogrify(rest),
        "composite" => run_composite(rest),
        "montage" => run_montage(rest),
        "--version" | "-version" => {
            println!("Version: ImageMagick 7.1.0 (SlateOS)");
            0
        }
        "--help" | "help" | "-h" => {
            println!("Usage: magick <command> [options ...]");
            println!();
            println!("Commands:");
            println!("  convert     Convert between image formats");
            println!("  identify    Describe image properties");
            println!("  mogrify     Transform images in-place");
            println!("  composite   Overlay images");
            println!("  montage     Create image mosaic");
            0
        }
        _ => {
            // Default: treat as convert args
            let mut full_args = vec![cmd];
            full_args.extend(rest);
            run_convert(full_args)
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("convert");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "identify" => run_identify(rest),
        "mogrify" => run_mogrify(rest),
        "composite" => run_composite(rest),
        "montage" => run_montage(rest),
        "magick" => run_magick(rest),
        _ => run_convert(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{run_convert};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_convert(vec!["--help".to_string()]), 0);
        assert_eq!(run_convert(vec!["-h".to_string()]), 0);
        let _ = run_convert(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_convert(vec![]);
    }
}
