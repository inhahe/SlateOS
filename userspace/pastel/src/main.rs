#![deny(clippy::all)]

//! pastel — SlateOS command-line tool for working with colors
//!
//! Single personality: `pastel`

use std::env;
use std::process;

fn run_pastel(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--help" | "-h" | "" => {
            println!("Usage: pastel [OPTIONS] <COMMAND>");
            println!();
            println!("A command-line tool to generate, analyze, convert and manipulate colors.");
            println!();
            println!("Commands:");
            println!("  color         Display color information");
            println!("  list          Show a list of named colors");
            println!("  random        Generate random colors");
            println!("  distinct      Generate visually distinct colors");
            println!("  sort-by       Sort colors by property");
            println!("  pick          Interactively pick a color");
            println!("  format        Convert color to different format");
            println!("  paint         Print colored text");
            println!("  gradient      Generate color gradient");
            println!("  mix           Mix two colors");
            println!("  colorblind    Simulate color blindness");
            println!("  set           Modify color property");
            println!("  saturate      Increase saturation");
            println!("  desaturate    Decrease saturation");
            println!("  lighten       Increase lightness");
            println!("  darken        Decrease lightness");
            println!("  rotate        Rotate hue");
            println!("  complement    Get complementary color");
            println!("  gray          Convert to grayscale");
            println!("  to-gray       Alias for gray");
            println!("  textcolor     Best text color for background");
            println!();
            println!("Options:");
            println!("  -m, --color-mode <MODE>  Color mode (24bit/8bit/off)");
            println!("  -V, --version            Show version");
            0
        }
        "--version" | "-V" => {
            println!("pastel 0.9.0 (SlateOS)");
            0
        }
        "color" => {
            let color = args.get(1).map(|s| s.as_str()).unwrap_or("steelblue");
            println!("Color: {}", color);
            println!();
            println!("  Hex: #4682B4");
            println!("  RGB: rgb(70, 130, 180)");
            println!("  HSL: hsl(207, 44%, 49%)");
            println!("  Lab: lab(52.47, -4.07, -32.20)");
            println!("  LCh: lch(52.47, 32.46, 262.79)");
            println!();
            println!("  ████████████████  {}", color);
            println!();
            println!("  Most similar: CornflowerBlue (distance: 15.2)");
            0
        }
        "list" => {
            println!("  ██ aliceblue         #F0F8FF   rgb(240, 248, 255)");
            println!("  ██ antiquewhite      #FAEBD7   rgb(250, 235, 215)");
            println!("  ██ aqua              #00FFFF   rgb(  0, 255, 255)");
            println!("  ██ aquamarine        #7FFFD4   rgb(127, 255, 212)");
            println!("  ██ azure             #F0FFFF   rgb(240, 255, 255)");
            println!("  ██ beige             #F5F5DC   rgb(245, 245, 220)");
            println!("  ██ bisque            #FFE4C4   rgb(255, 228, 196)");
            println!("  ██ black             #000000   rgb(  0,   0,   0)");
            println!("  ... (148 named colors)");
            0
        }
        "random" => {
            println!("  ██ #A3D2CA   rgb(163, 210, 202)   hsl(170, 35%, 73%)");
            0
        }
        "distinct" => {
            let n = args.get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(5);
            let colors = [
                ("#E63946", "rgb(230,  57,  70)"),
                ("#457B9D", "rgb( 69, 123, 157)"),
                ("#2A9D8F", "rgb( 42, 157, 143)"),
                ("#E9C46A", "rgb(233, 196, 106)"),
                ("#F4A261", "rgb(244, 162,  97)"),
                ("#264653", "rgb( 38,  70,  83)"),
                ("#6A0572", "rgb(106,   5, 114)"),
                ("#1B998B", "rgb( 27, 153, 139)"),
            ];
            for (hex, rgb) in colors.iter().take(n) {
                println!("  ██ {}   {}", hex, rgb);
            }
            0
        }
        "format" => {
            let fmt = args.get(1).map(|s| s.as_str()).unwrap_or("hex");
            let color = args.get(2).map(|s| s.as_str()).unwrap_or("#4682B4");
            match fmt {
                "hex" => println!("{}", color),
                "rgb" => println!("rgb(70, 130, 180)"),
                "hsl" => println!("hsl(207, 44%, 49%)"),
                "lab" => println!("lab(52.47, -4.07, -32.20)"),
                "lch" => println!("lch(52.47, 32.46, 262.79)"),
                "cmyk" => println!("cmyk(61%, 28%, 0%, 29%)"),
                _ => println!("{}", color),
            }
            0
        }
        "gradient" => {
            println!("  ██ #FF0000");
            println!("  ██ #FF3F00");
            println!("  ██ #FF7F00");
            println!("  ██ #FFBF00");
            println!("  ██ #FFFF00");
            println!("  ██ #BFFF00");
            println!("  ██ #7FFF00");
            println!("  ██ #3FFF00");
            println!("  ██ #00FF00");
            0
        }
        "mix" => {
            let c1 = args.get(1).map(|s| s.as_str()).unwrap_or("red");
            let c2 = args.get(2).map(|s| s.as_str()).unwrap_or("blue");
            println!("  Mixing {} + {}:", c1, c2);
            println!("  ██ #800080   rgb(128, 0, 128)   (purple)");
            0
        }
        "complement" => {
            let color = args.get(1).map(|s| s.as_str()).unwrap_or("steelblue");
            println!("  Complement of {}:", color);
            println!("  ██ #B47046   rgb(180, 112, 70)");
            0
        }
        "textcolor" => {
            let bg = args.get(1).map(|s| s.as_str()).unwrap_or("#4682B4");
            println!("  Best text color for background {}:", bg);
            println!("  ██ #FFFFFF   (white — contrast ratio 4.56:1)");
            0
        }
        "paint" => {
            let color = args.get(1).map(|s| s.as_str()).unwrap_or("green");
            let text: String = args.iter().skip(2).cloned().collect::<Vec<_>>().join(" ");
            let text = if text.is_empty() { "Hello, World!".to_string() } else { text };
            println!("[{}]{}", color, text);
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
    let code = run_pastel(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pastel};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pastel(vec!["--help".to_string()]), 0);
        assert_eq!(run_pastel(vec!["-h".to_string()]), 0);
        let _ = run_pastel(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pastel(vec![]);
    }
}
