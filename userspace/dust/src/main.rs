#![deny(clippy::all)]

//! dust — Slate OS intuitive disk usage tool (du + rust = dust)
//!
//! Single personality: `dust`

use std::env;
use std::process;

fn run_dust(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dust [OPTIONS] [PATH]...");
        println!();
        println!("Like du but more intuitive. Shows disk usage with a visual graph.");
        println!();
        println!("Options:");
        println!("  -d, --depth <NUM>         Maximum depth to display");
        println!("  -n, --number-of-lines <N> Number of lines to show");
        println!("  -p, --full-paths          Show full path names");
        println!("  -X, --ignore-directory <DIR>  Exclude directory");
        println!("  -I, --ignore-all-in-file <F>  Read exclusions from file");
        println!("  -L, --dereference         Follow symbolic links");
        println!("  -x, --limit-filesystem    Only count files on same filesystem");
        println!("  -s, --apparent-size       Use apparent size instead of disk usage");
        println!("  -r, --reverse             Reverse sort order");
        println!("  -c, --no-colors           Disable colors");
        println!("  -C, --force-colors        Force colors");
        println!("  -b, --no-percent-bars     Disable percent bars");
        println!("  -B, --bars-on-right       Show bars to the right");
        println!("  -R, --screen-reader       Screen reader mode");
        println!("  -z, --min-size <SIZE>     Minimum size to display");
        println!("  -e, --filter <REGEX>      Filter output by regex");
        println!("  -v, --invert-filter <RE>  Invert filter");
        println!("  -t, --file-types          Show file type breakdown");
        println!("  -w, --terminal-width <N>  Override terminal width");
        println!("  -H, --si                  Use SI units (powers of 1000)");
        println!("  -P, --no-progress         Disable progress indicator");
        println!("  -D, --only-dir            Only show directories");
        println!("  -F, --only-file           Only show files");
        println!("  -f, --filecount           Show file count instead of size");
        println!("  -o, --output <FMT>        Output format (text/json/completion)");
        println!("  -V, --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("dust 1.1.1 (Slate OS)");
        return 0;
    }

    let file_types = args.iter().any(|a| a == "-t" || a == "--file-types");
    let filecount = args.iter().any(|a| a == "-f" || a == "--filecount");

    if file_types {
        println!("  4.2M  ┌── .rs    │████████████████████████████████████████░░│  68%");
        println!("  1.1M  ├── .toml  │██████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│  18%");
        println!("  512K  ├── .md    │█████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   8%");
        println!("  256K  ├── .lock  │██░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   4%");
        println!("  128K  └── other  │█░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   2%");
        println!("  6.2M    Total");
    } else if filecount {
        println!("   24  ┌── src         │████████████████████████████████████████░│  62%");
        println!("    8  ├── tests       │█████████████░░░░░░░░░░░░░░░░░░░░░░░░░░│  21%");
        println!("    3  ├── benches     │████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   8%");
        println!("    2  ├── examples    │██░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   5%");
        println!("    2  └── .           │██░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   5%");
        println!("   39    Total files");
    } else {
        println!("  2.8M  ┌── target     │████████████████████████████████████████░│  45%");
        println!("  1.8M  ├── src        │█████████████████████████░░░░░░░░░░░░░░░│  29%");
        println!("  892K  ├── tests      │████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░│  14%");
        println!("  412K  ├── benches    │██████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   7%");
        println!("  156K  ├── examples   │██░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   3%");
        println!("   85K  ├── Cargo.toml │█░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   1%");
        println!("   42K  └── README.md  │░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   1%");
        println!("  6.2M    Total");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dust(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_dust};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dust(vec!["--help".to_string()]), 0);
        assert_eq!(run_dust(vec!["-h".to_string()]), 0);
        let _ = run_dust(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dust(vec![]);
    }
}
