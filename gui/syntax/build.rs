//! Turns each grammar's generated `parser.c` into Rust (`tsgrammar`): its
//! tables as little-endian files and its lexers as functions, written into
//! `OUT_DIR/<grammar>/`, where `src/grammars/<grammar>.rs` includes them.
//!
//! The `parser.c` files are the grammars' own, as their authors publish them
//! (`grammars/<grammar>/parser.c`): updating a grammar is replacing its file.
//! See `gui/tsgrammar/src/lib.rs` for why they are converted rather than
//! compiled.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

/// Every grammar in `grammars/`, by directory.
const GRAMMARS: &[&str] = &[
    "ada",
    "bash",
    "c",
    "cpp",
    "css",
    "diff",
    "dockerfile",
    "dtd",
    "go",
    "html",
    "ini",
    "java",
    "javascript",
    "jsdoc",
    "json",
    "linkerscript",
    "lua",
    "make",
    "markdown",
    "markdown_inline",
    "powershell",
    "python",
    "regex",
    "rust",
    "toml",
    "tsx",
    "typescript",
    "xml",
    "yaml",
];

fn main() -> Result<(), Box<dyn Error>> {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?);
    for name in GRAMMARS {
        let source = format!("grammars/{name}/parser.c");
        println!("cargo::rerun-if-changed={source}");
        let text = fs::read_to_string(&source).map_err(|e| format!("{source}: {e}"))?;
        let grammar = tsgrammar::parse(&text).map_err(|e| format!("{source}: {e}"))?;
        let output = grammar.render(name);
        let dir = out.join(name);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("language.rs"), output.rust)?;
        // Deflated: the generated Rust inflates what it includes. Level 6:
        // measured on C++'s and Bash's tables, 1.4% larger than level 9 in
        // a build script's unoptimised code at two-fifths of the time.
        for (file, bytes) in output.blobs {
            fs::write(dir.join(file), deflate::deflate_level(&bytes, 6))?;
        }
    }
    println!("cargo::rerun-if-changed=build.rs");
    Ok(())
}
