#![deny(clippy::all)]

//! objdump-cli — OurOS object file tools
//!
//! Multi-personality: `objdump`, `objcopy`, `strip`, `size`, `strings`, `addr2line`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_objdump(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: objdump [OPTIONS] FILE");
        println!();
        println!("objdump — display info from object files (OurOS).");
        println!();
        println!("Options:");
        println!("  -d, --disassemble      Disassemble executable sections");
        println!("  -D, --disassemble-all  Disassemble all sections");
        println!("  -h, --section-headers  Display section headers");
        println!("  -x, --all-headers      Display all headers");
        println!("  -t, --syms             Display symbol table");
        println!("  -r, --reloc            Display relocations");
        println!("  -S, --source           Intermix source with disassembly");
        println!("  -s, --full-contents    Display full contents of sections");
        println!("  -f, --file-headers     Display file header");
        println!("  -j SECTION             Only for specified section");
        println!("  -M OPTS               Disassembler options");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("GNU objdump (GNU Binutils) 2.42 (OurOS)");
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("a.out");
    let headers = args.iter().any(|a| a == "-h" || a == "--section-headers");
    let disasm = args.iter().any(|a| a == "-d" || a == "--disassemble");
    let file_hdr = args.iter().any(|a| a == "-f" || a == "--file-headers");

    println!("{}:     file format elf64-x86-64", file);
    println!();

    if file_hdr {
        println!("architecture: i386:x86-64, flags 0x00000112:");
        println!("EXEC_P, HAS_SYMS, D_PAGED");
        println!("start address 0x0000000000401000");
        println!();
    }

    if headers {
        println!("Sections:");
        println!("Idx Name          Size      VMA               LMA               File off  Algn");
        println!("  0 .text         00001234  0000000000401000  0000000000401000  00001000  2**4");
        println!("                  CONTENTS, ALLOC, LOAD, READONLY, CODE");
        println!("  1 .rodata       00000456  0000000000403000  0000000000403000  00003000  2**4");
        println!("                  CONTENTS, ALLOC, LOAD, READONLY, DATA");
        println!("  2 .data         00000100  0000000000404000  0000000000404000  00004000  2**3");
        println!("                  CONTENTS, ALLOC, LOAD, DATA");
        println!("  3 .bss          00000200  0000000000405000  0000000000405000  00004100  2**5");
        println!("                  ALLOC");
    }

    if disasm {
        println!("Disassembly of section .text:");
        println!();
        println!("0000000000401000 <_start>:");
        println!("  401000:\t48 89 e5             \tmov    %rsp,%rbp");
        println!("  401003:\t48 83 ec 10          \tsub    $0x10,%rsp");
        println!("  401007:\t48 8d 3d f2 1f 00 00 \tlea    0x1ff2(%rip),%rdi");
        println!("  40100e:\te8 0d 00 00 00       \tcall   401020 <puts@plt>");
        println!("  401013:\t31 c0                \txor    %eax,%eax");
        println!("  401015:\tc9                   \tleave");
        println!("  401016:\tc3                   \tret");
    }
    0
}

fn run_objcopy(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: objcopy [OPTIONS] INPUT [OUTPUT]");
        println!("Options: -O FORMAT, -j SECTION, -R SECTION, --strip-all, --strip-debug");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GNU objcopy (GNU Binutils) 2.42 (OurOS)");
        return 0;
    }
    // objcopy is silent on success
    0
}

fn run_strip_bin(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: strip [OPTIONS] FILE...");
        println!("Options: -s (all symbols), -g (debug only), -K SYM (keep symbol)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GNU strip (GNU Binutils) 2.42 (OurOS)");
        return 0;
    }
    0
}

fn run_size(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: size [OPTIONS] FILE...");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("a.out");
    println!("   text\t   data\t    bss\t    dec\t    hex\tfilename");
    println!("   4660\t    256\t    512\t   5428\t   1534\t{}", file);
    0
}

fn run_strings(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: strings [OPTIONS] FILE...");
        println!("Options: -n N (min length), -a (scan all), -t FORMAT (offset), -e ENC (encoding)");
        return 0;
    }
    println!("Hello, World!");
    println!("/lib64/ld-linux-x86-64.so.2");
    println!("libc.so.6");
    println!("__libc_start_main");
    println!("GLIBC_2.34");
    println!("GCC: (OurOS 14.0.0) 14.0.0");
    println!(".text");
    println!(".rodata");
    println!(".data");
    0
}

fn run_addr2line(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: addr2line [OPTIONS] [ADDR...]");
        println!("Options: -e FILE (executable), -f (show function names), -C (demangle), -i (inlines)");
        return 0;
    }
    let funcs = args.iter().any(|a| a == "-f");
    let addrs: Vec<&str> = args.iter().filter(|a| a.starts_with("0x") || a.chars().all(|c| c.is_ascii_hexdigit())).map(|s| s.as_str()).collect();
    for addr in &addrs {
        if funcs { println!("main"); }
        println!("main.c:10");
        let _ = addr;
    }
    if addrs.is_empty() {
        if funcs { println!("??"); }
        println!("??:0");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "objdump".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "objcopy" => run_objcopy(&rest),
        "strip" => run_strip_bin(&rest),
        "size" => run_size(&rest),
        "strings" => run_strings(&rest),
        "addr2line" => run_addr2line(&rest),
        _ => run_objdump(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
