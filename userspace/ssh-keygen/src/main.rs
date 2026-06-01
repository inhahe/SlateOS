//! OurOS SSH Key Generation Utility
//!
//! Generates and manages SSH key pairs for use with OurOS's SSH client.
//! Supports Ed25519 key pairs (the primary and recommended key type).
//!
//! # Usage
//!
//! ```text
//! ssh-keygen                           Generate Ed25519 key with defaults
//! ssh-keygen -t ed25519                Explicitly request Ed25519
//! ssh-keygen -f ~/.ssh/mykey           Write to a custom file
//! ssh-keygen -C "my comment"           Set key comment
//! ssh-keygen -l -f ~/.ssh/id_ed25519   Show SHA-256 fingerprint
//! ssh-keygen -y -f ~/.ssh/id_ed25519   Print public key from private key
//! ssh-keygen -q -f ~/.ssh/id_ed25519   Quiet mode
//! ```
//!
//! # Key Format
//!
//! Public key:  `ssh-ed25519 <base64> <comment>` (OpenSSH format)
//! Private key: PEM-like wrapper around a base64-encoded 32-byte seed
//!
//! # Cryptography
//!
//! Ed25519 is implemented from first principles:
//! - Field arithmetic mod p = 2^255 - 19
//! - Twisted Edwards curve operations (addition, doubling, scalar multiply)
//! - SHA-512 for key derivation (RFC 8032 §5.1.5)
//! - SHA-256 for fingerprints

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).

use std::env;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};

// ============================================================================
// I/O + randomness
// ============================================================================
//
// All file and stdout/stderr I/O routes through std, which reaches the native
// OurOS syscalls via the posix libc layer.  A previous hand-rolled syscall
// stub here hardcoded Linux numbers that collide with unrelated native
// syscalls — WRITE=1=SYS_EXIT (so every write terminated the process),
// OPEN=2=SYS_TASK_ID, CLOSE=3 unassigned, STAT=4 unassigned, EXIT=60=
// SYS_SYSCTL_GET, MKDIR=83 unassigned — making the tool completely
// non-functional.  Randomness uses the posix `getrandom` C symbol because no
// std API exposes the kernel CSPRNG.

#[cfg(unix)]
unsafe extern "C" {
    /// Fill `buf` with `buflen` random bytes; returns bytes written or -1.
    fn getrandom(buf: *mut u8, buflen: usize, flags: u32) -> isize;
}

/// Fill `buf` with cryptographically random bytes from the kernel CSPRNG.
fn fill_random(buf: &mut [u8]) -> Result<(), KeygenError> {
    #[cfg(unix)]
    {
        // SAFETY: valid mutable buffer pointer and exact length; the posix
        // getrandom writes at most `buflen` bytes and returns the count or -1.
        let ret = unsafe { getrandom(buf.as_mut_ptr(), buf.len(), 0) };
        if ret < 0 || usize::try_from(ret).unwrap_or(0) != buf.len() {
            return Err(KeygenError::RandomFailed);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        // Host test toolchain has no kernel CSPRNG; key generation is not
        // exercised in host unit tests, so fail explicitly if reached.
        let _ = buf;
        Err(KeygenError::RandomFailed)
    }
}

/// Apply a Unix permission `mode` to `path` (best effort; no-op off-unix).
#[cfg(unix)]
fn set_mode(path: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &str, _mode: u32) {}

/// Write `data` to stdout.
fn write_stdout(data: &[u8]) -> Result<(), KeygenError> {
    std::io::stdout()
        .write_all(data)
        .map_err(|_| KeygenError::WriteError("stdout".to_string()))
}

/// Write `data` to stderr (best effort).
fn write_stderr(data: &[u8]) {
    let _ = std::io::stderr().write_all(data);
}

/// Create a directory with the given `mode`, ignoring "already exists".
fn mkdir(path: &str, mode: u32) {
    // Best effort: only stamp the mode when we actually created the directory.
    // Other errors (e.g. parent missing) surface when we try to create files
    // inside, mirroring the previous behaviour.
    if std::fs::create_dir(path).is_ok() {
        set_mode(path, mode);
    }
}

/// Check whether a path exists.
fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Read an entire file into a `Vec<u8>`.
fn read_file(path: &str) -> Result<Vec<u8>, KeygenError> {
    std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => KeygenError::FileNotFound(path.to_string()),
        _ => KeygenError::ReadError,
    })
}

/// Write `data` to `path` (creating/truncating) and apply `mode`.
fn write_file(path: &str, data: &[u8], mode: u32) -> Result<(), KeygenError> {
    std::fs::write(path, data).map_err(|_| KeygenError::WriteError(path.to_string()))?;
    set_mode(path, mode);
    Ok(())
}

/// Terminate the process with the given exit code.
fn exit(code: i32) -> ! {
    std::process::exit(code)
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum KeygenError {
    RandomFailed,
    ReadError,
    WriteError(String),
    FileNotFound(String),
    FileExists(String),
    InvalidBase64,
    InvalidKeyFile(String),
    UnsupportedKeyType(String),
    ParseError(String),
}

impl fmt::Display for KeygenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RandomFailed => write!(f, "failed to get random bytes from kernel"),
            Self::ReadError => write!(f, "read error"),
            Self::WriteError(p) => write!(f, "write error: {p}"),
            Self::FileNotFound(p) => write!(f, "no such file: {p}"),
            Self::FileExists(p) => write!(f, "file already exists: {p}"),
            Self::InvalidBase64 => write!(f, "invalid base64 data"),
            Self::InvalidKeyFile(m) => write!(f, "invalid key file: {m}"),
            Self::UnsupportedKeyType(t) => write!(f, "unsupported key type: {t}"),
            Self::ParseError(m) => write!(f, "parse error: {m}"),
        }
    }
}

// ============================================================================
// Base64
// ============================================================================

/// Standard (non-URL-safe) Base64 alphabet.
static B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode `data` to standard base64 with `=` padding.
fn base64_encode(data: &[u8]) -> String {
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0usize;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as u32 } else { 0 };

        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_CHARS[((n >> 18) & 0x3f) as usize]);
        out.push(B64_CHARS[((n >> 12) & 0x3f) as usize]);
        out.push(if i + 1 < data.len() { B64_CHARS[((n >> 6) & 0x3f) as usize] } else { b'=' });
        out.push(if i + 2 < data.len() { B64_CHARS[(n & 0x3f) as usize] } else { b'=' });
        i = i.saturating_add(3);
    }
    // SAFETY: `out` only contains ASCII characters from B64_CHARS and `=`.
    unsafe { String::from_utf8_unchecked(out) }
}

/// Decode standard base64 (with or without `=` padding).
///
/// Returns `Err(InvalidBase64)` on illegal characters or truncated input.
fn base64_decode(s: &[u8]) -> Result<Vec<u8>, KeygenError> {
    // Build a reverse lookup table: 255 = invalid.
    let mut table = [255u8; 256];
    for (i, &c) in B64_CHARS.iter().enumerate() {
        table[c as usize] = i as u8;
    }

    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut i = 0usize;

    // Skip trailing `=` padding.
    let end = {
        let mut e = s.len();
        while e > 0 && s[e - 1] == b'=' {
            e -= 1;
        }
        e
    };

    while i < end {
        let c0 = table.get(s[i] as usize).copied().unwrap_or(255);
        let c1 = if i + 1 < end {
            table.get(s[i + 1] as usize).copied().unwrap_or(255)
        } else {
            return Err(KeygenError::InvalidBase64);
        };
        if c0 == 255 || c1 == 255 {
            return Err(KeygenError::InvalidBase64);
        }
        out.push((c0 << 2) | (c1 >> 4));

        if i + 2 < end {
            let c2 = table.get(s[i + 2] as usize).copied().unwrap_or(255);
            if c2 == 255 {
                return Err(KeygenError::InvalidBase64);
            }
            out.push(((c1 & 0xf) << 4) | (c2 >> 2));

            if i + 3 < end {
                let c3 = table.get(s[i + 3] as usize).copied().unwrap_or(255);
                if c3 == 255 {
                    return Err(KeygenError::InvalidBase64);
                }
                out.push(((c2 & 0x3) << 6) | c3);
            }
        }
        i = i.saturating_add(4);
    }
    Ok(out)
}

// ============================================================================
// SHA-256
// ============================================================================

/// SHA-256 round constants (first 32 bits of fractional parts of cube roots of
/// the first 64 primes).
#[rustfmt::skip]
const SHA256_K: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5,
    0x3956_c25b, 0x59f1_11f1, 0x923f_82a4, 0xab1c_5ed5,
    0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3,
    0x72be_5d74, 0x80de_b1fe, 0x9bdc_06a7, 0xc19b_f174,
    0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc,
    0x2de9_2c6f, 0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da,
    0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
    0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967,
    0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc, 0x5338_0d13,
    0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85,
    0xa2bf_e8a1, 0xa81a_664b, 0xc24b_8b70, 0xc76c_51a3,
    0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070,
    0x19a4_c116, 0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5,
    0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208,
    0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7, 0xc671_78f2,
];

/// Initial hash values for SHA-256 (first 32 bits of fractional parts of the
/// square roots of the first 8 primes).
const SHA256_INIT: [u32; 8] = [
    0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a,
    0x510e_527f, 0x9b05_688c, 0x1f83_d9ab, 0x5be0_cd19,
];

/// Process one 64-byte block into the running SHA-256 state.
fn sha256_compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for i in 0..16usize {
        w[i] = (block[i * 4] as u32) << 24
            | (block[i * 4 + 1] as u32) << 16
            | (block[i * 4 + 2] as u32) << 8
            | block[i * 4 + 3] as u32;
    }
    for i in 16..64usize {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;

    for i in 0..64usize {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// Compute SHA-256 of `data`. Returns the 32-byte digest.
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = SHA256_INIT;
    let len = data.len();

    // Process full 64-byte blocks.
    let full_blocks = len / 64;
    for i in 0..full_blocks {
        let mut block = [0u8; 64];
        block.copy_from_slice(&data[i * 64..(i + 1) * 64]);
        sha256_compress(&mut state, &block);
    }

    // Pad the final block(s).
    let remainder = &data[full_blocks * 64..];
    let mut last_block = [0u8; 64];
    last_block[..remainder.len()].copy_from_slice(remainder);
    last_block[remainder.len()] = 0x80;

    if remainder.len() < 56 {
        // Length fits in this block.
        let bit_len = (len as u64).wrapping_mul(8);
        last_block[56..64].copy_from_slice(&bit_len.to_be_bytes());
        sha256_compress(&mut state, &last_block);
    } else {
        // Need a second padding block.
        sha256_compress(&mut state, &last_block);
        let mut second_block = [0u8; 64];
        let bit_len = (len as u64).wrapping_mul(8);
        second_block[56..64].copy_from_slice(&bit_len.to_be_bytes());
        sha256_compress(&mut state, &second_block);
    }

    let mut out = [0u8; 32];
    for (i, &word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ============================================================================
// SHA-512
// ============================================================================

/// SHA-512 round constants.
#[rustfmt::skip]
const SHA512_K: [u64; 80] = [
    0x428a_2f98_d728_ae22, 0x7137_4491_23ef_65cd, 0xb5c0_fbcf_ec4d_3b2f, 0xe9b5_dba5_8189_dbbc,
    0x3956_c25b_f348_b538, 0x59f1_11f1_b605_d019, 0x923f_82a4_af19_4f9b, 0xab1c_5ed5_da6d_8118,
    0xd807_aa98_a303_0242, 0x1283_5b01_4570_6fbe, 0x2431_85be_4ee4_b28c, 0x550c_7dc3_d5ff_b4e2,
    0x72be_5d74_f27b_896f, 0x80de_b1fe_3b16_96b1, 0x9bdc_06a7_25c7_1235, 0xc19b_f174_cf69_2694,
    0xe49b_69c1_9ef1_4ad2, 0xefbe_4786_384f_25e3, 0x0fc1_9dc6_8b8c_d5b5, 0x240c_a1cc_77ac_9c65,
    0x2de9_2c6f_592b_0275, 0x4a74_84aa_6ea6_e483, 0x5cb0_a9dc_bd41_fbd4, 0x76f9_88da_8311_53b5,
    0x983e_5152_ee66_dfab, 0xa831_c66d_2db4_3210, 0xb003_27c8_98fb_213f, 0xbf59_7fc7_beef_0ee4,
    0xc6e0_0bf3_3da8_8fc2, 0xd5a7_9147_930a_a725, 0x06ca_6351_e003_826f, 0x1429_2967_0a0e_6e70,
    0x27b7_0a85_46d2_2ffc, 0x2e1b_2138_5c26_c926, 0x4d2c_6dfc_5ac4_2aed, 0x5338_0d13_9d95_b3df,
    0x650a_7354_8baf_63de, 0x766a_0abb_3c77_b2a8, 0x81c2_c92e_47ed_aee6, 0x9272_2c85_1482_353b,
    0xa2bf_e8a1_4cf1_0364, 0xa81a_664b_bc42_3001, 0xc24b_8b70_d0f8_9791, 0xc76c_51a3_0654_be30,
    0xd192_e819_d6ef_5218, 0xd699_0624_5565_a910, 0xf40e_3585_5771_202a, 0x106a_a070_32bb_d1b8,
    0x19a4_c116_b8d2_d0c8, 0x1e37_6c08_5141_ab53, 0x2748_774c_df8e_eb99, 0x34b0_bcb5_e19b_48a8,
    0x391c_0cb3_c5c9_5a63, 0x4ed8_aa4a_e341_8acb, 0x5b9c_ca4f_7763_e373, 0x682e_6ff3_d6b2_b8a3,
    0x748f_82ee_5def_b2fc, 0x78a5_636f_4317_2f60, 0x84c8_7814_a1f0_ab72, 0x8cc7_0208_1a64_39ec,
    0x90be_fffa_2363_1e28, 0xa450_6ceb_de82_bde9, 0xbef9_a3f7_b2c6_7915, 0xc671_78f2_e372_532b,
    0xca27_3ece_ea26_619c, 0xd186_b8c7_21c0_c207, 0xeada_7dd6_cde0_eb1e, 0xf57d_4f7f_ee6e_d178,
    0x06f0_67aa_7217_6fba, 0x0a63_7dc5_a2c8_98a6, 0x113f_9804_bef9_0dae, 0x1b71_0b35_131c_471b,
    0x28db_77f5_2304_7d84, 0x32ca_ab7b_40c7_2493, 0x3c9e_be0a_15c9_bebc, 0x431d_67c4_9c10_0d4c,
    0x4cc5_d4be_cb3e_42b6, 0x597f_299c_fc65_7e2a, 0x5fcb_6fab_3ad6_faec, 0x6c44_198c_4a47_5817,
];

/// SHA-512 initial hash values.
const SHA512_INIT: [u64; 8] = [
    0x6a09_e667_f3bc_c908, 0xbb67_ae85_84ca_a73b, 0x3c6e_f372_fe94_f82b, 0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1, 0x9b05_688c_2b3e_6c1f, 0x1f83_d9ab_fb41_bd6b, 0x5be0_cd19_137e_2179,
];

/// Process one 128-byte block into the running SHA-512 state.
fn sha512_compress(state: &mut [u64; 8], block: &[u8; 128]) {
    let mut w = [0u64; 80];
    for i in 0..16usize {
        w[i] = (block[i * 8] as u64) << 56
            | (block[i * 8 + 1] as u64) << 48
            | (block[i * 8 + 2] as u64) << 40
            | (block[i * 8 + 3] as u64) << 32
            | (block[i * 8 + 4] as u64) << 24
            | (block[i * 8 + 5] as u64) << 16
            | (block[i * 8 + 6] as u64) << 8
            | block[i * 8 + 7] as u64;
    }
    for i in 16..80usize {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;

    for i in 0..80usize {
        let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA512_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// Compute SHA-512 of `data`. Returns the 64-byte digest.
fn sha512(data: &[u8]) -> [u8; 64] {
    let mut state = SHA512_INIT;
    let len = data.len();

    let full_blocks = len / 128;
    for i in 0..full_blocks {
        let mut block = [0u8; 128];
        block.copy_from_slice(&data[i * 128..(i + 1) * 128]);
        sha512_compress(&mut state, &block);
    }

    let remainder = &data[full_blocks * 128..];
    let mut last_block = [0u8; 128];
    last_block[..remainder.len()].copy_from_slice(remainder);
    last_block[remainder.len()] = 0x80;

    if remainder.len() < 112 {
        let bit_len = (len as u128).wrapping_mul(8);
        last_block[112..128].copy_from_slice(&bit_len.to_be_bytes());
        sha512_compress(&mut state, &last_block);
    } else {
        sha512_compress(&mut state, &last_block);
        let mut second_block = [0u8; 128];
        let bit_len = (len as u128).wrapping_mul(8);
        second_block[112..128].copy_from_slice(&bit_len.to_be_bytes());
        sha512_compress(&mut state, &second_block);
    }

    let mut out = [0u8; 64];
    for (i, &word) in state.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ============================================================================
// Ed25519 — Field arithmetic over GF(2^255 - 19)
// ============================================================================
//
// Representation: a field element is a 256-bit integer reduced mod p where
// p = 2^255 - 19. We use a 5-limb representation with 51-bit limbs (a la
// the RFC 8032 reference implementation and the "curve25519-dalek" approach),
// but stored as u64 for simplicity. Each limb holds < 2^52 after reduction.
//
// This is NOT a constant-time implementation. For a key generation tool (not
// a signing oracle used in a server loop) this is acceptable; the only secret
// information is the seed which is known only at keygen time and is not reused
// across calls in an observable way.
//
// Reference: RFC 8032 §5.1; https://hyperelliptic.org/EFD/g1p/auto-twisted-edwards-extended.html

/// A field element mod p = 2^255 - 19, stored as 5 little-endian 51-bit limbs.
#[derive(Clone, Copy, Debug)]
struct Fe([u64; 5]);

const FE_ZERO: Fe = Fe([0, 0, 0, 0, 0]);
const FE_ONE: Fe = Fe([1, 0, 0, 0, 0]);

impl Fe {
    /// Create a field element from a 32-byte little-endian encoding.
    fn from_bytes(bytes: &[u8; 32]) -> Self {
        // Load into 5 × 51-bit limbs from the little-endian byte representation.
        // Mask the top bit (bit 255) as required by RFC 8032.
        let mut b = *bytes;
        b[31] &= 0x7f;

        let load = |i: usize| -> u64 {
            let mut v = 0u64;
            for k in 0..8usize {
                let byte_idx = i + k;
                if byte_idx < 32 {
                    v |= (b[byte_idx] as u64) << (k * 8);
                }
            }
            v
        };

        Fe([
            load(0) & 0x0007_ffff_ffff_ffff,
            (load(6) >> 3) & 0x0007_ffff_ffff_ffff,
            (load(12) >> 6) & 0x0007_ffff_ffff_ffff,
            (load(19) >> 1) & 0x0007_ffff_ffff_ffff,
            (load(24) >> 12) & 0x0007_ffff_ffff_ffff,
        ])
    }

    /// Encode a (reduced) field element to 32 bytes, little-endian.
    fn to_bytes(self) -> [u8; 32] {
        let r = self.reduce();
        let [h0, h1, h2, h3, h4] = r.0;
        // Pack 5 × 51-bit limbs back into 255 bits.
        let b0 = h0 | (h1 << 51);
        let b1 = (h1 >> 13) | (h2 << 38);
        let b2 = (h2 >> 26) | (h3 << 25);
        let b3 = (h3 >> 39) | (h4 << 12);

        let mut out = [0u8; 32];
        out[0..8].copy_from_slice(&b0.to_le_bytes());
        out[8..16].copy_from_slice(&b1.to_le_bytes());
        out[16..24].copy_from_slice(&b2.to_le_bytes());
        out[24..32].copy_from_slice(&b3.to_le_bytes());
        out
    }

    /// Fully reduce the element to its canonical representative in `[0, p)`,
    /// using the identity 2^255 ≡ 19 (mod p).
    fn reduce(self) -> Self {
        let mask = 0x0007_ffff_ffff_ffff_u64;
        let [mut h0, mut h1, mut h2, mut h3, mut h4] = self.0;

        // Two full carry passes (each folding the 2^255 overflow back as ×19)
        // bring any field-operation output — whose limbs are < 2^53 in the
        // worst case (sub: 2p plus a reduced limb) — down to a value `v` that
        // is congruent mod p and lies in [0, 2^255 + 19) ⊂ [0, 2p). After this
        // every limb is < 2^51 except h0, which may be up to 2^51 + 18 from the
        // final fold; the carry chains below tolerate that.
        for _ in 0..2 {
            let c = h0 >> 51; h0 &= mask; h1 = h1.wrapping_add(c);
            let c = h1 >> 51; h1 &= mask; h2 = h2.wrapping_add(c);
            let c = h2 >> 51; h2 &= mask; h3 = h3.wrapping_add(c);
            let c = h3 >> 51; h3 &= mask; h4 = h4.wrapping_add(c);
            let c = h4 >> 51; h4 &= mask; h0 = h0.wrapping_add(c.wrapping_mul(19));
        }

        // q = 1 iff v ≥ p, computed as the carry out of bit 255 of (v + 19).
        // Since p = 2^255 - 19, (v + 19) ≥ 2^255 exactly when v ≥ p, and
        // v < 2p guarantees q ∈ {0, 1}. The limb additions only track the
        // running carry, so individual limbs above 2^51 are fine.
        let mut q = h0.wrapping_add(19) >> 51;
        q = h1.wrapping_add(q) >> 51;
        q = h2.wrapping_add(q) >> 51;
        q = h3.wrapping_add(q) >> 51;
        q = h4.wrapping_add(q) >> 51;

        // Conditionally subtract p: v - q·p = v + 19·q - q·2^255. Add 19·q,
        // carry-propagate (without folding), then mask off the 2^255 bit, which
        // performs the - q·2^255. The result is the canonical value in [0, p).
        h0 = h0.wrapping_add(q.wrapping_mul(19));
        let c = h0 >> 51; h0 &= mask; h1 = h1.wrapping_add(c);
        let c = h1 >> 51; h1 &= mask; h2 = h2.wrapping_add(c);
        let c = h2 >> 51; h2 &= mask; h3 = h3.wrapping_add(c);
        let c = h3 >> 51; h3 &= mask; h4 = h4.wrapping_add(c);
        h4 &= mask;

        Fe([h0, h1, h2, h3, h4])
    }

    fn add(self, rhs: Self) -> Self {
        Fe([
            self.0[0].wrapping_add(rhs.0[0]),
            self.0[1].wrapping_add(rhs.0[1]),
            self.0[2].wrapping_add(rhs.0[2]),
            self.0[3].wrapping_add(rhs.0[3]),
            self.0[4].wrapping_add(rhs.0[4]),
        ])
    }

    fn sub(self, rhs: Self) -> Self {
        // Add 2p to ensure no underflow before subtracting.
        // 2p in limb form: [2*(2^51-19), 2*(2^51-1), …].
        const TWICE_P: [u64; 5] = [
            2 * (0x0007_ffff_ffff_ffff - 18),
            2 * 0x0007_ffff_ffff_ffff,
            2 * 0x0007_ffff_ffff_ffff,
            2 * 0x0007_ffff_ffff_ffff,
            2 * 0x0007_ffff_ffff_ffff,
        ];
        Fe([
            TWICE_P[0].wrapping_add(self.0[0]).wrapping_sub(rhs.0[0]),
            TWICE_P[1].wrapping_add(self.0[1]).wrapping_sub(rhs.0[1]),
            TWICE_P[2].wrapping_add(self.0[2]).wrapping_sub(rhs.0[2]),
            TWICE_P[3].wrapping_add(self.0[3]).wrapping_sub(rhs.0[3]),
            TWICE_P[4].wrapping_add(self.0[4]).wrapping_sub(rhs.0[4]),
        ])
    }

    /// Field negation: `-self ≡ 0 - self (mod p)`.
    ///
    /// Only exercised by unit tests today; gated so it does not trip
    /// `dead_code` in the production (non-test) build.
    #[cfg(test)]
    fn neg(self) -> Self {
        FE_ZERO.sub(self)
    }

    fn mul(self, rhs: Self) -> Self {
        // Schoolbook multiplication with 128-bit intermediate products, then reduce.
        // Each limb is < 2^52, so products are < 2^104 — fits in u128.
        let [a0, a1, a2, a3, a4] = self.0;
        let [b0, b1, b2, b3, b4] = rhs.0;

        // Precompute 19 * bN for the fold-back terms.
        let b1_19 = b1.wrapping_mul(19);
        let b2_19 = b2.wrapping_mul(19);
        let b3_19 = b3.wrapping_mul(19);
        let b4_19 = b4.wrapping_mul(19);

        let a0 = a0 as u128;
        let a1 = a1 as u128;
        let a2 = a2 as u128;
        let a3 = a3 as u128;
        let a4 = a4 as u128;
        let b0 = b0 as u128;
        let b1 = b1 as u128;
        let b2 = b2 as u128;
        let b3 = b3 as u128;
        let b4 = b4 as u128;
        let b1_19 = b1_19 as u128;
        let b2_19 = b2_19 as u128;
        let b3_19 = b3_19 as u128;
        let b4_19 = b4_19 as u128;

        let mut t0 = a0 * b0 + a1 * b4_19 + a2 * b3_19 + a3 * b2_19 + a4 * b1_19;
        let mut t1 = a0 * b1 + a1 * b0   + a2 * b4_19 + a3 * b3_19 + a4 * b2_19;
        let mut t2 = a0 * b2 + a1 * b1   + a2 * b0   + a3 * b4_19 + a4 * b3_19;
        let mut t3 = a0 * b3 + a1 * b2   + a2 * b1   + a3 * b0   + a4 * b4_19;
        let mut t4 = a0 * b4 + a1 * b3   + a2 * b2   + a3 * b1   + a4 * b0;

        let mask = 0x0007_ffff_ffff_ffffu128;
        let c0 = t0 >> 51; t0 &= mask;
        t1 += c0;
        let c1 = t1 >> 51; t1 &= mask;
        t2 += c1;
        let c2 = t2 >> 51; t2 &= mask;
        t3 += c2;
        let c3 = t3 >> 51; t3 &= mask;
        t4 += c3;
        let c4 = t4 >> 51; t4 &= mask;
        t0 += c4 * 19;
        let c0 = t0 >> 51; t0 &= mask;
        t1 += c0;

        Fe([t0 as u64, t1 as u64, t2 as u64, t3 as u64, t4 as u64])
    }

    fn sq(self) -> Self {
        self.mul(self)
    }

    /// Compute the modular inverse of self (self must be non-zero).
    /// Uses Fermat's little theorem: self^(p-2).
    fn invert(self) -> Self {
        // p - 2 = 2^255 - 21. Binary square-and-multiply.
        let mut result = FE_ONE;
        let mut base = self;
        // Exponent 2^255 - 21 in bits (255 bits, from LSB to MSB).
        // 2^255-21 = 0x7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffeb
        // We iterate over all 255 bits.
        const P_MINUS2: [u64; 4] = [
            0xffff_ffff_ffff_ffeb,
            0xffff_ffff_ffff_ffff,
            0xffff_ffff_ffff_ffff,
            0x7fff_ffff_ffff_ffff,
        ];
        for limb in P_MINUS2 {
            for bit in 0..64u32 {
                if (limb >> bit) & 1 == 1 {
                    result = result.mul(base);
                }
                base = base.sq();
            }
        }
        result
    }

}

// ============================================================================
// Ed25519 — Extended twisted Edwards point arithmetic
// ============================================================================
//
// Curve: −x² + y² = 1 + d·x²·y²  where d = −121665/121666 mod p
// Extended coordinates: (X:Y:Z:T) with x = X/Z, y = Y/Z, T = X·Y/Z²
// Base point is the canonical Ed25519 generator G.
//
// Formulae from: https://hyperelliptic.org/EFD/g1p/auto-twisted-edwards-extended.html

/// A point on the Ed25519 twisted Edwards curve in extended homogeneous coordinates.
#[derive(Clone, Copy, Debug)]
struct EdPoint {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

/// The Ed25519 curve constant d = -121665/121666 mod p.
/// Value from RFC 8032 appendix.
fn ed25519_d() -> Fe {
    Fe::from_bytes(&[
        0xa3, 0x78, 0x59, 0x26, 0xd2, 0xa0, 0x45, 0x0a,
        0x21, 0x6f, 0x1b, 0x3f, 0x2e, 0xdb, 0xb7, 0xd4,
        0x5f, 0x0e, 0xa1, 0xa9, 0x65, 0xa2, 0x09, 0xad,
        0x0d, 0xb9, 0x75, 0x52, 0xa1, 0x4a, 0x37, 0x52,
    ])
}

impl EdPoint {
    /// The neutral element (identity) of the group: (0, 1, 1, 0).
    fn identity() -> Self {
        EdPoint { x: FE_ZERO, y: FE_ONE, z: FE_ONE, t: FE_ZERO }
    }

    /// The standard Ed25519 base point G.
    fn base_point() -> Self {
        // Canonical coordinates from RFC 8032 §5.1.
        let gy = Fe::from_bytes(&[
            0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
            0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
            0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
            0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
        ]);
        let gx = Fe::from_bytes(&[
            0x1a, 0xd5, 0x25, 0x8f, 0x60, 0x2d, 0x56, 0xc9,
            0xb2, 0xa7, 0x25, 0x95, 0x60, 0xc7, 0x2c, 0x69,
            0x5c, 0xdc, 0xd6, 0xfd, 0x31, 0xe2, 0xa4, 0xc0,
            0xfe, 0x53, 0x6e, 0xcd, 0xd3, 0x36, 0x69, 0x21,
        ]);
        let gt = gx.mul(gy);
        EdPoint { x: gx, y: gy, z: FE_ONE, t: gt }
    }

    /// Unified twisted Edwards point addition.
    ///
    /// Formula: add-2008-hwcd from EFD.
    fn add(self, rhs: Self) -> Self {
        let d2 = ed25519_d().add(ed25519_d());

        let a = self.x.sub(self.y).mul(rhs.x.sub(rhs.y));
        let b = self.x.add(self.y).mul(rhs.x.add(rhs.y));
        let c = self.t.mul(rhs.t).mul(d2);
        let dd = self.z.mul(rhs.z);
        let e = b.sub(a);
        let f = dd.sub(c);
        let g2 = dd.add(c);
        let h = b.add(a);
        let x3 = e.mul(f);
        let y3 = g2.mul(h);
        let t3 = e.mul(h);
        let z3 = f.mul(g2);
        EdPoint { x: x3, y: y3, z: z3, t: t3 }
    }

    /// Point doubling using the dedicated formula dbl-2008-hwcd.
    fn double(self) -> Self {
        let a = self.x.sq();
        let b = self.y.sq();
        let c = self.z.sq().add(self.z.sq());
        let h = a.add(b);
        let e = h.sub(self.x.add(self.y).sq());
        let g2 = a.sub(b);
        let f = c.add(g2);
        let x3 = e.mul(f);
        let y3 = g2.mul(h);
        let t3 = e.mul(h);
        let z3 = f.mul(g2);
        EdPoint { x: x3, y: y3, z: z3, t: t3 }
    }

    /// Scalar multiplication: compute `scalar * self` using a simple double-and-add.
    ///
    /// `scalar` is a 32-byte little-endian integer.
    fn scalar_mul(self, scalar: &[u8; 32]) -> Self {
        let mut result = EdPoint::identity();
        let mut base = self;
        for byte in scalar {
            for bit in 0..8u8 {
                if (byte >> bit) & 1 == 1 {
                    result = result.add(base);
                }
                base = base.double();
            }
        }
        result
    }

    /// Encode the point to 32 bytes (compressed y coordinate, RFC 8032 §5.1.2).
    fn encode(self) -> [u8; 32] {
        let zi = self.z.invert();
        let x = self.x.mul(zi);
        let y = self.y.mul(zi);
        let mut out = y.reduce().to_bytes();
        // Set the sign bit (bit 255) to the low bit of x.
        let x_bytes = x.reduce().to_bytes();
        out[31] |= (x_bytes[0] & 1) << 7;
        out
    }
}

// ============================================================================
// Ed25519 — Key derivation (RFC 8032 §5.1.5)
// ============================================================================

/// An Ed25519 key pair: private seed + expanded scalar + public point.
struct Ed25519KeyPair {
    /// The 32-byte random seed (this is what we persist as the private key).
    seed: [u8; 32],
    /// The 32-byte compressed public key point.
    public: [u8; 32],
}

impl Ed25519KeyPair {
    /// Derive a key pair from a 32-byte random seed per RFC 8032 §5.1.5.
    fn from_seed(seed: [u8; 32]) -> Self {
        // Step 1: Hash the seed with SHA-512.
        let h = sha512(&seed);

        // Step 2: Clamp the first 32 bytes to produce the scalar.
        let mut scalar = [0u8; 32];
        scalar.copy_from_slice(&h[..32]);
        scalar[0] &= 0xf8;   // clear the lowest 3 bits
        scalar[31] &= 0x7f;  // clear the highest bit
        scalar[31] |= 0x40;  // set the second-highest bit

        // Step 3: Compute the public key as scalar * G.
        let public_point = EdPoint::base_point().scalar_mul(&scalar);
        let public = public_point.encode();

        Ed25519KeyPair { seed, public }
    }
}

// ============================================================================
// OpenSSH key format
// ============================================================================

/// The OpenSSH key type identifier for Ed25519.
const KEY_TYPE_ED25519: &str = "ssh-ed25519";

/// Encode a `u32` as a 4-byte big-endian OpenSSH string length prefix.
fn ssh_u32(n: u32) -> [u8; 4] {
    n.to_be_bytes()
}

/// Encode an SSH string (4-byte length prefix + bytes).
fn ssh_string(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + data.len());
    out.extend_from_slice(&ssh_u32(data.len() as u32));
    out.extend_from_slice(data);
    out
}

/// Build an OpenSSH public key wire encoding for Ed25519.
///
/// Format: `string("ssh-ed25519") || string(public_key_bytes)` — base64-encoded.
fn encode_public_key(public: &[u8; 32]) -> Vec<u8> {
    let mut wire = Vec::new();
    wire.extend_from_slice(&ssh_string(KEY_TYPE_ED25519.as_bytes()));
    wire.extend_from_slice(&ssh_string(public));
    wire
}

/// Format the complete public key line: `ssh-ed25519 <base64> <comment>`.
fn public_key_line(public: &[u8; 32], comment: &str) -> String {
    let wire = encode_public_key(public);
    let b64 = base64_encode(&wire);
    format!("{KEY_TYPE_ED25519} {b64} {comment}")
}

/// Private key PEM-like wrapper around a base64-encoded seed.
const PRIVKEY_HEADER: &str = "-----BEGIN ED25519 PRIVATE KEY-----";
const PRIVKEY_FOOTER: &str = "-----END ED25519 PRIVATE KEY-----";

/// Encode the private key as a PEM-like file (base64 seed + public key).
///
/// We store: seed (32 bytes) || public (32 bytes), base64-encoded.
fn encode_private_key(seed: &[u8; 32], public: &[u8; 32], comment: &str) -> String {
    let mut payload = Vec::with_capacity(96);
    payload.extend_from_slice(seed);
    payload.extend_from_slice(public);
    payload.extend_from_slice(comment.as_bytes());

    let b64 = base64_encode(&payload);

    // Wrap at 70 chars per line (PEM convention is 64 or 76; 70 is fine).
    let mut wrapped = String::new();
    let mut pos = 0usize;
    while pos < b64.len() {
        let end = (pos + 70).min(b64.len());
        wrapped.push_str(&b64[pos..end]);
        wrapped.push('\n');
        pos = end;
    }

    format!("{PRIVKEY_HEADER}\n{wrapped}{PRIVKEY_FOOTER}\n")
}

/// Parse a private key file and return `(seed, public, comment)`.
fn decode_private_key(data: &str) -> Result<([u8; 32], [u8; 32], String), KeygenError> {
    let lines: Vec<&str> = data.lines().collect();

    let start = lines
        .iter()
        .position(|l| *l == PRIVKEY_HEADER)
        .ok_or_else(|| KeygenError::InvalidKeyFile("missing header".to_string()))?;
    let end = lines
        .iter()
        .position(|l| *l == PRIVKEY_FOOTER)
        .ok_or_else(|| KeygenError::InvalidKeyFile("missing footer".to_string()))?;

    if end <= start {
        return Err(KeygenError::InvalidKeyFile("malformed PEM".to_string()));
    }

    let b64: String = lines[start + 1..end].join("");
    let payload = base64_decode(b64.as_bytes())?;

    if payload.len() < 64 {
        return Err(KeygenError::InvalidKeyFile("payload too short".to_string()));
    }

    let mut seed = [0u8; 32];
    let mut public = [0u8; 32];
    seed.copy_from_slice(&payload[..32]);
    public.copy_from_slice(&payload[32..64]);
    let comment = String::from_utf8_lossy(&payload[64..]).into_owned();

    Ok((seed, public, comment))
}

/// Parse a public key line and return `(wire_bytes, comment)`.
///
/// Expected format: `ssh-ed25519 <base64> [comment]`
fn parse_public_key_line(line: &str) -> Result<([u8; 32], String), KeygenError> {
    let mut parts = line.splitn(3, ' ');
    let keytype = parts
        .next()
        .ok_or_else(|| KeygenError::ParseError("empty line".to_string()))?;
    let b64 = parts
        .next()
        .ok_or_else(|| KeygenError::ParseError("missing base64".to_string()))?;
    let comment = parts.next().unwrap_or("").to_string();

    if keytype != KEY_TYPE_ED25519 {
        return Err(KeygenError::UnsupportedKeyType(keytype.to_string()));
    }

    let wire = base64_decode(b64.as_bytes())?;

    // Parse wire format: string("ssh-ed25519") || string(public_key_32_bytes).
    if wire.len() < 4 {
        return Err(KeygenError::InvalidKeyFile("wire too short".to_string()));
    }
    let type_len = u32::from_be_bytes([wire[0], wire[1], wire[2], wire[3]]) as usize;
    if wire.len() < 4 + type_len + 4 {
        return Err(KeygenError::InvalidKeyFile("wire truncated".to_string()));
    }
    let key_len_offset = 4 + type_len;
    let key_len = u32::from_be_bytes([
        wire[key_len_offset],
        wire[key_len_offset + 1],
        wire[key_len_offset + 2],
        wire[key_len_offset + 3],
    ]) as usize;
    let key_start = key_len_offset + 4;
    if wire.len() < key_start + key_len || key_len != 32 {
        return Err(KeygenError::InvalidKeyFile("bad public key length".to_string()));
    }
    let mut public = [0u8; 32];
    public.copy_from_slice(&wire[key_start..key_start + 32]);
    Ok((public, comment))
}

// ============================================================================
// Fingerprint
// ============================================================================

/// Compute and format the SHA-256 fingerprint of a public key wire encoding.
///
/// Format: `SHA256:<base64_no_padding>` (OpenSSH convention).
fn fingerprint(public: &[u8; 32]) -> String {
    let wire = encode_public_key(public);
    let digest = sha256(&wire);
    // OpenSSH omits trailing `=` padding from fingerprints.
    let b64 = base64_encode(&digest);
    let b64_no_pad = b64.trim_end_matches('=');
    format!("SHA256:{b64_no_pad}")
}

// ============================================================================
// Argument parsing
// ============================================================================

#[derive(Debug, Default)]
struct Args {
    /// Key type (only "ed25519" supported).
    key_type: Option<String>,
    /// Output file path.
    output_file: Option<String>,
    /// Key comment.
    comment: Option<String>,
    /// Show fingerprint mode.
    show_fingerprint: bool,
    /// Print public key from private key.
    print_public: bool,
    /// Quiet mode.
    quiet: bool,
}

fn parse_args(args: &[String]) -> Result<Args, KeygenError> {
    let mut out = Args::default();
    let mut i = 1usize; // skip argv[0]
    while i < args.len() {
        match args[i].as_str() {
            "-t" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| {
                    KeygenError::ParseError("-t requires an argument".to_string())
                })?;
                out.key_type = Some(val.clone());
            }
            "-f" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| {
                    KeygenError::ParseError("-f requires an argument".to_string())
                })?;
                out.output_file = Some(val.clone());
            }
            "-C" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| {
                    KeygenError::ParseError("-C requires an argument".to_string())
                })?;
                out.comment = Some(val.clone());
            }
            "-l" => out.show_fingerprint = true,
            "-y" => out.print_public = true,
            "-q" => out.quiet = true,
            other => {
                return Err(KeygenError::ParseError(format!("unknown option: {other}")));
            }
        }
        i += 1;
    }
    Ok(out)
}

// ============================================================================
// Key file paths
// ============================================================================

/// Resolve the default private key path: `~/.ssh/id_ed25519`.
fn default_key_path() -> String {
    match env::var("HOME") {
        Ok(home) => format!("{home}/.ssh/id_ed25519"),
        Err(_) => "id_ed25519".to_string(),
    }
}

/// Derive the public key path from the private key path (append `.pub`).
fn public_key_path(private_path: &str) -> String {
    format!("{private_path}.pub")
}

// ============================================================================
// Top-level operations
// ============================================================================

/// Generate a new Ed25519 key pair and write it to disk.
fn generate_key(args: &Args) -> Result<(), KeygenError> {
    // Validate key type if specified.
    if let Some(t) = &args.key_type
        && t != "ed25519"
    {
        return Err(KeygenError::UnsupportedKeyType(t.clone()));
    }

    let priv_path = args
        .output_file
        .clone()
        .unwrap_or_else(default_key_path);
    let pub_path = public_key_path(&priv_path);

    let comment = args.comment.clone().unwrap_or_else(|| {
        // Default comment: user@hostname (simplified — just use the path).
        format!("generated-key-{priv_path}")
    });

    // Generate 32 random bytes as the seed.
    let mut seed = [0u8; 32];
    fill_random(&mut seed)?;

    let kp = Ed25519KeyPair::from_seed(seed);

    // Ensure the parent directory exists.
    if let Some(parent) = PathBuf::from(&priv_path).parent()
        && let Some(p) = parent.to_str()
        && !p.is_empty()
    {
        mkdir(p, 0o700);
    }

    // Refuse to overwrite an existing private key.
    if path_exists(&priv_path) {
        return Err(KeygenError::FileExists(priv_path));
    }

    // Write the private key (mode 0600 — owner read/write only).
    let priv_content = encode_private_key(&kp.seed, &kp.public, &comment);
    write_file(&priv_path, priv_content.as_bytes(), 0o600)?;

    // Write the public key (mode 0644).
    let pub_line = public_key_line(&kp.public, &comment);
    let mut pub_content = pub_line.clone();
    pub_content.push('\n');
    write_file(&pub_path, pub_content.as_bytes(), 0o644)?;

    if !args.quiet {
        let msg = format!("Your identification has been saved in {priv_path}\n");
        write_stdout(msg.as_bytes())?;
        let msg = format!("Your public key has been saved in {pub_path}\n");
        write_stdout(msg.as_bytes())?;
        let fp = fingerprint(&kp.public);
        let msg = format!("The key fingerprint is:\n{fp} {comment}\n");
        write_stdout(msg.as_bytes())?;
    }

    Ok(())
}

/// Show the fingerprint of a key file.
fn show_fingerprint(args: &Args) -> Result<(), KeygenError> {
    let path = args.output_file.clone().unwrap_or_else(default_key_path);

    // Try reading as a public key first, then as a private key.
    let public = if path.ends_with(".pub") {
        let data = read_file(&path)?;
        let line = String::from_utf8_lossy(&data);
        let (pub_key, _) = parse_public_key_line(line.trim())?;
        pub_key
    } else {
        let data = read_file(&path)?;
        let s = String::from_utf8_lossy(&data);
        if s.contains(PRIVKEY_HEADER) {
            let (_, pub_key, _) = decode_private_key(&s)?;
            pub_key
        } else {
            let (pub_key, _) = parse_public_key_line(s.trim())?;
            pub_key
        }
    };

    let fp = fingerprint(&public);
    let comment = if path.ends_with(".pub") {
        let data = read_file(&path)?;
        let line = String::from_utf8_lossy(&data);
        let (_, c) = parse_public_key_line(line.trim())?;
        c
    } else {
        path.clone()
    };
    let msg = format!("256 {fp} {comment} (ED25519)\n");
    write_stdout(msg.as_bytes())?;
    Ok(())
}

/// Read a private key file and print the corresponding public key.
fn print_public_key(args: &Args) -> Result<(), KeygenError> {
    let path = args.output_file.clone().unwrap_or_else(default_key_path);
    let data = read_file(&path)?;
    let s = String::from_utf8_lossy(&data);
    let (_, public, comment) = decode_private_key(&s)?;
    let line = public_key_line(&public, &comment);
    let mut out = line;
    out.push('\n');
    write_stdout(out.as_bytes())?;
    Ok(())
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), KeygenError> {
    let argv: Vec<String> = env::args().collect();
    let args = parse_args(&argv)?;

    if args.show_fingerprint {
        show_fingerprint(&args)
    } else if args.print_public {
        print_public_key(&args)
    } else {
        generate_key(&args)
    }
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(e) => {
            let msg = format!("ssh-keygen: {e}\n");
            write_stderr(msg.as_bytes());
            exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Base64 tests ---

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn test_base64_encode_one_byte() {
        // 0b00000000 → "AA==" when one byte
        assert_eq!(base64_encode(b"\x00"), "AA==");
    }

    #[test]
    fn test_base64_encode_two_bytes() {
        assert_eq!(base64_encode(b"\x00\x00"), "AAA=");
    }

    #[test]
    fn test_base64_encode_three_bytes() {
        // Three full bytes — no padding.
        assert_eq!(base64_encode(b"\x00\x00\x00"), "AAAA");
    }

    #[test]
    fn test_base64_encode_hello() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn test_base64_encode_man() {
        // RFC 4648 test vector.
        assert_eq!(base64_encode(b"Man"), "TWFu");
    }

    #[test]
    fn test_base64_decode_empty() {
        assert_eq!(base64_decode(b"").unwrap(), b"");
    }

    #[test]
    fn test_base64_decode_hello() {
        assert_eq!(base64_decode(b"aGVsbG8=").unwrap(), b"hello");
    }

    #[test]
    fn test_base64_decode_man() {
        assert_eq!(base64_decode(b"TWFu").unwrap(), b"Man");
    }

    #[test]
    fn test_base64_roundtrip_arbitrary() {
        let data: Vec<u8> = (0u8..=255u8).collect();
        let encoded = base64_encode(&data);
        let decoded = base64_decode(encoded.as_bytes()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_base64_decode_invalid() {
        assert!(base64_decode(b"!!!!").is_err());
    }

    // --- SHA-256 tests ---

    #[test]
    fn test_sha256_empty() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb924...
        let digest = sha256(b"");
        assert_eq!(
            digest,
            [
                0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
                0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
                0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
                0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
            ]
        );
    }

    #[test]
    fn test_sha256_abc() {
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        // (canonical FIPS 180-4 example).
        let digest = sha256(b"abc");
        assert_eq!(
            digest,
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea,
                0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
                0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c,
                0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }

    #[test]
    fn test_sha256_448_bit_message() {
        // "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        let digest = sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        assert_eq!(
            digest,
            [
                0x24, 0x8d, 0x6a, 0x61, 0xd2, 0x06, 0x38, 0xb8,
                0xe5, 0xc0, 0x26, 0x93, 0x0c, 0x3e, 0x60, 0x39,
                0xa3, 0x3c, 0xe4, 0x59, 0x64, 0xff, 0x21, 0x67,
                0xf6, 0xec, 0xed, 0xd4, 0x19, 0xdb, 0x06, 0xc1,
            ]
        );
    }

    // --- SHA-512 tests ---

    #[test]
    fn test_sha512_empty() {
        // SHA-512("") known value.
        let digest = sha512(b"");
        assert_eq!(
            &digest[..8],
            &[0xcf, 0x83, 0xe1, 0x35, 0x7e, 0xef, 0xb8, 0xbd]
        );
    }

    #[test]
    fn test_sha512_abc() {
        let digest = sha512(b"abc");
        assert_eq!(
            &digest[..8],
            &[0xdd, 0xaf, 0x35, 0xa1, 0x93, 0x61, 0x7a, 0xba]
        );
    }

    // --- Field element tests ---

    #[test]
    fn test_fe_zero_plus_one() {
        let z = FE_ZERO;
        let o = FE_ONE;
        let r = z.add(o).reduce().to_bytes();
        assert_eq!(r[0], 1);
        assert_eq!(&r[1..], &[0u8; 31]);
    }

    #[test]
    fn test_fe_mul_by_zero() {
        let r = FE_ONE.mul(FE_ZERO).reduce().to_bytes();
        assert_eq!(r, [0u8; 32]);
    }

    #[test]
    fn test_fe_mul_by_one() {
        let val = Fe([7, 0, 0, 0, 0]);
        let r = val.mul(FE_ONE).reduce().to_bytes();
        let expected = val.reduce().to_bytes();
        assert_eq!(r, expected);
    }

    #[test]
    fn test_fe_sub_self() {
        let val = Fe([12345, 678, 0, 0, 0]);
        let r = val.sub(val).reduce().to_bytes();
        assert_eq!(r, [0u8; 32]);
    }

    #[test]
    fn test_fe_neg_double_negation() {
        let val = Fe([1, 2, 3, 4, 5]);
        let r = val.neg().neg().reduce().to_bytes();
        let expected = val.reduce().to_bytes();
        assert_eq!(r, expected);
    }

    // --- Ed25519 key derivation ---

    #[test]
    fn test_ed25519_base_point_not_identity() {
        let g = EdPoint::base_point();
        let identity = EdPoint::identity().encode();
        assert_ne!(g.encode(), identity);
    }

    #[test]
    fn test_ed25519_scalar_zero_is_identity() {
        // Multiplying by scalar 0 should give the identity point.
        let zero_scalar = [0u8; 32];
        let result = EdPoint::base_point().scalar_mul(&zero_scalar);
        // The identity has y = 1, x = 0 → encoding is 01 00 00 ... 00.
        let enc = result.encode();
        assert_eq!(enc[0], 1);
        assert_eq!(&enc[1..], &[0u8; 31]);
    }

    #[test]
    fn test_ed25519_keygen_from_zero_seed() {
        // Derive keys from an all-zeros seed — should not panic.
        let seed = [0u8; 32];
        let kp = Ed25519KeyPair::from_seed(seed);
        // Public key must be 32 bytes (trivially true by type) and non-zero.
        assert_ne!(kp.public, [0u8; 32]);
    }

    #[test]
    fn test_ed25519_keygen_from_ones_seed() {
        let seed = [0xffu8; 32];
        let kp = Ed25519KeyPair::from_seed(seed);
        assert_ne!(kp.public, [0u8; 32]);
    }

    #[test]
    fn test_ed25519_deterministic() {
        // Same seed → same public key.
        let seed = [42u8; 32];
        let kp1 = Ed25519KeyPair::from_seed(seed);
        let kp2 = Ed25519KeyPair::from_seed(seed);
        assert_eq!(kp1.public, kp2.public);
    }

    #[test]
    fn test_ed25519_different_seeds_different_keys() {
        let seed1 = [1u8; 32];
        let seed2 = [2u8; 32];
        let kp1 = Ed25519KeyPair::from_seed(seed1);
        let kp2 = Ed25519KeyPair::from_seed(seed2);
        assert_ne!(kp1.public, kp2.public);
    }

    // --- Public key format ---

    #[test]
    fn test_public_key_line_prefix() {
        let public = [0u8; 32];
        let line = public_key_line(&public, "test@host");
        assert!(line.starts_with("ssh-ed25519 "));
        assert!(line.ends_with("test@host"));
    }

    #[test]
    fn test_parse_public_key_roundtrip() {
        let seed = [77u8; 32];
        let kp = Ed25519KeyPair::from_seed(seed);
        let line = public_key_line(&kp.public, "user@example.com");
        let (parsed_pub, comment) = parse_public_key_line(&line).unwrap();
        assert_eq!(parsed_pub, kp.public);
        assert_eq!(comment, "user@example.com");
    }

    #[test]
    fn test_parse_public_key_wrong_type() {
        let result = parse_public_key_line("ssh-rsa AAAA== comment");
        assert!(result.is_err());
    }

    // --- Private key format ---

    #[test]
    fn test_private_key_roundtrip() {
        let seed = [0xabu8; 32];
        let kp = Ed25519KeyPair::from_seed(seed);
        let pem = encode_private_key(&kp.seed, &kp.public, "my-comment");
        let (dec_seed, dec_pub, dec_comment) = decode_private_key(&pem).unwrap();
        assert_eq!(dec_seed, kp.seed);
        assert_eq!(dec_pub, kp.public);
        assert_eq!(dec_comment, "my-comment");
    }

    #[test]
    fn test_private_key_has_header() {
        let seed = [0u8; 32];
        let kp = Ed25519KeyPair::from_seed(seed);
        let pem = encode_private_key(&kp.seed, &kp.public, "c");
        assert!(pem.contains(PRIVKEY_HEADER));
        assert!(pem.contains(PRIVKEY_FOOTER));
    }

    #[test]
    fn test_private_key_decode_missing_header() {
        let result = decode_private_key("not a key");
        assert!(result.is_err());
    }

    // --- Fingerprint ---

    #[test]
    fn test_fingerprint_prefix() {
        let public = [0u8; 32];
        let fp = fingerprint(&public);
        assert!(fp.starts_with("SHA256:"));
    }

    #[test]
    fn test_fingerprint_deterministic() {
        let public = [42u8; 32];
        let fp1 = fingerprint(&public);
        let fp2 = fingerprint(&public);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_fingerprint_different_keys_different_fp() {
        let pub1 = [1u8; 32];
        let pub2 = [2u8; 32];
        assert_ne!(fingerprint(&pub1), fingerprint(&pub2));
    }

    // --- Argument parsing ---

    #[test]
    fn test_parse_args_defaults() {
        let args: Vec<String> = vec!["ssh-keygen".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.key_type.is_none());
        assert!(parsed.output_file.is_none());
        assert!(!parsed.show_fingerprint);
        assert!(!parsed.print_public);
        assert!(!parsed.quiet);
    }

    #[test]
    fn test_parse_args_all_flags() {
        let args: Vec<String> = [
            "ssh-keygen", "-t", "ed25519", "-f", "/tmp/key",
            "-C", "my comment", "-l", "-q",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.key_type.as_deref(), Some("ed25519"));
        assert_eq!(parsed.output_file.as_deref(), Some("/tmp/key"));
        assert_eq!(parsed.comment.as_deref(), Some("my comment"));
        assert!(parsed.show_fingerprint);
        assert!(parsed.quiet);
    }

    #[test]
    fn test_parse_args_unknown_flag() {
        let args: Vec<String> = vec!["ssh-keygen".to_string(), "-z".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_missing_t_value() {
        let args: Vec<String> = vec!["ssh-keygen".to_string(), "-t".to_string()];
        assert!(parse_args(&args).is_err());
    }
}
