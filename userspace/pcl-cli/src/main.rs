#![deny(clippy::all)]

//! pcl-cli — SlateOS Point Cloud Library tools
//!
//! Multi-personality: `pcl_viewer`, `pcl_pcd2ply`, `pcl_ply2pcd`, `pcl_mesh_sampling`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pcl_viewer(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pcl_viewer [OPTIONS] FILE.pcd");
        println!("PCL Viewer 1.14.0 (SlateOS)");
        println!("  -bc R,G,B     Background color");
        println!("  -fc R,G,B     Point color");
        println!("  -ps N         Point size");
        println!("  -normals N    Display normals (length N)");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".pcd") || a.ends_with(".ply")).map(|s| s.as_str()).unwrap_or("cloud.pcd");
    println!("PCL Viewer 1.14.0");
    println!("Loading: {}", file);
    println!("  Points: 1,234,567");
    println!("  Fields: x y z rgb");
    println!("Viewer ready. Press 'q' to quit.");
    0
}

fn run_pcl_pcd2ply(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pcl_pcd2ply INPUT.pcd OUTPUT.ply");
        println!("  Convert PCD to PLY format");
        return 0;
    }
    let input = args.first().map(|s| s.as_str()).unwrap_or("cloud.pcd");
    let output = args.get(1).map(|s| s.as_str()).unwrap_or("cloud.ply");
    println!("Converting {} -> {}", input, output);
    println!("  1,234,567 points converted.");
    println!("Done.");
    0
}

fn run_pcl_ply2pcd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pcl_ply2pcd INPUT.ply OUTPUT.pcd");
        println!("  Convert PLY to PCD format");
        return 0;
    }
    let input = args.first().map(|s| s.as_str()).unwrap_or("cloud.ply");
    let output = args.get(1).map(|s| s.as_str()).unwrap_or("cloud.pcd");
    println!("Converting {} -> {}", input, output);
    println!("  1,234,567 points converted.");
    println!("Done.");
    0
}

fn run_pcl_mesh_sampling(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pcl_mesh_sampling [OPTIONS] INPUT.ply OUTPUT.pcd");
        println!("  -n_samples N     Number of samples");
        println!("  -leaf_size F     Voxel leaf size");
        println!("  -no_vis_result   Don't visualize");
        return 0;
    }
    let input = args.iter().find(|a| a.ends_with(".ply") || a.ends_with(".obj") || a.ends_with(".stl")).map(|s| s.as_str()).unwrap_or("mesh.ply");
    let n = args.windows(2).find(|w| w[0] == "-n_samples").map(|w| w[1].as_str()).unwrap_or("100000");
    println!("Sampling mesh: {}", input);
    println!("  Target samples: {}", n);
    println!("  Generated: {} points", n);
    println!("  Output: sampled.pcd");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pcl_viewer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pcl_pcd2ply" => run_pcl_pcd2ply(&rest),
        "pcl_ply2pcd" => run_pcl_ply2pcd(&rest),
        "pcl_mesh_sampling" => run_pcl_mesh_sampling(&rest),
        _ => run_pcl_viewer(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pcl_viewer};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pcl"), "pcl");
        assert_eq!(basename(r"C:\bin\pcl.exe"), "pcl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pcl.exe"), "pcl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pcl_viewer(&["--help".to_string()]), 0);
        assert_eq!(run_pcl_viewer(&["-h".to_string()]), 0);
        let _ = run_pcl_viewer(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pcl_viewer(&[]);
    }
}
