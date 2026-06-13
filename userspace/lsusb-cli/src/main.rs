#![deny(clippy::all)]

//! lsusb-cli — Slate OS USB device lister
//!
//! Multi-personality: `lsusb`, `usb-devices`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_lsusb(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lsusb [OPTIONS]");
        println!();
        println!("lsusb — list USB devices (Slate OS).");
        println!();
        println!("Options:");
        println!("  -v              Verbose");
        println!("  -s BUS:DEV      Show only specified device");
        println!("  -d VENDOR:PROD  Show only specified vendor:product");
        println!("  -D DEVICE       Show only given device file");
        println!("  -t              Tree view");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("lsusb (usbutils) 017 (Slate OS)");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-v");
    let tree = args.iter().any(|a| a == "-t");

    if tree {
        println!("/:  Bus 02.Port 1: Dev 1, Class=root_hub, Driver=xhci_hcd/4p, 10000M");
        println!("    |__ Port 1: Dev 2, If 0, Class=Mass Storage, Driver=usb-storage, 5000M");
        println!("/:  Bus 01.Port 1: Dev 1, Class=root_hub, Driver=xhci_hcd/16p, 480M");
        println!("    |__ Port 2: Dev 2, If 0, Class=Human Interface Device, Driver=usbhid, 12M");
        println!("    |__ Port 2: Dev 2, If 1, Class=Human Interface Device, Driver=usbhid, 12M");
        println!("    |__ Port 5: Dev 3, If 0, Class=Wireless, Driver=btusb, 12M");
        println!("    |__ Port 5: Dev 3, If 1, Class=Wireless, Driver=btusb, 12M");
        println!("    |__ Port 8: Dev 4, If 0, Class=Video, Driver=uvcvideo, 480M");
        println!("    |__ Port 8: Dev 4, If 1, Class=Video, Driver=uvcvideo, 480M");
        println!("    |__ Port 10: Dev 5, If 0, Class=Human Interface Device, Driver=usbhid, 12M");
        return 0;
    }

    let devices = [
        ("001", "001", "1d6b", "0002", "Linux Foundation", "2.0 root hub"),
        ("002", "001", "1d6b", "0003", "Linux Foundation", "3.0 root hub"),
        ("001", "002", "046d", "c52b", "Logitech, Inc.", "Unifying Receiver"),
        ("001", "003", "8087", "0029", "Intel Corp.", "AX200 Bluetooth"),
        ("001", "004", "0c45", "6366", "Microdia", "USB 2.0 Camera"),
        ("001", "005", "1050", "0407", "Yubico.com", "Yubikey 4/5 OTP+U2F+CCID"),
        ("002", "002", "0781", "5583", "SanDisk Corp.", "Ultra Fit USB 3.1"),
    ];

    for (bus, dev, vid, pid, vendor, product) in &devices {
        println!("Bus {} Device {}: ID {}:{} {} {}", bus, dev, vid, pid, vendor, product);
        if verbose {
            println!("  bcdUSB               2.00");
            println!("  bDeviceClass         0");
            println!("  bDeviceSubClass      0");
            println!("  bDeviceProtocol      0");
            println!("  bMaxPacketSize0     64");
            println!("  idVendor           0x{}", vid);
            println!("  idProduct          0x{}", pid);
            println!("  iManufacturer        1 {}", vendor);
            println!("  iProduct             2 {}", product);
            println!();
        }
    }
    0
}

fn run_usb_devices(_args: &[String]) -> i32 {
    println!("T:  Bus=01 Lev=00 Prnt=00 Port=00 Cnt=00 Dev#=  1 Spd=480 MxCh=16");
    println!("D:  Ver= 2.00 Cls=09(hub  ) Sub=00 Prot=01 MxPS=64 #Cfgs=  1");
    println!("P:  Vendor=1d6b ProdID=0002 Rev= 6.07");
    println!("S:  Manufacturer=Linux 6.7.0 xhci-hcd");
    println!("S:  Product=xHCI Host Controller");
    println!();
    println!("T:  Bus=01 Lev=01 Prnt=01 Port=01 Cnt=01 Dev#=  2 Spd=12  MxCh= 0");
    println!("D:  Ver= 2.00 Cls=00(>ifc ) Sub=00 Prot=00 MxPS=32 #Cfgs=  1");
    println!("P:  Vendor=046d ProdID=c52b Rev=24.07");
    println!("S:  Manufacturer=Logitech");
    println!("S:  Product=USB Receiver");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "lsusb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "usb-devices" => run_usb_devices(&rest),
        _ => run_lsusb(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lsusb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lsusb"), "lsusb");
        assert_eq!(basename(r"C:\bin\lsusb.exe"), "lsusb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lsusb.exe"), "lsusb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lsusb(&["--help".to_string()]), 0);
        assert_eq!(run_lsusb(&["-h".to_string()]), 0);
        let _ = run_lsusb(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lsusb(&[]);
    }
}
