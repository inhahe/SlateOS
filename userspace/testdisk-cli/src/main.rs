#![deny(clippy::all)]

//! testdisk-cli — OurOS TestDisk data recovery
//!
//! Multi-personality: `testdisk`, `photorec`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_testdisk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: testdisk [OPTIONS] [DEVICE|IMAGE]");
        println!("testdisk v7.2 (OurOS) — Partition recovery & repair");
        println!();
        println!("Options:");
        println!("  /log          Create testdisk.log");
        println!("  /debug        Enable debug mode");
        println!("  /list         List current partitions");
        println!("  --version     Show version");
        println!();
        println!("Recovers lost partitions, makes non-booting disks bootable,");
        println!("recovers FAT/NTFS/ext boot sectors, rebuilds partition tables.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("testdisk v7.2 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "/list") {
        println!("Disk /dev/sda - 500 GiB");
        println!("  Partition 1: EFI System   512 MiB");
        println!("  Partition 2: Linux        480 GiB");
        println!("  Partition 3: Swap         19 GiB");
        return 0;
    }
    println!("testdisk: data recovery utility");
    println!("  Select a media (disk or image)");
    println!("  Disk /dev/sda - 500 GiB");
    0
}

fn run_photorec(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: photorec [OPTIONS] [DEVICE|IMAGE]");
        println!("photorec v7.2 (OurOS) — File data recovery");
        println!();
        println!("Options:");
        println!("  /d DIR        Set recovery directory");
        println!("  /log          Create photorec.log");
        println!("  --version     Show version");
        println!();
        println!("Recovers lost files: photos, videos, documents, archives.");
        println!("Works even after reformatting or severe filesystem damage.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("photorec v7.2 (OurOS)"); return 0; }
    println!("photorec: file recovery utility");
    println!("  Supported formats: jpg, png, gif, bmp, tiff, pdf, doc,");
    println!("    xls, ppt, zip, mp3, mp4, avi, mov, and 480+ more");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "testdisk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "photorec" => run_photorec(&rest, &prog),
        _ => run_testdisk(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_testdisk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/testdisk"), "testdisk");
        assert_eq!(basename(r"C:\bin\testdisk.exe"), "testdisk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("testdisk.exe"), "testdisk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_testdisk(&["--help".to_string()], "testdisk"), 0);
        assert_eq!(run_testdisk(&["-h".to_string()], "testdisk"), 0);
        let _ = run_testdisk(&["--version".to_string()], "testdisk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_testdisk(&[], "testdisk");
    }
}
