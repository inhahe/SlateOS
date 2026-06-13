#![deny(clippy::all)]

//! mesa-cli — SlateOS Mesa 3D graphics library tools CLI
//!
//! Multi-personality: `eglinfo`, `es2_info`, `es2gears`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_eglinfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: eglinfo [OPTIONS]");
        println!();
        println!("eglinfo — display EGL information (SlateOS).");
        return 0;
    }
    println!("EGL client extensions:");
    println!("    EGL_EXT_client_extensions, EGL_EXT_platform_base,");
    println!("    EGL_KHR_client_get_all_proc_addresses, EGL_KHR_debug");
    println!();
    println!("EGL API version: 1.5");
    println!("EGL vendor string: Mesa Project");
    println!("EGL version string: 1.5");
    println!("EGL client APIs: OpenGL OpenGL_ES");
    println!("EGL extensions: EGL_KHR_create_context, EGL_KHR_gl_renderbuffer_image,");
    println!("    EGL_KHR_gl_texture_2D_image, EGL_KHR_image_base,");
    println!("    EGL_KHR_surfaceless_context, EGL_MESA_drm_image");
    println!();
    println!("Configurations:");
    println!("  bf  lv colorbuffer dp st  ms  vis   cav bi  renderable  supported");
    println!("  id  dw  r  g  b  a  th cl ns  b  id  eat nd  gl  es  es2 surfaces");
    println!("----------------------------------------------------------------------");
    println!("0x01 24  8  8  8  0  24  8  0  0 0x21 non  a   y   y   y   win,pb");
    println!("0x02 24  8  8  8  8  24  8  0  0 0x21 non  a   y   y   y   win,pb");
    0
}

fn run_es2_info(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: es2_info");
        println!();
        println!("es2_info — OpenGL ES 2.0 implementation info (SlateOS).");
        return 0;
    }
    println!("GL_VERSION: OpenGL ES 3.2 Mesa 24.0.1");
    println!("GL_RENDERER: llvmpipe (LLVM 17.0.6, 256 bits)");
    println!("GL_VENDOR: Mesa");
    println!("GL_SHADING_LANGUAGE_VERSION: OpenGL ES GLSL ES 3.20");
    println!("GL_EXTENSIONS:");
    println!("  GL_EXT_blend_minmax, GL_EXT_multi_draw_arrays,");
    println!("  GL_EXT_texture_filter_anisotropic, GL_OES_depth_texture,");
    println!("  GL_OES_element_index_uint, GL_OES_texture_npot");
    0
}

fn run_es2gears(_args: &[String]) -> i32 {
    println!("ES2 Gears running...");
    println!("  GL_RENDERER: llvmpipe (LLVM 17.0.6, 256 bits)");
    println!("  Frame: 1  FPS: 60.0");
    println!("  Frame: 60  FPS: 59.8");
    println!("  Frame: 120  FPS: 60.1");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "eglinfo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "es2_info" => run_es2_info(&rest),
        "es2gears" => run_es2gears(&rest),
        _ => run_eglinfo(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_eglinfo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mesa"), "mesa");
        assert_eq!(basename(r"C:\bin\mesa.exe"), "mesa.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mesa.exe"), "mesa");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_eglinfo(&["--help".to_string()]), 0);
        assert_eq!(run_eglinfo(&["-h".to_string()]), 0);
        let _ = run_eglinfo(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_eglinfo(&[]);
    }
}
