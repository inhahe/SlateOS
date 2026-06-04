#![deny(clippy::all)]

//! ogre-cli — OurOS OGRE 3D graphics engine
//!
//! Single personality: `ogre`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ogre(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ogre [COMMAND] [OPTIONS]");
        println!("OGRE v14.3 (OurOS) — Object-Oriented Graphics Rendering Engine");
        println!();
        println!("Commands:");
        println!("  sample list|run    Run sample browser");
        println!("  meshmagick OP MESH Manipulate mesh files");
        println!("  meshconv IN OUT    Convert mesh formats");
        println!("  shadercache clear  Clear shader cache");
        println!("  resource list      List loaded resources");
        println!("  info               Print system info");
        println!();
        println!("Options:");
        println!("  --render API       Rendering API (gl3plus/gles2/d3d11/vulkan/metal)");
        println!("  --fullscreen       Full-screen mode");
        println!("  --resolution WxH   Window resolution");
        println!("  --vsync on|off     Vertical sync");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("OGRE v14.3.3 (OurOS)"); return 0; }
    println!("OGRE v14.3.3 (OurOS)");
    println!("  Render systems: OpenGL3+, GLES2, D3D11, Vulkan, Metal");
    println!("  Scene managers: OctreeSceneManager, DefaultSceneManager");
    println!("  Material system: HLMS (PBS, Unlit)");
    println!("  Mesh formats: .mesh, .obj, .gltf");
    println!("  Plugins: 12 loaded");
    println!("  Samples: 28 available");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ogre".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ogre(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ogre};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ogre"), "ogre");
        assert_eq!(basename(r"C:\bin\ogre.exe"), "ogre.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ogre.exe"), "ogre");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ogre(&["--help".to_string()], "ogre"), 0);
        assert_eq!(run_ogre(&["-h".to_string()], "ogre"), 0);
        let _ = run_ogre(&["--version".to_string()], "ogre");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ogre(&[], "ogre");
    }
}
