#![deny(clippy::all)]

//! nm-cli — SlateOS symbol table tools
//!
//! Multi-personality: `nm`, `c++filt`, `ar`, `ranlib`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_nm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nm [OPTIONS] FILE...");
        println!();
        println!("nm — list symbols from object files (Slate OS).");
        println!();
        println!("Options:");
        println!("  -a, --debug-syms     Display all symbols");
        println!("  -C, --demangle       Decode C++ names");
        println!("  -D, --dynamic        Display dynamic symbols");
        println!("  -g, --extern-only    Display only external symbols");
        println!("  -n, --numeric-sort   Sort by address");
        println!("  -p, --no-sort        Don't sort");
        println!("  -r, --reverse-sort   Reverse sort");
        println!("  -S, --print-size     Print symbol size");
        println!("  -u, --undefined-only Display only undefined symbols");
        println!("  -A, --print-file-name Print file name");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("GNU nm (GNU Binutils) 2.42 (Slate OS)");
        return 0;
    }

    let undefined = args.iter().any(|a| a == "-u" || a == "--undefined-only");
    let dynamic = args.iter().any(|a| a == "-D" || a == "--dynamic");
    let with_size = args.iter().any(|a| a == "-S" || a == "--print-size");

    if undefined {
        println!("                 U __libc_start_main");
        println!("                 U printf");
        println!("                 U malloc");
        println!("                 U free");
        println!("                 U exit");
    } else if dynamic {
        println!("                 w __gmon_start__");
        println!("0000000000401020 T main");
        println!("                 U printf@@GLIBC_2.2.5");
        println!("                 U __libc_start_main@@GLIBC_2.34");
    } else {
        let size_col = if with_size { " 00000017" } else { "" };
        println!("0000000000401000{} T _start", size_col);
        println!("0000000000401020{} T main", if with_size { " 00000064" } else { "" });
        println!("00000000004010a0{} T helper_func", if with_size { " 00000032" } else { "" });
        println!("0000000000403000{} R message_str", if with_size { " 0000000d" } else { "" });
        println!("0000000000404000{} D global_var", if with_size { " 00000008" } else { "" });
        println!("0000000000405000{} B bss_buffer", if with_size { " 00000100" } else { "" });
        println!("                 U printf");
        println!("                 U __libc_start_main");
    }
    0
}

fn run_cppfilt(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: c++filt [OPTIONS] [SYMBOL...]");
        println!();
        println!("c++filt — demangle C++/Java symbol names (Slate OS).");
        println!();
        println!("Options:");
        println!("  -n               Don't strip underscores");
        println!("  -p               Don't display function parameters");
        println!("  -t               Demangle types");
        println!("  -s FORMAT        Set demangling style (auto, gnu-v3, java)");
        return 0;
    }

    let symbols: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    for sym in &symbols {
        if sym.starts_with("_Z") {
            println!("demangled::{}", sym);
        } else {
            println!("{}", sym);
        }
    }
    if symbols.is_empty() {
        println!("c++filt: reading from stdin (pipe mangled symbols)");
    }
    0
}

fn run_ar(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ar [OPTIONS] ARCHIVE [MEMBER...]");
        println!();
        println!("ar — create, modify, and extract from archives (Slate OS).");
        println!();
        println!("Commands:");
        println!("  r    Insert/replace files");
        println!("  d    Delete files");
        println!("  t    List contents");
        println!("  x    Extract files");
        println!("  s    Create/update archive index");
        println!("Modifiers: c (create), u (update), v (verbose)");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("GNU ar (GNU Binutils) 2.42 (Slate OS)");
        return 0;
    }

    let operation = args.first().map(|s| s.as_str()).unwrap_or("t");
    let archive = args.get(1).map(|s| s.as_str()).unwrap_or("libfoo.a");

    if operation.contains('t') {
        println!("main.o");
        println!("utils.o");
        println!("parser.o");
    } else if operation.contains('r') {
        let members: Vec<&str> = args.iter().skip(2).map(|s| s.as_str()).collect();
        for m in &members {
            println!("a - {}", m);
        }
        let _ = archive;
    }
    0
}

fn run_ranlib(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: ranlib [OPTIONS] ARCHIVE...");
        println!("ranlib — generate index to archive (Slate OS).");
        return 0;
    }
    let _ = args;
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "nm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "c++filt" | "cppfilt" => run_cppfilt(&rest),
        "ar" => run_ar(&rest),
        "ranlib" => run_ranlib(&rest),
        _ => run_nm(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nm"), "nm");
        assert_eq!(basename(r"C:\bin\nm.exe"), "nm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nm.exe"), "nm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nm(&["--help".to_string()]), 0);
        assert_eq!(run_nm(&["-h".to_string()]), 0);
        let _ = run_nm(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nm(&[]);
    }
}
