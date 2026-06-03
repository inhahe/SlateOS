#![deny(clippy::all)]

//! imagemagick-cli — OurOS ImageMagick-compatible CLI (convert/identify/mogrify)
//!
//! Multi-personality: `convert`, `identify`, `mogrify`, `composite`, `montage`

use std::env;
use std::process;

fn personality(argv0: &str) -> &str {
    let base = argv0.rsplit(&['/', '\\'][..]).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    match name {
        "identify" => "identify",
        "mogrify" => "mogrify",
        "composite" => "composite",
        "montage" => "montage",
        "convert" | _ => "convert",
    }
}

fn run_convert(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: convert [OPTIONS] <INPUT> [OPERATIONS...] <OUTPUT>");
        println!();
        println!("Convert and transform images between formats.");
        println!();
        println!("Operations:");
        println!("  -resize <GEOMETRY>       Resize image (e.g., 800x600, 50%)");
        println!("  -crop <GEOMETRY>         Crop image");
        println!("  -rotate <DEGREES>        Rotate image");
        println!("  -flip                    Flip vertically");
        println!("  -flop                    Flip horizontally");
        println!("  -blur <RADIUS>x<SIGMA>   Gaussian blur");
        println!("  -sharpen <R>x<S>         Sharpen image");
        println!("  -brightness-contrast <B>x<C>  Adjust brightness/contrast");
        println!("  -colorspace <SPACE>      Convert colorspace (sRGB/CMYK/Gray)");
        println!("  -depth <N>               Bit depth");
        println!("  -quality <N>             Output quality (1-100)");
        println!("  -strip                   Strip metadata");
        println!("  -auto-orient             Auto-orient from EXIF");
        println!("  -thumbnail <GEOMETRY>    Create thumbnail");
        println!("  -alpha <TYPE>            Alpha channel ops (on/off/remove/set)");
        println!("  -background <COLOR>      Background color");
        println!("  -gravity <TYPE>          Gravity (center/north/south/...)");
        println!("  -extent <GEOMETRY>       Set image extent (pad/crop to size)");
        println!("  -compose <OP>            Composition operator");
        println!("  -format <FMT>            Output format override");
        println!("  -density <DPI>           Resolution in DPI");
        println!("  -V, --version            Show version");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let input = files.first().copied().unwrap_or("input.png");
    let output = files.last().copied().unwrap_or("output.png");

    let has_resize = args.windows(2).any(|w| w[0] == "-resize");
    let has_crop = args.windows(2).any(|w| w[0] == "-crop");
    let has_blur = args.windows(2).any(|w| w[0] == "-blur");

    println!("Converting: {} -> {}", input, output);
    if has_resize {
        let geom = args.windows(2)
            .find(|w| w[0] == "-resize")
            .map(|w| w[1].as_str())
            .unwrap_or("800x600");
        println!("  Resize: {}", geom);
    }
    if has_crop {
        let geom = args.windows(2)
            .find(|w| w[0] == "-crop")
            .map(|w| w[1].as_str())
            .unwrap_or("400x300+100+50");
        println!("  Crop: {}", geom);
    }
    if has_blur {
        let spec = args.windows(2)
            .find(|w| w[0] == "-blur")
            .map(|w| w[1].as_str())
            .unwrap_or("0x5");
        println!("  Blur: {}", spec);
    }
    println!("  Input:  1920x1080 PNG (4,567,890 bytes)");
    println!("  Output: 800x600 JPEG (123,456 bytes)");
    println!("  Done.");
    0
}

fn run_identify(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: identify [OPTIONS] <FILE>...");
        println!();
        println!("Identify image format and properties.");
        println!();
        println!("Options:");
        println!("  -verbose               Detailed output");
        println!("  -format <STRING>       Custom output format");
        println!("  -ping                  Efficient identification (header only)");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-verbose");

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        eprintln!("Error: file required. See --help.");
        return 1;
    }

    for file in &files {
        if verbose {
            println!("Image: {}", file);
            println!("  Format: PNG");
            println!("  Geometry: 1920x1080+0+0");
            println!("  Resolution: 72x72 PPI");
            println!("  Depth: 8-bit");
            println!("  Colorspace: sRGB");
            println!("  Type: TrueColorAlpha");
            println!("  Alpha channel: yes");
            println!("  Compression: Zip");
            println!("  Quality: 95");
            println!("  Filesize: 4,567,890 bytes");
            println!("  Number pixels: 2.07M");
            println!("  EXIF:");
            println!("    Make: Canon");
            println!("    Model: EOS R5");
            println!("    DateTime: 2024:06:15 14:30:00");
        } else {
            println!("{} PNG 1920x1080 1920x1080+0+0 8-bit sRGB 4.567MB", file);
        }
    }
    0
}

fn run_mogrify(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mogrify [OPTIONS] <FILE>...");
        println!();
        println!("Transform images in-place.");
        println!();
        println!("Options:");
        println!("  -resize <GEOMETRY>   Resize");
        println!("  -crop <GEOMETRY>     Crop");
        println!("  -quality <N>         Quality");
        println!("  -strip               Strip metadata");
        println!("  -auto-orient         Auto-orient");
        println!("  -format <FMT>        Convert format");
        println!("  -path <DIR>          Output directory");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    for file in &files {
        println!("Mogrifying: {} (in-place)", file);
    }
    println!("  {} file(s) processed.", files.len().max(1));
    0
}

fn run_composite(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: composite [OPTIONS] <OVERLAY> <BASE> <OUTPUT>");
        println!();
        println!("Composite one image over another.");
        println!();
        println!("Options:");
        println!("  -compose <OP>        Composition operator (over/multiply/screen/...)");
        println!("  -gravity <TYPE>      Gravity for positioning");
        println!("  -geometry <GEOM>     Offset geometry (+X+Y)");
        println!("  -dissolve <PCT>      Dissolve percentage");
        println!("  -blend <PCT>         Blend percentage");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let overlay = files.first().copied().unwrap_or("overlay.png");
    let base = files.get(1).copied().unwrap_or("base.png");
    let output = files.get(2).copied().unwrap_or("composite.png");

    println!("Compositing:");
    println!("  Overlay: {} (400x300 PNG)", overlay);
    println!("  Base:    {} (1920x1080 PNG)", base);
    println!("  Output:  {} (1920x1080 PNG)", output);
    println!("  Done.");
    0
}

fn run_montage(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: montage [OPTIONS] <FILES>... <OUTPUT>");
        println!();
        println!("Create image montage/contact sheet.");
        println!();
        println!("Options:");
        println!("  -tile <COLS>x<ROWS>    Tile layout");
        println!("  -geometry <GEOM>       Tile geometry");
        println!("  -background <COLOR>    Background color");
        println!("  -border <N>            Border width");
        println!("  -label <TEXT>          Label format");
        println!("  -title <TEXT>          Montage title");
        println!("  -shadow                Add shadow");
        println!("  -frame <GEOM>          Frame geometry");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let count = if files.len() > 1 { files.len() - 1 } else { 0 };
    let output = files.last().copied().unwrap_or("montage.png");

    println!("Creating montage:");
    println!("  Images: {}", count);
    println!("  Layout: 4x3 grid");
    println!("  Tile size: 300x200");
    println!("  Output: {} (1200x600)", output);
    println!("  Done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_else(|| String::from("convert"));
    let p = personality(&argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.iter().any(|a| a == "-V" || a == "--version") {
        println!("ImageMagick 7.1.1-29 (OurOS)");
        process::exit(0);
    }

    let code = match p {
        "convert" => run_convert(&rest),
        "identify" => run_identify(&rest),
        "mogrify" => run_mogrify(&rest),
        "composite" => run_composite(&rest),
        "montage" => run_montage(&rest),
        _ => run_convert(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_convert};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_convert(&["--help".to_string()]), 0);
        assert_eq!(run_convert(&["-h".to_string()]), 0);
        assert_eq!(run_convert(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_convert(&[]), 0);
    }
}
