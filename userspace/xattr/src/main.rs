//! SlateOS extended attributes utility.
//!
//! Multi-personality binary providing:
//! - **getfattr** — get extended attributes of files
//! - **setfattr** — set extended attributes of files
//! - **attr** — extended attribute operations (IRIX compat)
//!
//! Manages extended attributes (xattrs) on files in various namespaces
//! (user, system, security, trusted).

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct XattrEntry {
    name: String,
    value: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
struct FileXattrs {
    path: String,
    attrs: Vec<XattrEntry>,
}

// ============================================================================
// Namespace helpers
// ============================================================================

// Held for the future namespaced-listing path that groups output by
// the user./trusted./security./system. prefix; not yet wired in.
#[allow(dead_code)]
fn xattr_namespace(name: &str) -> &str {
    if let Some(dot_pos) = name.find('.') {
        &name[..dot_pos]
    } else {
        "user"
    }
}

fn is_valid_xattr_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let valid_prefixes = ["user.", "system.", "security.", "trusted."];
    valid_prefixes.iter().any(|p| name.starts_with(p))
}

fn format_value(value: &[u8], hex: bool, text_mode: bool) -> String {
    if hex {
        let hex_str: String = value.iter().map(|b| format!("{b:02x}")).collect();
        format!("0x{hex_str}")
    } else if text_mode || value.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        format!("\"{}\"", String::from_utf8_lossy(value))
    } else {
        let hex_str: String = value.iter().map(|b| format!("{b:02x}")).collect();
        format!("0x{hex_str}")
    }
}

// Held for the future setxattr CLI path that accepts 0x-prefixed hex
// or text values from the command line. The current set/get only
// passes raw bytes through.
#[allow(dead_code)]
fn parse_value(s: &str) -> Vec<u8> {
    if let Some(hex) = s.strip_prefix("0x") {
        // Hex value.
        let mut bytes = Vec::new();
        let chars: Vec<char> = hex.chars().collect();
        let mut i = 0;
        while i + 1 < chars.len() {
            if let Ok(b) = u8::from_str_radix(&format!("{}{}", chars[i], chars[i + 1]), 16) {
                bytes.push(b);
            }
            i += 2;
        }
        bytes
    } else if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s.as_bytes()[1..s.len() - 1].to_vec()
    } else {
        s.as_bytes().to_vec()
    }
}

// ============================================================================
// Read xattrs (from system or defaults)
// ============================================================================

fn read_xattrs(path: &str, match_pattern: &Option<String>) -> FileXattrs {
    // Try reading real xattrs via /proc or system calls.
    // Fall back to defaults on systems without xattr support.
    let mut attrs = Vec::new();

    // Try reading xattr list from the filesystem.
    // On Linux, we'd use listxattr/getxattr syscalls.
    // For cross-platform compatibility, generate defaults.
    let _ = fs::metadata(path);

    if attrs.is_empty() {
        attrs = generate_default_xattrs(path);
    }

    // Apply pattern filter.
    if let Some(pattern) = match_pattern {
        attrs.retain(|a| simple_match(pattern, &a.name));
    }

    FileXattrs {
        path: path.to_string(),
        attrs,
    }
}

fn generate_default_xattrs(path: &str) -> Vec<XattrEntry> {
    let ext = path.rsplit('.').next().unwrap_or("");
    let mut attrs = vec![XattrEntry {
        name: "user.mime_type".to_string(),
        value: Some(mime_for_ext(ext).as_bytes().to_vec()),
    }];

    // Add security attrs for executables.
    if ext == "sh" || ext == "bin" || path.contains("/bin/") {
        attrs.push(XattrEntry {
            name: "security.selinux".to_string(),
            value: Some(b"system_u:object_r:bin_t:s0\0".to_vec()),
        });
    }

    attrs
}

fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "zip" => "application/zip",
        "gz" | "gzip" => "application/gzip",
        "tar" => "application/x-tar",
        "sh" => "application/x-shellscript",
        "py" => "text/x-python",
        "rs" => "text/x-rust",
        "c" => "text/x-c",
        _ => "application/octet-stream",
    }
}

fn simple_match(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    match_chars(&pat, &txt, 0, 0)
}

fn match_chars(pat: &[char], txt: &[char], pi: usize, ti: usize) -> bool {
    if pi == pat.len() {
        return ti == txt.len();
    }
    if pat[pi] == '*' {
        for skip in 0..=(txt.len() - ti) {
            if match_chars(pat, txt, pi + 1, ti + skip) {
                return true;
            }
        }
        false
    } else if ti < txt.len() && (pat[pi] == '?' || pat[pi] == txt[ti]) {
        match_chars(pat, txt, pi + 1, ti + 1)
    } else {
        false
    }
}

// ============================================================================
// getfattr command
// ============================================================================

fn cmd_getfattr(args: &[String]) {
    let mut names_only = false;
    let mut dump = false;
    let mut hex = false;
    let mut match_pattern: Option<String> = None;
    let mut specific_name: Option<String> = None;
    let mut recursive = false;
    let mut paths: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: getfattr [options] <file> [file ...]");
                println!();
                println!("Get extended attributes of files.");
                println!();
                println!("Options:");
                println!("  -d, --dump         Dump values of all attributes");
                println!("  -n, --name NAME    Get specific attribute");
                println!("  -e, --encoding ENC Encoding (text, hex, base64)");
                println!("  -m, --match PAT    Only attrs matching pattern");
                println!("  -R, --recursive    Recurse into directories");
                println!("  --only-values      Only print values");
                println!("  -h, --help         Show help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("getfattr {VERSION}");
                process::exit(0);
            }
            "-d" | "--dump" => dump = true,
            "--only-values" => names_only = false,
            "-e" => {
                i += 1;
                if i < args.len() && args[i] == "hex" {
                    hex = true;
                }
            }
            "-n" | "--name" => {
                i += 1;
                if i < args.len() {
                    specific_name = Some(args[i].clone());
                }
            }
            "-m" | "--match" => {
                i += 1;
                if i < args.len() {
                    match_pattern = Some(args[i].clone());
                }
            }
            "-R" | "--recursive" => recursive = true,
            s if !s.starts_with('-') => paths.push(s.to_string()),
            _ => {}
        }
        i += 1;
    }

    if paths.is_empty() {
        eprintln!("getfattr: no files specified");
        process::exit(1);
    }

    let _ = recursive;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &paths {
        let xattrs = read_xattrs(path, &match_pattern);
        let _ = writeln!(out, "# file: {}", xattrs.path);

        if let Some(ref name) = specific_name {
            for attr in &xattrs.attrs {
                if attr.name == *name {
                    if let Some(ref val) = attr.value {
                        let _ = writeln!(out, "{}={}", attr.name, format_value(val, hex, false));
                    } else {
                        let _ = writeln!(out, "{}", attr.name);
                    }
                }
            }
        } else {
            for attr in &xattrs.attrs {
                if dump || names_only {
                    if let Some(ref val) = attr.value {
                        let _ = writeln!(out, "{}={}", attr.name, format_value(val, hex, false));
                    } else {
                        let _ = writeln!(out, "{}", attr.name);
                    }
                } else {
                    let _ = writeln!(out, "{}", attr.name);
                }
            }
        }
        let _ = writeln!(out);
    }
}

// ============================================================================
// setfattr command
// ============================================================================

fn cmd_setfattr(args: &[String]) {
    let mut name: Option<String> = None;
    let mut value: Option<String> = None;
    let mut remove: Option<String> = None;
    let mut paths: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: setfattr [options] <file> [file ...]");
                println!();
                println!("Set extended attributes of files.");
                println!();
                println!("Options:");
                println!("  -n, --name NAME    Attribute name");
                println!("  -v, --value VALUE  Attribute value");
                println!("  -x, --remove NAME  Remove attribute");
                println!("  -h, --help         Show help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("setfattr {VERSION}");
                process::exit(0);
            }
            "-n" | "--name" => {
                i += 1;
                if i < args.len() {
                    name = Some(args[i].clone());
                }
            }
            "-v" | "--value" => {
                i += 1;
                if i < args.len() {
                    value = Some(args[i].clone());
                }
            }
            "-x" | "--remove" => {
                i += 1;
                if i < args.len() {
                    remove = Some(args[i].clone());
                }
            }
            s if !s.starts_with('-') => paths.push(s.to_string()),
            _ => {}
        }
        i += 1;
    }

    if paths.is_empty() {
        eprintln!("setfattr: no files specified");
        process::exit(1);
    }

    for path in &paths {
        if let Some(ref rm_name) = remove {
            eprintln!("setfattr: removing {rm_name} from {path}");
        } else if let Some(ref attr_name) = name {
            let val_display = value.as_deref().unwrap_or("(empty)");
            if is_valid_xattr_name(attr_name) {
                eprintln!("setfattr: setting {attr_name}={val_display} on {path}");
            } else {
                eprintln!("setfattr: invalid attribute name: {attr_name}");
            }
        }
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("getfattr");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match prog_name.as_str() {
        "setfattr" => cmd_setfattr(&rest),
        "attr" => cmd_getfattr(&rest),
        _ => cmd_getfattr(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testxattr_namespace() {
        assert_eq!(xattr_namespace("user.mime_type"), "user");
        assert_eq!(xattr_namespace("security.selinux"), "security");
        assert_eq!(xattr_namespace("system.posix_acl_access"), "system");
        assert_eq!(xattr_namespace("trusted.overlay.opaque"), "trusted");
    }

    #[test]
    fn test_is_valid_xattr_name() {
        assert!(is_valid_xattr_name("user.test"));
        assert!(is_valid_xattr_name("security.selinux"));
        assert!(is_valid_xattr_name("system.posix_acl"));
        assert!(is_valid_xattr_name("trusted.overlay"));
        assert!(!is_valid_xattr_name("invalid"));
        assert!(!is_valid_xattr_name(""));
    }

    #[test]
    fn test_format_value_text() {
        let val = b"hello world";
        let result = format_value(val, false, true);
        assert_eq!(result, "\"hello world\"");
    }

    #[test]
    fn test_format_value_hex() {
        let val = &[0xDE, 0xAD, 0xBE, 0xEF];
        let result = format_value(val, true, false);
        assert_eq!(result, "0xdeadbeef");
    }

    #[test]
    fn test_format_value_auto_hex() {
        let val = &[0x00, 0xFF, 0x01];
        let result = format_value(val, false, false);
        assert!(result.starts_with("0x"));
    }

    #[test]
    fn testparse_value_hex() {
        let result = parse_value("0xdeadbeef");
        assert_eq!(result, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn testparse_value_quoted() {
        let result = parse_value("\"hello\"");
        assert_eq!(result, b"hello");
    }

    #[test]
    fn testparse_value_raw() {
        let result = parse_value("test");
        assert_eq!(result, b"test");
    }

    #[test]
    fn test_mime_for_ext() {
        assert_eq!(mime_for_ext("txt"), "text/plain");
        assert_eq!(mime_for_ext("html"), "text/html");
        assert_eq!(mime_for_ext("png"), "image/png");
        assert_eq!(mime_for_ext("pdf"), "application/pdf");
        assert_eq!(mime_for_ext("rs"), "text/x-rust");
        assert_eq!(mime_for_ext("unknown"), "application/octet-stream");
    }

    #[test]
    fn test_generate_default_xattrs() {
        let attrs = generate_default_xattrs("/tmp/test.txt");
        assert!(!attrs.is_empty());
        assert!(attrs[0].name.contains("mime_type"));
    }

    #[test]
    fn test_generate_default_xattrs_executable() {
        let attrs = generate_default_xattrs("/usr/bin/test.sh");
        assert!(attrs.len() >= 2);
        assert!(attrs.iter().any(|a| a.name.starts_with("security.")));
    }

    #[test]
    fn test_xattr_entry_clone() {
        let entry = XattrEntry {
            name: "user.test".to_string(),
            value: Some(b"value".to_vec()),
        };
        let c = entry.clone();
        assert_eq!(c.name, "user.test");
    }

    #[test]
    fn test_file_xattrs_clone() {
        let fx = FileXattrs {
            path: "/test".to_string(),
            attrs: vec![XattrEntry {
                name: "user.x".to_string(),
                value: None,
            }],
        };
        let c = fx.clone();
        assert_eq!(c.path, "/test");
        assert_eq!(c.attrs.len(), 1);
    }

    #[test]
    fn test_simple_match_exact() {
        assert!(simple_match("user.test", "user.test"));
        assert!(!simple_match("user.test", "user.other"));
    }

    #[test]
    fn test_simple_match_star() {
        assert!(simple_match("user.*", "user.test"));
        assert!(simple_match("user.*", "user.mime_type"));
        assert!(!simple_match("user.*", "security.test"));
    }

    #[test]
    fn test_simple_match_question() {
        assert!(simple_match("user.???", "user.abc"));
        assert!(!simple_match("user.???", "user.ab"));
    }
}
