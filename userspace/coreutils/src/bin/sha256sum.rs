//! sha256sum — compute and check SHA-256 message digest.
//!
//! Usage: sha256sum [FILE...]
//!   Prints the SHA-256 hash of each file (or stdin if no files given).

use coreutils::quote::quotef_os;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.is_empty() {
        let mut data = Vec::new();
        if io::stdin().read_to_end(&mut data).is_ok() {
            let hash = sha256(&data);
            let _ = writeln!(out, "{}  -", hex(&hash));
        }
        return;
    }

    for path in &args {
        match File::open(path) {
            Ok(mut f) => {
                let mut data = Vec::new();
                if f.read_to_end(&mut data).is_ok() {
                    let hash = sha256(&data);
                    let _ = writeln!(out, "{}  {path}", hex(&hash));
                } else {
                    eprintln!("sha256sum: {}: read error", quotef_os(path));
                }
            }
            Err(e) => {
                eprintln!("sha256sum: {}: {e}", quotef_os(path));
            }
        }
    }
}

/// Format a digest as lowercase hex.
fn hex(bytes: &[u8; 32]) -> String {
    sha2::hex(bytes).as_str().to_string()
}

/// Compute the SHA-256 hash of `data`.
///
/// Delegates to the shared `sha2` crate.  This file used to carry its own
/// SHA-256 — in the one utility whose entire output is a digest that another
/// machine will compare against its own.  The FIPS vectors in the test module
/// below now check the shared implementation, which is where a checksum tool
/// wants its vectors pointed.
fn sha256(data: &[u8]) -> [u8; 32] {
    sha2::sha256(data)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn sha256_hex(data: &[u8]) -> String {
        hex(&sha256(data))
    }

    // ---------------- hex ----------------

    #[test]
    fn hex_zero_bytes() {
        let bytes = [0u8; 32];
        assert_eq!(
            hex(&bytes),
            "0000000000000000000000000000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn hex_full_range() {
        let mut bytes = [0u8; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = i as u8;
        }
        assert_eq!(
            hex(&bytes),
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
        );
    }

    // ---------------- sha256: FIPS 180-2 / standard test vectors ----------------

    #[test]
    fn sha256_empty() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_alphabet() {
        assert_eq!(
            sha256_hex(b"abcdefghijklmnopqrstuvwxyz"),
            "71c480df93d6ae2f1efad1447c66c9525e316218cf51fc8d9ed832f2daf18b73"
        );
    }

    #[test]
    fn sha256_two_block_message() {
        // 56-byte input from FIPS 180-2 Appendix B.2.
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_one_million_a_repeats_short() {
        // We don't run the full million-character test (slow); use 1000 'a's instead.
        let data = vec![b'a'; 1000];
        assert_eq!(
            sha256_hex(&data),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    #[test]
    fn sha256_exactly_55_bytes() {
        // Boundary: padding fits in one block.
        let data = vec![b'a'; 55];
        assert_eq!(
            sha256_hex(&data),
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"
        );
    }

    #[test]
    fn sha256_exactly_56_bytes() {
        // Boundary: padding requires a second block.
        let data = vec![b'a'; 56];
        assert_eq!(
            sha256_hex(&data),
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"
        );
    }

    #[test]
    fn sha256_exactly_64_bytes() {
        // One full block exactly.
        let data = vec![b'a'; 64];
        assert_eq!(
            sha256_hex(&data),
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
    }

    #[test]
    fn sha256_single_zero_byte() {
        assert_eq!(
            sha256_hex(&[0u8]),
            "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"
        );
    }

    #[test]
    fn sha256_high_bit_bytes() {
        assert_eq!(
            sha256_hex(&[0xffu8; 32]),
            "af9613760f72635fbdb44a5a0a63c39f12af30f950a6ee5c971be188e89c4051"
        );
    }

    #[test]
    fn sha256_length_matches() {
        let hash = sha256(b"anything");
        assert_eq!(hash.len(), 32);
    }
}
