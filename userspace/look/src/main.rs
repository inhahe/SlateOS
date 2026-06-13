//! look — display lines beginning with a given string for SlateOS
//!
//! Searches for lines in a sorted file that begin with the given
//! prefix string. Uses binary search for efficiency.

use std::env;
use std::fs;
use std::process;

struct Options {
    /// Alternative characters considered (default: alphanumeric only)
    alpha_only: bool,
    /// Case-insensitive comparison
    case_insensitive: bool,
    /// Custom termination character
    terminate: Option<char>,
    /// String to search for
    prefix: String,
    /// File to search in (default: /usr/share/dict/words)
    file: String,
}

fn print_help() {
    println!("Usage: look [-df] [-t CHAR] STRING [FILE]");
    println!("Display lines beginning with STRING in sorted FILE.");
    println!();
    println!("Options:");
    println!("  -d, --alphanum     only compare alphanumeric characters");
    println!("  -f, --ignore-case  fold case (case-insensitive comparison)");
    println!("  -t CHAR            specify a string termination character");
    println!("  -h, --help         display this help and exit");
    println!("  --version          output version information and exit");
    println!();
    println!("FILE defaults to /usr/share/dict/words if not specified.");
    println!("When -d is used with a dictionary file, only letters and");
    println!("digits are considered when comparing.");
}

fn parse_args(args: &[String]) -> Options {
    let mut opts = Options {
        alpha_only: false,
        case_insensitive: false,
        terminate: None,
        prefix: String::new(),
        file: "/usr/share/dict/words".to_string(),
    };

    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-d" | "--alphanum" => opts.alpha_only = true,
            "-f" | "--ignore-case" => opts.case_insensitive = true,
            "-t" => {
                i += 1;
                if i < args.len() && !args[i].is_empty() {
                    opts.terminate = args[i].chars().next();
                }
            }
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            "--version" => {
                println!("look (SlateOS coreutils) 0.1.0");
                process::exit(0);
            }
            _ if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") => {
                // Combined flags like -df
                let chars: Vec<char> = arg[1..].chars().collect();
                let mut j = 0;
                while j < chars.len() {
                    match chars[j] {
                        'd' => opts.alpha_only = true,
                        'f' => opts.case_insensitive = true,
                        't' => {
                            if j + 1 < chars.len() {
                                opts.terminate = Some(chars[j + 1]);
                                j = chars.len(); // consumed rest
                                continue;
                            } else {
                                i += 1;
                                if i < args.len() && !args[i].is_empty() {
                                    opts.terminate = args[i].chars().next();
                                }
                            }
                        }
                        _ => {
                            eprintln!("look: unknown option '-{}'", chars[j]);
                            process::exit(1);
                        }
                    }
                    j += 1;
                }
            }
            _ => {
                positional.push(arg.clone());
            }
        }
        i += 1;
    }

    match positional.len() {
        0 => {
            eprintln!("look: missing operand");
            eprintln!("Try 'look --help' for more information.");
            process::exit(1);
        }
        1 => {
            opts.prefix = positional[0].clone();
        }
        2 => {
            opts.prefix = positional[0].clone();
            opts.file = positional[1].clone();
        }
        _ => {
            eprintln!("look: extra operand '{}'", positional[2]);
            process::exit(1);
        }
    }

    opts
}

/// Normalize a string for comparison based on options
fn normalize(s: &str, alpha_only: bool, case_insensitive: bool) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        if alpha_only && !ch.is_alphanumeric() {
            continue;
        }
        if case_insensitive {
            for lower in ch.to_lowercase() {
                result.push(lower);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Extract the comparison key from a string, applying termination char
fn extract_key(s: &str, terminate: Option<char>, alpha_only: bool, case_insensitive: bool) -> String {
    let base = if let Some(term) = terminate {
        if let Some(pos) = s.find(term) {
            &s[..pos]
        } else {
            s
        }
    } else {
        s
    };
    normalize(base, alpha_only, case_insensitive)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let opts = parse_args(&args);

    let content = match fs::read_to_string(&opts.file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("look: {}: {}", opts.file, e);
            process::exit(2);
        }
    };

    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        process::exit(1);
    }

    let search_key = normalize(&opts.prefix, opts.alpha_only, opts.case_insensitive);

    // Binary search for the first line that could match
    let mut lo = 0usize;
    let mut hi = lines.len();

    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let line_key = extract_key(lines[mid], opts.terminate, opts.alpha_only, opts.case_insensitive);
        if line_key < search_key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }

    // Print all matching lines starting from lo
    let mut found = false;
    for line in &lines[lo..] {
        let line_key = extract_key(line, opts.terminate, opts.alpha_only, opts.case_insensitive);
        if line_key.starts_with(&search_key) {
            println!("{}", line);
            found = true;
        } else if line_key > search_key
            && !line_key.starts_with(&search_key)
        {
            break;
        }
    }

    process::exit(if found { 0 } else { 1 });
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_plain() {
        assert_eq!(normalize("hello", false, false), "hello");
    }

    #[test]
    fn test_normalize_case_insensitive() {
        assert_eq!(normalize("Hello", false, true), "hello");
        assert_eq!(normalize("WORLD", false, true), "world");
    }

    #[test]
    fn test_normalize_alpha_only() {
        assert_eq!(normalize("he-llo", true, false), "hello");
        assert_eq!(normalize("a.b.c", true, false), "abc");
    }

    #[test]
    fn test_normalize_both() {
        assert_eq!(normalize("He-LLo!", true, true), "hello");
    }

    #[test]
    fn test_normalize_digits() {
        assert_eq!(normalize("abc123", true, false), "abc123");
        assert_eq!(normalize("abc-123", true, false), "abc123");
    }

    #[test]
    fn test_normalize_empty() {
        assert_eq!(normalize("", false, false), "");
        assert_eq!(normalize("", true, true), "");
    }

    #[test]
    fn test_normalize_all_special() {
        assert_eq!(normalize("---", true, false), "");
    }

    #[test]
    fn test_extract_key_no_terminate() {
        assert_eq!(extract_key("hello world", None, false, false), "hello world");
    }

    #[test]
    fn test_extract_key_with_terminate() {
        assert_eq!(extract_key("hello:world", Some(':'), false, false), "hello");
    }

    #[test]
    fn test_extract_key_terminate_not_found() {
        assert_eq!(extract_key("hello", Some(':'), false, false), "hello");
    }

    #[test]
    fn test_extract_key_terminate_at_start() {
        assert_eq!(extract_key(":hello", Some(':'), false, false), "");
    }

    #[test]
    fn test_extract_key_with_options() {
        assert_eq!(
            extract_key("He-llo:world", Some(':'), true, true),
            "hello"
        );
    }

    // Argument parsing
    #[test]
    fn test_parse_basic() {
        let args = vec!["hello".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.prefix, "hello");
        assert_eq!(opts.file, "/usr/share/dict/words");
        assert!(!opts.alpha_only);
        assert!(!opts.case_insensitive);
    }

    #[test]
    fn test_parse_with_file() {
        let args = vec!["hello".to_string(), "/tmp/words".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.prefix, "hello");
        assert_eq!(opts.file, "/tmp/words");
    }

    #[test]
    fn test_parse_flags() {
        let args = vec!["-df".to_string(), "hello".to_string()];
        let opts = parse_args(&args);
        assert!(opts.alpha_only);
        assert!(opts.case_insensitive);
    }

    #[test]
    fn test_parse_terminate() {
        let args = vec!["-t".to_string(), ":".to_string(), "hello".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.terminate, Some(':'));
    }

    // Binary search correctness
    #[test]
    fn test_binary_search_finds_prefix() {
        let lines = ["apple", "banana", "cherry", "date", "elderberry"];
        let search = normalize("ch", false, false);

        let mut lo = 0usize;
        let mut hi = lines.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let key = normalize(lines[mid], false, false);
            if key < search {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        assert_eq!(lo, 2); // "cherry" is at index 2
        assert!(normalize(lines[lo], false, false).starts_with(&search));
    }

    #[test]
    fn test_binary_search_no_match() {
        let lines = ["apple", "banana", "cherry"];
        let search = normalize("dog", false, false);

        let mut lo = 0usize;
        let mut hi = lines.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let key = normalize(lines[mid], false, false);
            if key < search {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        // lo should be past all matches
        assert!(lo >= lines.len() || !normalize(lines[lo], false, false).starts_with(&search));
    }

    #[test]
    fn test_binary_search_first_element() {
        let lines = ["apple", "banana", "cherry"];
        let search = normalize("ap", false, false);

        let mut lo = 0usize;
        let mut hi = lines.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let key = normalize(lines[mid], false, false);
            if key < search {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        assert_eq!(lo, 0);
    }

    #[test]
    fn test_binary_search_last_element() {
        let lines = ["apple", "banana", "cherry"];
        let search = normalize("ch", false, false);

        let mut lo = 0usize;
        let mut hi = lines.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let key = normalize(lines[mid], false, false);
            if key < search {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        assert_eq!(lo, 2);
    }

    #[test]
    fn test_case_insensitive_match() {
        let lines = vec!["Apple", "Banana", "Cherry"];
        let search = normalize("ban", false, true);

        let mut found = false;
        for line in &lines {
            let key = normalize(line, false, true);
            if key.starts_with(&search) {
                found = true;
                break;
            }
        }
        assert!(found);
    }
}
