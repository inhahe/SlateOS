#![deny(clippy::all)]

//! protobuf-cli — OurOS Protocol Buffers / serialization tools
//!
//! Multi-personality: `protoc`, `protoc-gen-go`, `buf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_protoc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: protoc [OPTIONS] PROTO_FILES");
        println!();
        println!("protoc — Protocol Buffer compiler (OurOS).");
        println!();
        println!("Options:");
        println!("  --proto_path=PATH, -I PATH   Import path");
        println!("  --cpp_out=OUT_DIR            Generate C++ code");
        println!("  --java_out=OUT_DIR           Generate Java code");
        println!("  --python_out=OUT_DIR         Generate Python code");
        println!("  --go_out=OUT_DIR             Generate Go code");
        println!("  --rust_out=OUT_DIR           Generate Rust code");
        println!("  --grpc_out=OUT_DIR           Generate gRPC stubs");
        println!("  --descriptor_set_out=FILE    Write descriptors");
        println!("  --decode=MESSAGE_TYPE        Decode binary");
        println!("  --encode=MESSAGE_TYPE        Encode to binary");
        println!("  --decode_raw                 Decode raw binary");
        println!("  --version                    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("libprotoc 25.2 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "--decode_raw") {
        println!("1: \"John Doe\"");
        println!("2: 123");
        println!("3: \"john@example.com\"");
        return 0;
    }

    // Simulate compilation
    let proto_files: Vec<&str> = args.iter()
        .filter(|a| a.ends_with(".proto"))
        .map(|s| s.as_str())
        .collect();
    if proto_files.is_empty() {
        println!("Missing input file.");
        return 1;
    }
    for f in &proto_files {
        println!("Compiling {}", f);
    }
    0
}

fn run_buf(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: buf [FLAGS] COMMAND [ARGS]");
        println!();
        println!("buf — Protobuf build tool (OurOS).");
        println!();
        println!("Commands:");
        println!("  lint          Lint proto files");
        println!("  breaking      Check breaking changes");
        println!("  build         Build proto files");
        println!("  generate      Generate code");
        println!("  push          Push to BSR");
        println!("  dep update    Update dependencies");
        println!("  mod init      Initialize module");
        println!("  format        Format proto files");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("1.29.0 (OurOS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("lint");
    match subcmd {
        "lint" => println!("No lint errors found."),
        "breaking" => println!("No breaking changes detected."),
        "build" => println!("Build completed successfully."),
        "generate" => {
            println!("Generating code...");
            println!("Generated myapp/v1/user.pb.go");
            println!("Generated myapp/v1/user_grpc.pb.go");
            println!("Generated myapp/v1/order.pb.go");
            println!("Generated myapp/v1/order_grpc.pb.go");
        }
        "format" => println!("Formatted 4 files."),
        "mod" => {
            let sub2 = args.get(1).map(|s| s.as_str()).unwrap_or("init");
            if sub2 == "init" {
                println!("Created buf.yaml and buf.gen.yaml");
            } else {
                println!("buf mod {} completed", sub2);
            }
        }
        "dep" => println!("Dependencies updated."),
        _ => println!("buf: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "protoc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "buf" => run_buf(&rest),
        "protoc-gen-go" | "protoc-gen-rust" => {
            println!("This is a protoc plugin. Run via protoc --go_out or --rust_out.");
            0
        }
        _ => run_protoc(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_protoc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/protobuf"), "protobuf");
        assert_eq!(basename(r"C:\bin\protobuf.exe"), "protobuf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("protobuf.exe"), "protobuf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_protoc(&["--help".to_string()]), 0);
        assert_eq!(run_protoc(&["-h".to_string()]), 0);
        let _ = run_protoc(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_protoc(&[]);
    }
}
