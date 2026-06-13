#![deny(clippy::all)]

//! syft-cli — Slate OS Syft SBOM generator
//!
//! Multi-personality: `syft`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_syft(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: syft [SOURCE] [OPTIONS]");
        println!("Syft 1.8.0 (Slate OS) — SBOM generator");
        println!();
        println!("Sources:");
        println!("  IMAGE           Container image");
        println!("  dir:PATH        Directory");
        println!("  file:PATH       Single file");
        println!("  registry:IMAGE  Remote registry");
        println!();
        println!("Options:");
        println!("  -o, --output FMT   Output format (syft-table, syft-json, cyclonedx-json,");
        println!("                     spdx-json, spdx-tag-value, github-json)");
        println!("  --scope SCOPE      Scan scope (squashed, all-layers)");
        println!("  --platform P       Platform filter");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("syft 1.8.0");
        return 0;
    }
    let source = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("alpine:latest");
    let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--output")
        .map(|w| w[1].as_str()).unwrap_or("syft-table");

    match output {
        "syft-json" | "json" => {
            println!("{{");
            println!("  \"artifacts\": [");
            println!("    {{\"name\": \"alpine-baselayout\", \"version\": \"3.4.3-r2\", \"type\": \"apk\"}},");
            println!("    {{\"name\": \"busybox\", \"version\": \"1.36.1-r15\", \"type\": \"apk\"}},");
            println!("    {{\"name\": \"libcrypto3\", \"version\": \"3.3.0-r2\", \"type\": \"apk\"}},");
            println!("    {{\"name\": \"musl\", \"version\": \"1.2.5-r0\", \"type\": \"apk\"}}");
            println!("  ],");
            println!("  \"source\": {{\"type\": \"image\", \"target\": \"{}\"}}", source);
            println!("}}");
        }
        "cyclonedx-json" => {
            println!("{{");
            println!("  \"bomFormat\": \"CycloneDX\",");
            println!("  \"specVersion\": \"1.5\",");
            println!("  \"components\": [");
            println!("    {{\"type\": \"library\", \"name\": \"alpine-baselayout\", \"version\": \"3.4.3-r2\"}},");
            println!("    {{\"type\": \"library\", \"name\": \"busybox\", \"version\": \"1.36.1-r15\"}}");
            println!("  ]");
            println!("}}");
        }
        _ => {
            println!(" ✔ Loaded image             {}", source);
            println!(" ✔ Parsed image             sha256:abc123...");
            println!(" ✔ Cataloged packages       [42 packages]");
            println!();
            println!("NAME                  VERSION       TYPE");
            println!("alpine-baselayout     3.4.3-r2      apk");
            println!("busybox               1.36.1-r15    apk");
            println!("ca-certificates       20240226-r0   apk");
            println!("libcrypto3            3.3.0-r2      apk");
            println!("libssl3               3.3.0-r2      apk");
            println!("musl                  1.2.5-r0      apk");
            println!("musl-utils            1.2.5-r0      apk");
            println!("zlib                  1.3.1-r1      apk");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "syft".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_syft(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_syft};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/syft"), "syft");
        assert_eq!(basename(r"C:\bin\syft.exe"), "syft.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("syft.exe"), "syft");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_syft(&["--help".to_string()]), 0);
        assert_eq!(run_syft(&["-h".to_string()]), 0);
        let _ = run_syft(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_syft(&[]);
    }
}
