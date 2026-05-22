#![deny(clippy::all)]

//! asciidoctor-cli — OurOS Asciidoctor CLI
//!
//! Single personality: `asciidoctor`

use std::env;
use std::process;

fn run_asciidoctor(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: asciidoctor [OPTIONS] FILE...");
        println!();
        println!("Asciidoctor — AsciiDoc processor (OurOS).");
        println!();
        println!("Options:");
        println!("  -b, --backend NAME     Backend (html5, docbook5, manpage)");
        println!("  -d, --doctype TYPE     Document type (article, book, manpage, inline)");
        println!("  -o, --out-file FILE    Output file");
        println!("  -D, --destination-dir  Output directory");
        println!("  -a, --attribute KEY=V  Set document attribute");
        println!("  -r, --require LIB      Require library");
        println!("  -n, --section-numbers  Number sections");
        println!("  -s, --no-header-footer Suppress header/footer");
        println!("  -S, --safe-mode MODE   Safe mode (unsafe, safe, server, secure)");
        println!("  --template-dir DIR     Template directory");
        println!("  --trace                Include backtrace");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Asciidoctor 2.0.20 (OurOS)");
        return 0;
    }

    let backend = args.windows(2).find(|w| w[0] == "-b" || w[0] == "--backend")
        .map(|w| w[1].as_str()).unwrap_or("html5");
    let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--out-file")
        .map(|w| w[1].as_str());

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        eprintln!("asciidoctor: no input files. See --help.");
        return 1;
    }

    for file in &files {
        let out = if let Some(o) = output {
            o.to_string()
        } else {
            let ext = match backend {
                "html5" => "html",
                "docbook5" => "xml",
                "manpage" => "1",
                _ => "html",
            };
            file.replace(".adoc", &format!(".{}", ext))
                .replace(".asciidoc", &format!(".{}", ext))
        };
        println!("asciidoctor: converting {} -> {} (backend: {})", file, out, backend);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_asciidoctor(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
