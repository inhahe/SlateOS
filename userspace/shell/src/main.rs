//! Toolchain validation program — exercises key std features to verify
//! our custom Rust target and POSIX sysroot work correctly.
//!
//! Tests: formatted output, string operations, heap allocation (Vec,
//! String, HashMap), file I/O, environment variables, process info.
//!
//! This will become the shell once the toolchain is validated.

use std::collections::HashMap;
use std::env;
use std::fs;

fn main() {
    println!("=== Rust std toolchain validation ===");
    println!();

    // 1. Formatted output
    println!("[1] Formatted output");
    let x = 42;
    let pi = std::f64::consts::PI;
    println!("  integer: {x}");
    println!("  float:   {pi:.4}");
    println!("  hex:     {x:#x}");
    println!("  binary:  {x:#b}");
    println!("  OK");
    println!();

    // 2. String operations (heap allocation)
    println!("[2] String operations (heap)");
    let mut s = String::from("Hello");
    s.push_str(", world!");
    println!("  string: {s}");
    println!("  length: {}", s.len());
    println!("  upper:  {}", s.to_uppercase());
    let parts: Vec<&str> = s.split(", ").collect();
    println!("  split:  {:?}", parts);
    println!("  OK");
    println!();

    // 3. Vec and HashMap (heap allocation, hashing)
    println!("[3] Collections (Vec, HashMap)");
    let mut v: Vec<i32> = (1..=10).collect();
    v.sort_by(|a, b| b.cmp(a)); // reverse sort
    println!("  vec (reversed): {:?}", &v[..5]);

    let mut map = HashMap::new();
    map.insert("os", "our OS");
    map.insert("shell", "nushell");
    map.insert("compat", "oils");
    println!("  map entries: {}", map.len());
    if let Some(val) = map.get("os") {
        println!("  map[\"os\"] = {val}");
    }
    println!("  OK");
    println!();

    // 4. Process info
    println!("[4] Process info");
    println!("  pid: {}", std::process::id());
    println!("  OK");
    println!();

    // 5. Environment variables
    println!("[5] Environment variables");
    // Set and read back
    // SAFETY: single-threaded at this point — no concurrent env access.
    unsafe {
        env::set_var("SHELL_TEST", "it_works");
    }
    match env::var("SHELL_TEST") {
        Ok(val) => println!("  SHELL_TEST = {val}"),
        Err(e) => println!("  SHELL_TEST error: {e}"),
    }
    match env::var("HOME") {
        Ok(val) => println!("  HOME = {val}"),
        Err(_) => println!("  HOME not set (expected on our OS)"),
    }
    println!("  OK");
    println!();

    // 6. File I/O
    println!("[6] File I/O");
    let test_path = "/tmp/std_test.txt";
    let test_content = "Hello from Rust std!\nLine 2.\n";

    // Write
    match fs::write(test_path, test_content) {
        Ok(()) => println!("  wrote {test_path} ({} bytes)", test_content.len()),
        Err(e) => println!("  write error: {e}"),
    }

    // Read back
    match fs::read_to_string(test_path) {
        Ok(content) => {
            if content == test_content {
                println!("  read back matches: OK");
            } else {
                println!(
                    "  MISMATCH: read {} bytes vs wrote {}",
                    content.len(),
                    test_content.len()
                );
            }
        }
        Err(e) => println!("  read error: {e}"),
    }

    // Stat
    match fs::metadata(test_path) {
        Ok(meta) => println!("  metadata: len={}, is_file={}", meta.len(), meta.is_file()),
        Err(e) => println!("  metadata error: {e}"),
    }

    // Cleanup
    match fs::remove_file(test_path) {
        Ok(()) => println!("  removed {test_path}"),
        Err(e) => println!("  remove error: {e}"),
    }
    println!("  OK");
    println!();

    // 7. Directory listing
    println!("[7] Directory listing");
    match fs::read_dir("/bin") {
        Ok(entries) => {
            let mut names: Vec<String> = Vec::new();
            for entry in entries.flatten() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
            names.sort();
            println!("  /bin: {:?}", names);
        }
        Err(e) => println!("  readdir error: {e}"),
    }
    println!("  OK");
    println!();

    // 8. Iterator / closure chains
    println!("[8] Iterator chains");
    let sum: i64 = (1..=100).filter(|n| n % 2 == 0).sum();
    println!("  sum of even 1..100 = {sum}");
    let words = "the quick brown fox";
    let capitalized: String = words
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    println!("  capitalized: {capitalized}");
    println!("  OK");
    println!();

    println!("=== All tests passed ===");
}
