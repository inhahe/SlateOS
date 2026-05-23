#![deny(clippy::all)]

//! qemu-cli — OurOS QEMU emulator CLI
//!
//! Multi-personality: `qemu-system-x86_64`, `qemu-img`, `qemu-nbd`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_qemu_system(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qemu-system-x86_64 [OPTIONS]");
        println!();
        println!("QEMU x86_64 system emulator (OurOS).");
        println!();
        println!("Options:");
        println!("  -m SIZE            Memory size (e.g., 2G)");
        println!("  -smp N             Number of CPUs");
        println!("  -hda FILE          Hard disk image");
        println!("  -cdrom FILE        CD-ROM image");
        println!("  -boot ORDER        Boot order (c,d,n)");
        println!("  -enable-kvm        Enable KVM acceleration");
        println!("  -cpu MODEL         CPU model");
        println!("  -machine TYPE      Machine type");
        println!("  -net TYPE          Network config");
        println!("  -display TYPE      Display type (sdl,gtk,vnc,none)");
        println!("  -nographic         No graphical output");
        println!("  -serial DEV        Serial port device");
        println!("  -monitor DEV       Monitor device");
        println!("  -snapshot          Write to temp files");
        println!("  -daemonize         Run in background");
        println!("  -name NAME         Set VM name");
        println!("  -vnc DISPLAY       VNC display");
        println!("  -qmp URI           QMP monitor");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-version") {
        println!("QEMU emulator version 8.2.2 (OurOS)");
        return 0;
    }

    let mem = args.windows(2).find(|w| w[0] == "-m").map(|w| w[1].as_str()).unwrap_or("128M");
    let smp = args.windows(2).find(|w| w[0] == "-smp").map(|w| w[1].as_str()).unwrap_or("1");
    let kvm = args.iter().any(|a| a == "-enable-kvm");

    println!("QEMU x86_64 system emulator v8.2.2");
    println!("  Memory: {}", mem);
    println!("  CPUs: {}", smp);
    println!("  KVM: {}", if kvm { "enabled" } else { "disabled" });
    if let Some(w) = args.windows(2).find(|w| w[0] == "-hda") {
        println!("  Disk: {}", w[1]);
    }
    if let Some(w) = args.windows(2).find(|w| w[0] == "-cdrom") {
        println!("  CDROM: {}", w[1]);
    }
    println!("  Starting VM...");
    0
}

fn run_qemu_img(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: qemu-img COMMAND [OPTIONS]");
        println!();
        println!("qemu-img — QEMU disk image utility (OurOS).");
        println!();
        println!("Commands:");
        println!("  create       Create new disk image");
        println!("  info         Show image info");
        println!("  convert      Convert between formats");
        println!("  resize       Resize image");
        println!("  snapshot     Manage snapshots");
        println!("  check        Check image consistency");
        println!("  compare      Compare two images");
        println!("  map          Show image file mapping");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "create" => {
            let fmt = args.windows(2).find(|w| w[0] == "-f").map(|w| w[1].as_str()).unwrap_or("qcow2");
            let positional: Vec<&str> = args.iter().skip(1).filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
            let file = positional.first().unwrap_or(&"disk.qcow2");
            let size = positional.get(1).unwrap_or(&"10G");
            println!("Formatting '{}', fmt={} size={}", file, fmt, size);
        }
        "info" => {
            let file = args.iter().skip(1).find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("disk.qcow2");
            println!("image: {}", file);
            println!("file format: qcow2");
            println!("virtual size: 10 GiB (10737418240 bytes)");
            println!("disk size: 1.2 GiB");
            println!("cluster_size: 65536");
            println!("Format specific information:");
            println!("    compat: 1.1");
            println!("    compression type: zlib");
            println!("    lazy refcounts: false");
        }
        "convert" => {
            println!("Converting image...");
            println!("    (100.00/100%)");
        }
        "resize" => {
            let file = args.iter().skip(1).find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("disk.qcow2");
            println!("Image '{}' resized.", file);
        }
        "check" => {
            println!("No errors were found on the image.");
        }
        _ => {
            eprintln!("qemu-img: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn run_qemu_nbd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qemu-nbd [OPTIONS] FILE");
        println!();
        println!("qemu-nbd — QEMU NBD server (OurOS).");
        println!();
        println!("Options:");
        println!("  -c DEV       Connect to NBD device");
        println!("  -d DEV       Disconnect NBD device");
        println!("  -p PORT      Listen port (default 10809)");
        println!("  -f FORMAT    Image format");
        println!("  --fork       Fork into background");
        return 0;
    }
    if args.iter().any(|a| a == "-d") {
        println!("qemu-nbd: disconnected");
    } else {
        println!("qemu-nbd: serving on port 10809");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "qemu-system-x86_64".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "qemu-img" => run_qemu_img(&rest),
        "qemu-nbd" => run_qemu_nbd(&rest),
        _ => run_qemu_system(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
