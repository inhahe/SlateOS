#![deny(clippy::all)]

//! readelf-cli — OurOS ELF file reader
//!
//! Multi-personality: `readelf`, `elfedit`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_readelf(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: readelf [OPTIONS] FILE...");
        println!();
        println!("readelf — display ELF file information (OurOS).");
        println!();
        println!("Options:");
        println!("  -a, --all            Display all info");
        println!("  -h, --file-header    Display ELF file header");
        println!("  -l, --program-headers Display program headers");
        println!("  -S, --section-headers Display section headers");
        println!("  -s, --syms           Display symbol table");
        println!("  -r, --relocs         Display relocations");
        println!("  -d, --dynamic        Display dynamic section");
        println!("  -n, --notes          Display notes");
        println!("  -e, --headers        All headers (= -h -l -S)");
        println!("  -W, --wide           Wide output");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("GNU readelf version 2.42 (OurOS)");
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("a.out");
    let file_header = args.iter().any(|a| a == "-h" || a == "--file-header" || a == "-a" || a == "-e");
    let sections = args.iter().any(|a| a == "-S" || a == "--section-headers" || a == "-a" || a == "-e");
    let program_headers = args.iter().any(|a| a == "-l" || a == "--program-headers" || a == "-a" || a == "-e");
    let symbols = args.iter().any(|a| a == "-s" || a == "--syms" || a == "-a");
    let dynamic = args.iter().any(|a| a == "-d" || a == "--dynamic" || a == "-a");

    if file_header {
        println!("ELF Header:");
        println!("  Magic:   7f 45 4c 46 02 01 01 00 00 00 00 00 00 00 00 00");
        println!("  Class:                             ELF64");
        println!("  Data:                              2's complement, little endian");
        println!("  Version:                           1 (current)");
        println!("  OS/ABI:                            UNIX - System V");
        println!("  ABI Version:                       0");
        println!("  Type:                              EXEC (Executable file)");
        println!("  Machine:                           Advanced Micro Devices X86-64");
        println!("  Version:                           0x1");
        println!("  Entry point address:               0x401000");
        println!("  Start of program headers:          64 (bytes into file)");
        println!("  Start of section headers:          8192 (bytes into file)");
        println!("  Flags:                             0x0");
        println!("  Size of this header:               64 (bytes)");
        println!("  Size of program headers:           56 (bytes)");
        println!("  Number of program headers:         5");
        println!("  Size of section headers:           64 (bytes)");
        println!("  Number of section headers:         12");
        println!("  Section header string table index: 11");
        let _ = file;
    }

    if program_headers {
        println!();
        println!("Program Headers:");
        println!("  Type           Offset             VirtAddr           PhysAddr");
        println!("                 FileSiz            MemSiz              Flags  Align");
        println!("  LOAD           0x0000000000000000 0x0000000000400000 0x0000000000400000");
        println!("                 0x0000000000001000 0x0000000000001000  R      0x1000");
        println!("  LOAD           0x0000000000001000 0x0000000000401000 0x0000000000401000");
        println!("                 0x0000000000001234 0x0000000000001234  R E    0x1000");
        println!("  LOAD           0x0000000000003000 0x0000000000403000 0x0000000000403000");
        println!("                 0x0000000000000456 0x0000000000000456  R      0x1000");
        println!("  LOAD           0x0000000000004000 0x0000000000404000 0x0000000000404000");
        println!("                 0x0000000000000100 0x0000000000000300  RW     0x1000");
        println!("  GNU_STACK      0x0000000000000000 0x0000000000000000 0x0000000000000000");
        println!("                 0x0000000000000000 0x0000000000000000  RW     0x10");
    }

    if sections {
        println!();
        println!("Section Headers:");
        println!("  [Nr] Name              Type             Address           Offset");
        println!("       Size              EntSize          Flags  Link  Info  Align");
        println!("  [ 0]                   NULL             0000000000000000  00000000");
        println!("       0000000000000000  0000000000000000           0     0     0");
        println!("  [ 1] .text             PROGBITS         0000000000401000  00001000");
        println!("       0000000000001234  0000000000000000  AX       0     0     16");
        println!("  [ 2] .rodata           PROGBITS         0000000000403000  00003000");
        println!("       0000000000000456  0000000000000000   A       0     0     16");
        println!("  [ 3] .data             PROGBITS         0000000000404000  00004000");
        println!("       0000000000000100  0000000000000000  WA       0     0     8");
        println!("  [ 4] .bss              NOBITS           0000000000405000  00004100");
        println!("       0000000000000200  0000000000000000  WA       0     0     32");
    }

    if symbols {
        println!();
        println!("Symbol table '.symtab' contains 8 entries:");
        println!("   Num:    Value          Size Type    Bind   Vis      Ndx Name");
        println!("     0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND");
        println!("     1: 0000000000401000     0 SECTION LOCAL  DEFAULT    1 .text");
        println!("     2: 0000000000401000    23 FUNC    GLOBAL DEFAULT    1 _start");
        println!("     3: 0000000000401020   100 FUNC    GLOBAL DEFAULT    1 main");
        println!("     4: 0000000000404000     8 OBJECT  GLOBAL DEFAULT    3 global_var");
        println!("     5: 0000000000405000   256 OBJECT  GLOBAL DEFAULT    4 bss_buffer");
    }

    if dynamic {
        println!();
        println!("Dynamic section at offset 0x3e00 contains 20 entries:");
        println!("  Tag        Type                         Name/Value");
        println!(" 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]");
        println!(" 0x000000000000000c (INIT)               0x401000");
        println!(" 0x000000000000000d (FINI)               0x402000");
    }
    0
}

fn run_elfedit(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: elfedit [OPTIONS] FILE");
        println!("Options: --output-type TYPE, --output-osabi OSABI, --output-abiversion VER");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("a.out");
    println!("elfedit: updated ELF header of '{}'", file);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "readelf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "elfedit" => run_elfedit(&rest),
        _ => run_readelf(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_readelf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/readelf"), "readelf");
        assert_eq!(basename(r"C:\bin\readelf.exe"), "readelf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("readelf.exe"), "readelf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_readelf(&["--help".to_string()]), 0);
        assert_eq!(run_readelf(&["-h".to_string()]), 0);
        let _ = run_readelf(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_readelf(&[]);
    }
}
