//! Minimal std test — verifies that Rust std builds and links against
//! our POSIX layer (provided as libc.a via the sysroot).
//!
//! If this compiles and links, it means:
//! - The custom target spec (x86_64-ouros) works
//! - Rust std builds with -Zbuild-std for our target
//! - The POSIX library provides all symbols std needs
//! - We can build real Rust programs with std for our OS

fn main() {
    println!("Hello from std on our OS!");
}
