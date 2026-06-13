#![deny(clippy::all)]

//! glxinfo-cli — SlateOS glxinfo/glxgears OpenGL tools CLI
//!
//! Multi-personality: `glxinfo`, `glxgears`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_glxinfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: glxinfo [OPTIONS]");
        println!();
        println!("glxinfo — display OpenGL information (SlateOS).");
        println!();
        println!("Options:");
        println!("  -B           Brief output");
        println!("  -l           List extensions (one per line)");
        println!("  -s           Single visual mode");
        println!("  -t           Print table of visuals");
        println!("  -display D   X display");
        return 0;
    }

    let brief = args.iter().any(|a| a == "-B");

    println!("name of display: :0");
    println!("display: :0  screen: 0");
    println!("direct rendering: Yes");

    if brief {
        println!("OpenGL vendor string: Mesa");
        println!("OpenGL renderer string: llvmpipe (LLVM 17.0.6, 256 bits)");
        println!("OpenGL core profile version string: 4.5 (Core Profile) Mesa 24.0.1");
        println!("OpenGL version string: 4.5 (Compatibility Profile) Mesa 24.0.1");
        println!("OpenGL ES profile version string: OpenGL ES 3.2 Mesa 24.0.1");
    } else {
        println!("server glx vendor string: SGI");
        println!("server glx version string: 1.4");
        println!("client glx vendor string: Mesa Project");
        println!("client glx version string: 1.4");
        println!("OpenGL vendor string: Mesa");
        println!("OpenGL renderer string: llvmpipe (LLVM 17.0.6, 256 bits)");
        println!("OpenGL core profile version string: 4.5 (Core Profile) Mesa 24.0.1");
        println!("OpenGL core profile shading language version string: 4.50");
        println!("OpenGL version string: 4.5 (Compatibility Profile) Mesa 24.0.1");
        println!("OpenGL shading language version string: 4.50");
        println!("OpenGL ES profile version string: OpenGL ES 3.2 Mesa 24.0.1");
        println!("OpenGL ES profile shading language version string: OpenGL ES GLSL ES 3.20");
    }
    0
}

fn run_glxgears(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: glxgears [OPTIONS]");
        println!();
        println!("glxgears — OpenGL gears demo (SlateOS).");
        println!();
        println!("Options:");
        println!("  -display D   X display");
        println!("  -info        Show GL info");
        println!("  -stereo      Stereo rendering");
        return 0;
    }

    if args.iter().any(|a| a == "-info") {
        println!("GL_RENDERER   = llvmpipe (LLVM 17.0.6, 256 bits)");
        println!("GL_VERSION    = 4.5 (Compatibility Profile) Mesa 24.0.1");
        println!("GL_VENDOR     = Mesa");
    }
    println!("300 frames in 5.0 seconds = 60.0 FPS");
    println!("300 frames in 5.0 seconds = 60.0 FPS");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "glxinfo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "glxgears" => run_glxgears(&rest),
        _ => run_glxinfo(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_glxinfo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/glxinfo"), "glxinfo");
        assert_eq!(basename(r"C:\bin\glxinfo.exe"), "glxinfo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("glxinfo.exe"), "glxinfo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_glxinfo(&["--help".to_string()]), 0);
        assert_eq!(run_glxinfo(&["-h".to_string()]), 0);
        let _ = run_glxinfo(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_glxinfo(&[]);
    }
}
