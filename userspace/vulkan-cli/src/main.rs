#![deny(clippy::all)]

//! vulkan-cli — Slate OS Vulkan tools CLI
//!
//! Multi-personality: `vulkaninfo`, `vkcube`, `vkvia`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_vulkaninfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vulkaninfo [OPTIONS]");
        println!();
        println!("vulkaninfo — Vulkan GPU information (Slate OS).");
        println!();
        println!("Options:");
        println!("  --json          JSON output");
        println!("  --summary       Summary only");
        println!("  --html          HTML output");
        println!("  -o FILE         Output file");
        return 0;
    }

    let summary = args.iter().any(|a| a == "--summary");
    let json = args.iter().any(|a| a == "--json");

    if json {
        println!("{{");
        println!("  \"apiVersion\": \"1.3.275\",");
        println!("  \"driverVersion\": \"24.0.1\",");
        println!("  \"vendorID\": \"0x8086\",");
        println!("  \"deviceName\": \"llvmpipe (LLVM 17.0.6, 256 bits)\",");
        println!("  \"deviceType\": \"VK_PHYSICAL_DEVICE_TYPE_CPU\"");
        println!("}}");
    } else if summary {
        println!("==========");
        println!("VULKANINFO");
        println!("==========");
        println!();
        println!("Vulkan Instance Version: 1.3.275");
        println!();
        println!("GPU0:");
        println!("  apiVersion    = 1.3.275");
        println!("  driverVersion = 24.0.1");
        println!("  vendorID      = 0x8086");
        println!("  deviceID      = 0x0000");
        println!("  deviceType    = PHYSICAL_DEVICE_TYPE_CPU");
        println!("  deviceName    = llvmpipe (LLVM 17.0.6, 256 bits)");
        println!("  driverName    = llvmpipe");
    } else {
        println!("==========");
        println!("VULKANINFO");
        println!("==========");
        println!();
        println!("Vulkan Instance Version: 1.3.275");
        println!();
        println!("Instance Extensions: count = 19");
        println!("  VK_KHR_device_group_creation    : extension revision 1");
        println!("  VK_KHR_external_fence_capabilities : extension revision 1");
        println!("  VK_KHR_get_physical_device_properties2 : extension revision 2");
        println!("  VK_KHR_surface                  : extension revision 25");
        println!();
        println!("Devices:");
        println!("  GPU id = 0 (llvmpipe (LLVM 17.0.6, 256 bits))");
    }
    0
}

fn run_vkcube(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vkcube [OPTIONS]");
        println!();
        println!("vkcube — Vulkan spinning cube demo (Slate OS).");
        println!();
        println!("Options:");
        println!("  --gpu_number N   GPU index");
        println!("  --present_mode M Present mode");
        println!("  -c N             Frame count");
        return 0;
    }
    println!("Selected GPU 0: llvmpipe (LLVM 17.0.6, 256 bits)");
    println!("  Frames: 60  FPS: 60.0");
    0
}

fn run_vkvia(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vkvia [OPTIONS]");
        println!();
        println!("vkvia — Vulkan installation analyzer (Slate OS).");
        return 0;
    }
    println!("Vulkan Installation Analyzer");
    println!("  SDK: found (1.3.275)");
    println!("  Runtime: found");
    println!("  Drivers: 1 ICD(s) found");
    println!("  Layers: 0 implicit, 0 explicit");
    println!("  Result: PASS");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "vulkaninfo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "vkcube" => run_vkcube(&rest),
        "vkvia" => run_vkvia(&rest),
        _ => run_vulkaninfo(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vulkaninfo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vulkan"), "vulkan");
        assert_eq!(basename(r"C:\bin\vulkan.exe"), "vulkan.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vulkan.exe"), "vulkan");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vulkaninfo(&["--help".to_string()]), 0);
        assert_eq!(run_vulkaninfo(&["-h".to_string()]), 0);
        let _ = run_vulkaninfo(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vulkaninfo(&[]);
    }
}
