#![deny(clippy::all)]

//! modprobe-cli — Slate OS kernel module tools
//!
//! Multi-personality: `modprobe`, `insmod`, `rmmod`, `lsmod`, `modinfo`, `depmod`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_modprobe(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: modprobe [OPTIONS] MODULE [PARAMS]");
        println!();
        println!("modprobe — add/remove kernel modules (Slate OS).");
        println!();
        println!("Options:");
        println!("  -r, --remove     Remove module");
        println!("  -n, --dry-run    Dry run");
        println!("  -v, --verbose    Verbose");
        println!("  -q, --quiet      Quiet");
        println!("  -a, --all        Insert all modules");
        println!("  --show-depends   Show dependencies");
        println!("  -c, --showconfig Show config");
        println!("  -D, --show-modversions  Show module versions");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("kmod version 31 (Slate OS)");
        return 0;
    }

    let remove = args.iter().any(|a| a == "-r" || a == "--remove");
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let show_depends = args.iter().any(|a| a == "--show-depends");

    let module = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("");

    if module.is_empty() {
        eprintln!("modprobe: missing module name");
        return 1;
    }

    if show_depends {
        println!("insmod /lib/modules/1.0.0/kernel/drivers/deps/{}.ko", module);
        return 0;
    }

    if remove {
        if verbose {
            println!("rmmod {}", module);
        }
        println!("modprobe: removed '{}'", module);
    } else {
        if verbose {
            println!("insmod /lib/modules/1.0.0/kernel/drivers/{}.ko", module);
        }
        println!("modprobe: inserted '{}'", module);
    }
    0
}

fn run_insmod(args: &[String]) -> i32 {
    let module = args.first().map(|s| s.as_str()).unwrap_or("");
    if module.is_empty() || module == "--help" {
        println!("Usage: insmod MODULE [PARAMS]");
        return if module == "--help" { 0 } else { 1 };
    }
    println!("insmod: loading module '{}'", module);
    0
}

fn run_rmmod(args: &[String]) -> i32 {
    let module = args.first().map(|s| s.as_str()).unwrap_or("");
    if module.is_empty() || module == "--help" {
        println!("Usage: rmmod MODULE");
        return if module == "--help" { 0 } else { 1 };
    }
    println!("rmmod: unloading module '{}'", module);
    0
}

fn run_lsmod(_args: &[String]) -> i32 {
    println!("Module                  Size  Used by");
    println!("nvidia_drm            102400  8");
    println!("nvidia_modeset       1478656  14 nvidia_drm");
    println!("nvidia              62324736  678 nvidia_drm,nvidia_modeset");
    println!("snd_hda_intel          65536  4");
    println!("snd_hda_codec_hdmi    102400  1");
    println!("snd_hda_codec         200704  2 snd_hda_codec_hdmi,snd_hda_intel");
    println!("snd_hda_core          131072  3 snd_hda_codec,snd_hda_codec_hdmi,snd_hda_intel");
    println!("iwlwifi               536576  1 iwlmvm");
    println!("iwlmvm                655360  0");
    println!("igc                    262144  0");
    println!("i915                 3145728  12");
    println!("xhci_hcd              393216  1 xhci_pci");
    println!("nvme                  131072  3");
    println!("nvme_core             196608  5 nvme");
    println!("ext4                  999424  2");
    0
}

fn run_modinfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: modinfo [OPTIONS] MODULE...");
        println!();
        println!("modinfo — show kernel module information (Slate OS).");
        return 0;
    }

    let module = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("nvidia");
    println!("filename:       /lib/modules/1.0.0/kernel/drivers/{}.ko", module);
    println!("license:        GPL");
    println!("description:    {} kernel module", module);
    println!("author:         Slate OS");
    println!("srcversion:     ABCDEF0123456789");
    println!("alias:          pci:v*d*sv*sd*bc03sc00i00*");
    println!("depends:        ");
    println!("retpoline:      Y");
    println!("intree:         Y");
    println!("name:           {}", module);
    println!("vermagic:       1.0.0 SMP preempt mod_unload");
    0
}

fn run_depmod(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: depmod [OPTIONS] [VERSION]");
        println!();
        println!("depmod — generate module dependency file (Slate OS).");
        return 0;
    }
    let verbose = args.iter().any(|a| a == "-v");
    if verbose {
        println!("depmod: scanning /lib/modules/1.0.0...");
    }
    println!("depmod: generating modules.dep");
    println!("depmod: generating modules.alias");
    println!("depmod: generating modules.symbols");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "modprobe".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "insmod" => run_insmod(&rest),
        "rmmod" => run_rmmod(&rest),
        "lsmod" => run_lsmod(&rest),
        "modinfo" => run_modinfo(&rest),
        "depmod" => run_depmod(&rest),
        _ => run_modprobe(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_modprobe};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/modprobe"), "modprobe");
        assert_eq!(basename(r"C:\bin\modprobe.exe"), "modprobe.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("modprobe.exe"), "modprobe");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_modprobe(&["--help".to_string()]), 0);
        assert_eq!(run_modprobe(&["-h".to_string()]), 0);
        let _ = run_modprobe(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_modprobe(&[]);
    }
}
