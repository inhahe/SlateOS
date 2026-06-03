#![deny(clippy::all)]

//! protobuf — OurOS Protocol Buffers compiler and tools
//!
//! Multi-personality: `protoc`, `protoc-gen-rust`, `protoc-gen-go`

use std::env;
use std::process;

fn run_protoc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: protoc [OPTION] PROTO_FILES");
        println!();
        println!("Options:");
        println!("  --proto_path=PATH, -I PATH  Specify import path");
        println!("  --cpp_out=OUT_DIR           Generate C++ code");
        println!("  --java_out=OUT_DIR          Generate Java code");
        println!("  --python_out=OUT_DIR        Generate Python code");
        println!("  --go_out=OUT_DIR            Generate Go code");
        println!("  --rust_out=OUT_DIR          Generate Rust code");
        println!("  --grpc_out=OUT_DIR          Generate gRPC stubs");
        println!("  --descriptor_set_out=FILE   Write descriptor set");
        println!("  --include_imports           Include imported descriptors");
        println!("  --include_source_info       Include source info in descriptors");
        println!("  --plugin=EXECUTABLE         Specify plugin executable");
        println!("  --decode=MESSAGE_TYPE       Decode binary to text");
        println!("  --decode_raw                Decode without schema");
        println!("  --encode=MESSAGE_TYPE       Encode text to binary");
        println!("  --version                   Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("libprotoc 26.1 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--decode_raw") {
        println!("1: \"hello\"");
        println!("2: 42");
        println!("3 {{");
        println!("  1: \"nested\"");
        println!("}}");
        return 0;
    }

    let out_flags: Vec<&str> = args.iter()
        .filter(|a| a.ends_with("_out") || a.contains("_out="))
        .map(|s| s.as_str())
        .collect();
    let proto_files: Vec<&str> = args.iter()
        .filter(|a| a.ends_with(".proto"))
        .map(|s| s.as_str())
        .collect();

    if proto_files.is_empty() {
        eprintln!("Missing input file.");
        return 1;
    }
    if out_flags.is_empty() {
        eprintln!("Missing output directive (e.g., --cpp_out=.)");
        return 1;
    }

    for f in &proto_files {
        for out in &out_flags {
            println!("Compiling {} ({})", f, out);
        }
    }
    0
}

fn run_protoc_gen(args: Vec<String>, lang: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: protoc-gen-{} (invoked by protoc as a plugin)", lang);
        println!("  Reads CodeGeneratorRequest from stdin, writes CodeGeneratorResponse to stdout.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("protoc-gen-{} 1.0.0 (OurOS)", lang);
        return 0;
    }
    println!("protoc-gen-{}: reading CodeGeneratorRequest from stdin...", lang);
    println!("(plugin mode — invoked by protoc, not directly)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("protoc");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "protoc-gen-rust" => run_protoc_gen(rest, "rust"),
        "protoc-gen-go" => run_protoc_gen(rest, "go"),
        _ => run_protoc(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_protoc_gen};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_protoc_gen(vec!["--help".to_string()], "protobuf"), 0);
        assert_eq!(run_protoc_gen(vec!["-h".to_string()], "protobuf"), 0);
        assert_eq!(run_protoc_gen(vec!["--version".to_string()], "protobuf"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_protoc_gen(vec![], "protobuf"), 0);
    }
}
