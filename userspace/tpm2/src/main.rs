#![deny(clippy::all)]
//! Multi-personality TPM 2.0 management tool suite for OurOS.
//!
//! Detects the active personality from `argv[0]` basename (stripping path
//! separators and `.exe` suffix):
//!
//! - `tpm2`               -- main dispatcher (runs subcommands like `tpm2 getrandom`)
//! - `tpm2_getrandom`     -- get random bytes from TPM
//! - `tpm2_pcrread`       -- read PCR registers
//! - `tpm2_pcrextend`     -- extend a PCR register
//! - `tpm2_createprimary` -- create a primary key
//! - `tpm2_create`        -- create a child key
//! - `tpm2_load`          -- load key into TPM
//! - `tpm2_sign`          -- sign data with TPM key
//! - `tpm2_verifysignature` -- verify a signature
//! - `tpm2_nvdefine`      -- define NV index
//! - `tpm2_nvwrite`       -- write to NV index
//! - `tpm2_nvread`        -- read from NV index
//! - `tpm2_getcap`        -- query TPM capabilities
//! - `tpm2_selftest`      -- run TPM self-test
//! - `tpm2_clear`         -- clear TPM
//!
//! All data is simulated (no real TPM hardware access). This provides a
//! faithful CLI interface for development, testing, and scripting on OurOS.

use std::env;
use std::process;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const VERSION: &str = "0.1.0";

/// SHA-1 digest length in bytes.
const SHA1_DIGEST_LEN: usize = 20;
/// SHA-256 digest length in bytes.
const SHA256_DIGEST_LEN: usize = 32;
/// SHA-384 digest length in bytes.
const SHA384_DIGEST_LEN: usize = 48;
/// SHA-512 digest length in bytes.
const SHA512_DIGEST_LEN: usize = 64;

/// Number of PCR registers per bank.
const PCR_COUNT: usize = 24;

/// Maximum NV index data size in bytes.
const NV_MAX_SIZE: usize = 2048;

/// Maximum number of NV indices.
const NV_MAX_INDICES: usize = 64;

/// Maximum number of loaded key objects.
const KEY_MAX_OBJECTS: usize = 32;

/// TPM manufacturer string (simulated).
const TPM_MANUFACTURER: &str = "OUROS-SIM";

/// TPM firmware version (simulated).
const TPM_FIRMWARE_VERSION: &str = "1.0.0";

/// Simulated TPM spec family.
const TPM_SPEC_FAMILY: &str = "2.0";

/// Simulated TPM spec level.
const TPM_SPEC_LEVEL: u32 = 0;

/// Simulated TPM spec revision.
const TPM_SPEC_REVISION: u32 = 164;

// ---------------------------------------------------------------------------
// Hash algorithm enum
// ---------------------------------------------------------------------------

/// Supported hash algorithms for PCR banks and key operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HashAlgorithm {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    /// Algorithm identifier string used in CLI output and parsing.
    fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
            Self::Sha384 => "sha384",
            Self::Sha512 => "sha512",
        }
    }

    /// TPM2 algorithm identifier number (TCG spec).
    fn alg_id(self) -> u16 {
        match self {
            Self::Sha1 => 0x0004,
            Self::Sha256 => 0x000B,
            Self::Sha384 => 0x000C,
            Self::Sha512 => 0x000D,
        }
    }

    /// Digest length in bytes for this algorithm.
    fn digest_len(self) -> usize {
        match self {
            Self::Sha1 => SHA1_DIGEST_LEN,
            Self::Sha256 => SHA256_DIGEST_LEN,
            Self::Sha384 => SHA384_DIGEST_LEN,
            Self::Sha512 => SHA512_DIGEST_LEN,
        }
    }

    /// Parse from a string name (case-insensitive).
    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "sha1" => Some(Self::Sha1),
            "sha256" => Some(Self::Sha256),
            "sha384" => Some(Self::Sha384),
            "sha512" => Some(Self::Sha512),
            _ => None,
        }
    }

    /// All supported algorithms.
    fn all() -> &'static [HashAlgorithm] {
        &[Self::Sha1, Self::Sha256, Self::Sha384, Self::Sha512]
    }
}

// ---------------------------------------------------------------------------
// Simulated hash functions
// ---------------------------------------------------------------------------

/// Simulated SHA-1 hash. Produces deterministic output from input bytes.
/// Not cryptographically secure -- for CLI simulation only.
fn sim_sha1(data: &[u8]) -> [u8; SHA1_DIGEST_LEN] {
    let mut state: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    for (i, &b) in data.iter().enumerate() {
        let idx = i % 5;
        state[idx] = state[idx].wrapping_add(u32::from(b).wrapping_mul(31));
        state[idx] = state[idx].rotate_left(7);
        let next = (idx + 1) % 5;
        state[next] ^= state[idx];
    }
    // Mix length
    let len = data.len() as u32;
    for s in &mut state {
        *s = s.wrapping_add(len);
        *s = s.rotate_left(13);
    }
    let mut out = [0u8; SHA1_DIGEST_LEN];
    for (i, s) in state.iter().enumerate() {
        let bytes = s.to_be_bytes();
        out[i * 4] = bytes[0];
        out[i * 4 + 1] = bytes[1];
        out[i * 4 + 2] = bytes[2];
        out[i * 4 + 3] = bytes[3];
    }
    out
}

/// Simulated SHA-256 hash.
fn sim_sha256(data: &[u8]) -> [u8; SHA256_DIGEST_LEN] {
    let mut state: [u32; 8] = [
        0x6A09_E667, 0xBB67_AE85, 0x3C6E_F372, 0xA54F_F53A,
        0x510E_527F, 0x9B05_688C, 0x1F83_D9AB, 0x5BE0_CD19,
    ];
    for (i, &b) in data.iter().enumerate() {
        let idx = i % 8;
        state[idx] = state[idx].wrapping_add(u32::from(b).wrapping_mul(37));
        state[idx] = state[idx].rotate_left(11);
        let next = (idx + 1) % 8;
        state[next] ^= state[idx];
    }
    let len = data.len() as u32;
    for s in &mut state {
        *s = s.wrapping_add(len);
        *s = s.rotate_left(17);
    }
    let mut out = [0u8; SHA256_DIGEST_LEN];
    for (i, s) in state.iter().enumerate() {
        let bytes = s.to_be_bytes();
        out[i * 4] = bytes[0];
        out[i * 4 + 1] = bytes[1];
        out[i * 4 + 2] = bytes[2];
        out[i * 4 + 3] = bytes[3];
    }
    out
}

/// Simulated SHA-384 hash.
fn sim_sha384(data: &[u8]) -> [u8; SHA384_DIGEST_LEN] {
    let mut state: [u64; 6] = [
        0xCBBB_9D5D_C105_9ED8, 0x629A_292A_367C_D507,
        0x9159_015A_3070_DD17, 0x152F_ECD8_F70E_5939,
        0x6733_2667_FFC0_0B31, 0x8EB4_4A87_6858_1511,
    ];
    for (i, &b) in data.iter().enumerate() {
        let idx = i % 6;
        state[idx] = state[idx].wrapping_add(u64::from(b).wrapping_mul(41));
        state[idx] = state[idx].rotate_left(13);
        let next = (idx + 1) % 6;
        state[next] ^= state[idx];
    }
    let len = data.len() as u64;
    for s in &mut state {
        *s = s.wrapping_add(len);
        *s = s.rotate_left(19);
    }
    let mut out = [0u8; SHA384_DIGEST_LEN];
    for (i, s) in state.iter().enumerate() {
        let bytes = s.to_be_bytes();
        for (j, &byte) in bytes.iter().enumerate() {
            out[i * 8 + j] = byte;
        }
    }
    out
}

/// Simulated SHA-512 hash.
fn sim_sha512(data: &[u8]) -> [u8; SHA512_DIGEST_LEN] {
    let mut state: [u64; 8] = [
        0x6A09_E667_F3BC_C908, 0xBB67_AE85_84CA_A73B,
        0x3C6E_F372_FE94_F82B, 0xA54F_F53A_5F1D_36F1,
        0x510E_527F_ADE6_82D1, 0x9B05_688C_2B3E_6C1F,
        0x1F83_D9AB_FB41_BD6B, 0x5BE0_CD19_137E_2179,
    ];
    for (i, &b) in data.iter().enumerate() {
        let idx = i % 8;
        state[idx] = state[idx].wrapping_add(u64::from(b).wrapping_mul(43));
        state[idx] = state[idx].rotate_left(17);
        let next = (idx + 1) % 8;
        state[next] ^= state[idx];
    }
    let len = data.len() as u64;
    for s in &mut state {
        *s = s.wrapping_add(len);
        *s = s.rotate_left(23);
    }
    let mut out = [0u8; SHA512_DIGEST_LEN];
    for (i, s) in state.iter().enumerate() {
        let bytes = s.to_be_bytes();
        for (j, &byte) in bytes.iter().enumerate() {
            out[i * 8 + j] = byte;
        }
    }
    out
}

/// Compute a simulated hash using the specified algorithm.
fn sim_hash(alg: HashAlgorithm, data: &[u8]) -> Vec<u8> {
    match alg {
        HashAlgorithm::Sha1 => sim_sha1(data).to_vec(),
        HashAlgorithm::Sha256 => sim_sha256(data).to_vec(),
        HashAlgorithm::Sha384 => sim_sha384(data).to_vec(),
        HashAlgorithm::Sha512 => sim_sha512(data).to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Hex encoding / decoding
// ---------------------------------------------------------------------------

/// Encode bytes to lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX_CHARS[(b >> 4) as usize]);
        s.push(HEX_CHARS[(b & 0x0F) as usize]);
    }
    s
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7',
    '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// Decode a hex string to bytes. Returns None if the string is invalid.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let hi = hex_val(chars[i])?;
        let lo = hex_val(chars[i + 1])?;
        bytes.push((hi << 4) | lo);
        i += 2;
    }
    Some(bytes)
}

/// Convert a single hex char to its numeric value.
fn hex_val(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Simulated PRNG for TPM random number generation
// ---------------------------------------------------------------------------

/// Simple xorshift64 PRNG for simulated random bytes.
struct SimPrng {
    state: u64,
}

impl SimPrng {
    fn new(seed: u64) -> Self {
        let state = if seed == 0 { 0xDEAD_BEEF_CAFE_BABE } else { seed };
        Self { state }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn next_u8(&mut self) -> u8 {
        (self.next_u64() & 0xFF) as u8
    }

    fn fill_bytes(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            *b = self.next_u8();
        }
    }
}

// ---------------------------------------------------------------------------
// TPM Key types
// ---------------------------------------------------------------------------

/// Key type for TPM key objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyType {
    Rsa2048,
    Rsa3072,
    Ecc256,
    Ecc384,
}

impl KeyType {
    fn name(self) -> &'static str {
        match self {
            Self::Rsa2048 => "rsa2048",
            Self::Rsa3072 => "rsa3072",
            Self::Ecc256 => "ecc256",
            Self::Ecc384 => "ecc384",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "rsa" | "rsa2048" => Some(Self::Rsa2048),
            "rsa3072" => Some(Self::Rsa3072),
            "ecc" | "ecc256" => Some(Self::Ecc256),
            "ecc384" => Some(Self::Ecc384),
            _ => None,
        }
    }

    /// Size of the simulated public key material in bytes.
    fn pub_key_size(self) -> usize {
        match self {
            Self::Rsa2048 => 256,
            Self::Rsa3072 => 384,
            Self::Ecc256 => 64,
            Self::Ecc384 => 96,
        }
    }

    /// Size of the simulated private key material in bytes.
    fn priv_key_size(self) -> usize {
        match self {
            Self::Rsa2048 => 128,
            Self::Rsa3072 => 192,
            Self::Ecc256 => 32,
            Self::Ecc384 => 48,
        }
    }

    /// Size of the simulated signature in bytes.
    fn sig_size(self) -> usize {
        match self {
            Self::Rsa2048 => 256,
            Self::Rsa3072 => 384,
            Self::Ecc256 => 64,
            Self::Ecc384 => 96,
        }
    }
}

/// TPM hierarchy for key creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hierarchy {
    Owner,
    Endorsement,
    Platform,
    Null,
}

impl Hierarchy {
    fn name(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Endorsement => "endorsement",
            Self::Platform => "platform",
            Self::Null => "null",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "owner" | "o" | "0x40000001" => Some(Self::Owner),
            "endorsement" | "e" | "0x4000000b" => Some(Self::Endorsement),
            "platform" | "p" | "0x4000000c" => Some(Self::Platform),
            "null" | "n" | "0x40000007" => Some(Self::Null),
            _ => None,
        }
    }

    fn handle_value(self) -> u32 {
        match self {
            Self::Owner => 0x4000_0001,
            Self::Endorsement => 0x4000_000B,
            Self::Platform => 0x4000_000C,
            Self::Null => 0x4000_0007,
        }
    }
}

/// Key usage attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyAttributes {
    _fixed_tpm: bool,
    _fixed_parent: bool,
    _sensitive_data_origin: bool,
    _user_with_auth: bool,
    _sign_encrypt: bool,
    _decrypt: bool,
    _restricted: bool,
}

impl KeyAttributes {
    fn default_primary() -> Self {
        Self {
            _fixed_tpm: true,
            _fixed_parent: true,
            _sensitive_data_origin: true,
            _user_with_auth: true,
            _sign_encrypt: false,
            _decrypt: true,
            _restricted: true,
        }
    }

    fn default_signing() -> Self {
        Self {
            _fixed_tpm: true,
            _fixed_parent: true,
            _sensitive_data_origin: true,
            _user_with_auth: true,
            _sign_encrypt: true,
            _decrypt: false,
            _restricted: false,
        }
    }

    fn default_storage() -> Self {
        Self {
            _fixed_tpm: true,
            _fixed_parent: true,
            _sensitive_data_origin: true,
            _user_with_auth: true,
            _sign_encrypt: false,
            _decrypt: true,
            _restricted: true,
        }
    }

    fn to_bits(self) -> u32 {
        let mut bits = 0u32;
        if self._fixed_tpm { bits |= 1 << 1; }
        if self._fixed_parent { bits |= 1 << 4; }
        if self._sensitive_data_origin { bits |= 1 << 5; }
        if self._user_with_auth { bits |= 1 << 6; }
        if self._sign_encrypt { bits |= 1 << 18; }
        if self._decrypt { bits |= 1 << 17; }
        if self._restricted { bits |= 1 << 16; }
        bits
    }
}

// ---------------------------------------------------------------------------
// Key object
// ---------------------------------------------------------------------------

/// A loaded key in the TPM context.
#[derive(Clone, Debug)]
struct KeyObject {
    handle: u32,
    key_type: KeyType,
    _hierarchy: Hierarchy,
    hash_alg: HashAlgorithm,
    _attributes: KeyAttributes,
    pub_key: Vec<u8>,
    priv_key: Vec<u8>,
    _parent_handle: u32,
    _is_primary: bool,
    _name: Vec<u8>,
}

impl KeyObject {
    fn new_primary(
        handle: u32,
        key_type: KeyType,
        hierarchy: Hierarchy,
        hash_alg: HashAlgorithm,
        prng: &mut SimPrng,
    ) -> Self {
        let mut pub_key = vec![0u8; key_type.pub_key_size()];
        prng.fill_bytes(&mut pub_key);
        let mut priv_key = vec![0u8; key_type.priv_key_size()];
        prng.fill_bytes(&mut priv_key);
        let name = sim_hash(hash_alg, &pub_key);
        Self {
            handle,
            key_type,
            _hierarchy: hierarchy,
            hash_alg,
            _attributes: KeyAttributes::default_primary(),
            pub_key,
            priv_key,
            _parent_handle: hierarchy.handle_value(),
            _is_primary: true,
            _name: name,
        }
    }

    fn new_child(
        handle: u32,
        key_type: KeyType,
        hierarchy: Hierarchy,
        hash_alg: HashAlgorithm,
        parent_handle: u32,
        usage: &str,
        prng: &mut SimPrng,
    ) -> Self {
        let mut pub_key = vec![0u8; key_type.pub_key_size()];
        prng.fill_bytes(&mut pub_key);
        let mut priv_key = vec![0u8; key_type.priv_key_size()];
        prng.fill_bytes(&mut priv_key);
        let name = sim_hash(hash_alg, &pub_key);
        let attrs = match usage {
            "sign" | "signing" => KeyAttributes::default_signing(),
            "storage" | "decrypt" => KeyAttributes::default_storage(),
            _ => KeyAttributes::default_signing(),
        };
        Self {
            handle,
            key_type,
            _hierarchy: hierarchy,
            hash_alg,
            _attributes: attrs,
            pub_key,
            priv_key,
            _parent_handle: parent_handle,
            _is_primary: false,
            _name: name,
        }
    }

    /// Produce a simulated signature over the given data.
    fn sign(&self, data: &[u8]) -> Vec<u8> {
        let digest = sim_hash(self.hash_alg, data);
        let combined: Vec<u8> = digest.iter().chain(self.priv_key.iter()).copied().collect();
        let sig_hash = sim_hash(self.hash_alg, &combined);
        // Repeat/truncate to match the expected signature size.
        let sig_size = self.key_type.sig_size();
        let mut sig = Vec::with_capacity(sig_size);
        while sig.len() < sig_size {
            let remaining = sig_size - sig.len();
            let take = remaining.min(sig_hash.len());
            sig.extend_from_slice(&sig_hash[..take]);
        }
        sig.truncate(sig_size);
        sig
    }

    /// Verify a simulated signature.
    fn verify(&self, data: &[u8], signature: &[u8]) -> bool {
        let expected = self.sign(data);
        expected == signature
    }
}

// ---------------------------------------------------------------------------
// PCR Bank
// ---------------------------------------------------------------------------

/// A single PCR bank for one hash algorithm.
#[derive(Clone, Debug)]
struct PcrBank {
    algorithm: HashAlgorithm,
    registers: Vec<Vec<u8>>,
}

impl PcrBank {
    /// Create a new PCR bank with all registers initialized to zero.
    fn new(algorithm: HashAlgorithm) -> Self {
        let digest_len = algorithm.digest_len();
        let registers = (0..PCR_COUNT).map(|_| vec![0u8; digest_len]).collect();
        Self { algorithm, registers }
    }

    /// Read a PCR register value.
    fn read(&self, index: usize) -> Option<&[u8]> {
        self.registers.get(index).map(|v| v.as_slice())
    }

    /// Extend a PCR register: new_value = hash(old_value || extend_data).
    fn extend(&mut self, index: usize, data: &[u8]) -> Result<(), TpmError> {
        if index >= PCR_COUNT {
            return Err(TpmError::InvalidPcrIndex(index));
        }
        let old = self.registers[index].clone();
        let mut combined = old;
        combined.extend_from_slice(data);
        self.registers[index] = sim_hash(self.algorithm, &combined);
        Ok(())
    }

    /// Reset a PCR register to all zeros.
    fn _reset(&mut self, index: usize) -> Result<(), TpmError> {
        if index >= PCR_COUNT {
            return Err(TpmError::InvalidPcrIndex(index));
        }
        let digest_len = self.algorithm.digest_len();
        self.registers[index] = vec![0u8; digest_len];
        Ok(())
    }

    /// Check if a PCR is all zeros (not yet extended).
    fn is_zero(&self, index: usize) -> bool {
        self.registers.get(index).is_some_and(|v| v.iter().all(|&b| b == 0))
    }
}

// ---------------------------------------------------------------------------
// NV Index
// ---------------------------------------------------------------------------

/// NV index attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NvAttributes {
    _owner_write: bool,
    _owner_read: bool,
    _auth_write: bool,
    _auth_read: bool,
    _write_locked: bool,
    _read_locked: bool,
    _written: bool,
    _platform_create: bool,
}

impl NvAttributes {
    fn default_owner() -> Self {
        Self {
            _owner_write: true,
            _owner_read: true,
            _auth_write: true,
            _auth_read: true,
            _write_locked: false,
            _read_locked: false,
            _written: false,
            _platform_create: false,
        }
    }

    fn default_platform() -> Self {
        Self {
            _owner_write: false,
            _owner_read: true,
            _auth_write: true,
            _auth_read: true,
            _write_locked: false,
            _read_locked: false,
            _written: false,
            _platform_create: true,
        }
    }

    fn to_bits(self) -> u32 {
        let mut bits = 0u32;
        if self._owner_write { bits |= 1 << 0; }
        if self._owner_read { bits |= 1 << 1; }
        if self._auth_write { bits |= 1 << 2; }
        if self._auth_read { bits |= 1 << 3; }
        if self._write_locked { bits |= 1 << 10; }
        if self._read_locked { bits |= 1 << 11; }
        if self._written { bits |= 1 << 29; }
        if self._platform_create { bits |= 1 << 30; }
        bits
    }
}

/// An NV (Non-Volatile) storage index.
#[derive(Clone, Debug)]
struct NvIndex {
    index: u32,
    size: usize,
    attributes: NvAttributes,
    _auth_policy: Vec<u8>,
    data: Vec<u8>,
    _hash_alg: HashAlgorithm,
}

impl NvIndex {
    fn new(
        index: u32,
        size: usize,
        hash_alg: HashAlgorithm,
        platform_create: bool,
    ) -> Result<Self, TpmError> {
        if size > NV_MAX_SIZE {
            return Err(TpmError::NvSizeTooLarge(size));
        }
        let attrs = if platform_create {
            NvAttributes::default_platform()
        } else {
            NvAttributes::default_owner()
        };
        Ok(Self {
            index,
            size,
            attributes: attrs,
            _auth_policy: Vec::new(),
            data: vec![0u8; size],
            _hash_alg: hash_alg,
        })
    }

    fn write(&mut self, offset: usize, data: &[u8]) -> Result<(), TpmError> {
        if self.attributes._write_locked {
            return Err(TpmError::NvWriteLocked(self.index));
        }
        if offset + data.len() > self.size {
            return Err(TpmError::NvWriteOutOfBounds {
                index: self.index,
                offset,
                len: data.len(),
                size: self.size,
            });
        }
        self.data[offset..offset + data.len()].copy_from_slice(data);
        self.attributes._written = true;
        Ok(())
    }

    fn read(&self, offset: usize, len: usize) -> Result<&[u8], TpmError> {
        if self.attributes._read_locked {
            return Err(TpmError::NvReadLocked(self.index));
        }
        if offset + len > self.size {
            return Err(TpmError::NvReadOutOfBounds {
                index: self.index,
                offset,
                len,
                size: self.size,
            });
        }
        Ok(&self.data[offset..offset + len])
    }
}

// ---------------------------------------------------------------------------
// TPM Error
// ---------------------------------------------------------------------------

/// Error type for TPM operations.
#[derive(Clone, Debug, PartialEq, Eq)]
enum TpmError {
    InvalidPcrIndex(usize),
    UnknownAlgorithm(String),
    NvIndexExists(u32),
    NvIndexNotFound(u32),
    NvSizeTooLarge(usize),
    NvWriteLocked(u32),
    NvReadLocked(u32),
    NvWriteOutOfBounds { index: u32, offset: usize, len: usize, size: usize },
    NvReadOutOfBounds { index: u32, offset: usize, len: usize, size: usize },
    NvTooManyIndices,
    KeyNotFound(u32),
    TooManyKeys,
    SelfTestFailed(String),
    _InvalidHandle(u32),
    _InvalidArgument(String),
    _NotInitialized,
    _AlreadyCleared,
}

impl TpmError {
    fn message(&self) -> String {
        match self {
            Self::InvalidPcrIndex(i) => format!("invalid PCR index: {i}"),
            Self::UnknownAlgorithm(a) => format!("unknown algorithm: {a}"),
            Self::NvIndexExists(i) => format!("NV index 0x{i:08X} already defined"),
            Self::NvIndexNotFound(i) => format!("NV index 0x{i:08X} not found"),
            Self::NvSizeTooLarge(s) => format!("NV size {s} exceeds maximum {NV_MAX_SIZE}"),
            Self::NvWriteLocked(i) => format!("NV index 0x{i:08X} is write-locked"),
            Self::NvReadLocked(i) => format!("NV index 0x{i:08X} is read-locked"),
            Self::NvWriteOutOfBounds { index, offset, len, size } => {
                format!("NV write out of bounds: index=0x{index:08X} offset={offset} len={len} size={size}")
            }
            Self::NvReadOutOfBounds { index, offset, len, size } => {
                format!("NV read out of bounds: index=0x{index:08X} offset={offset} len={len} size={size}")
            }
            Self::NvTooManyIndices => "too many NV indices defined".to_string(),
            Self::KeyNotFound(h) => format!("key handle 0x{h:08X} not found"),
            Self::TooManyKeys => "too many key objects loaded".to_string(),
            Self::SelfTestFailed(msg) => format!("self-test failed: {msg}"),
            Self::_InvalidHandle(h) => format!("invalid handle: 0x{h:08X}"),
            Self::_InvalidArgument(msg) => format!("invalid argument: {msg}"),
            Self::_NotInitialized => "TPM not initialized".to_string(),
            Self::_AlreadyCleared => "TPM already cleared".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test status
// ---------------------------------------------------------------------------

/// TPM self-test state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelfTestState {
    NotRun,
    Passed,
    _Failed,
}

// ---------------------------------------------------------------------------
// Capability types
// ---------------------------------------------------------------------------

/// TPM capability categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapabilityType {
    Algorithms,
    Handles,
    Commands,
    Properties,
    PcrBanks,
}

impl CapabilityType {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "algorithms" | "algs" => Some(Self::Algorithms),
            "handles" => Some(Self::Handles),
            "commands" | "cmds" => Some(Self::Commands),
            "properties" | "props" => Some(Self::Properties),
            "pcrs" | "pcr-banks" => Some(Self::PcrBanks),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Algorithms => "algorithms",
            Self::Handles => "handles",
            Self::Commands => "commands",
            Self::Properties => "properties",
            Self::PcrBanks => "pcrs",
        }
    }
}

// ---------------------------------------------------------------------------
// TPM Context (main state)
// ---------------------------------------------------------------------------

/// Full simulated TPM 2.0 context.
struct TpmContext {
    pcr_banks: Vec<PcrBank>,
    nv_indices: Vec<NvIndex>,
    keys: Vec<KeyObject>,
    prng: SimPrng,
    _self_test_state: SelfTestState,
    _initialized: bool,
    next_handle: u32,
    _lockout_counter: u32,
    _max_lockout: u32,
}

impl TpmContext {
    /// Create a new TPM context with default PCR banks.
    fn new() -> Self {
        let pcr_banks = HashAlgorithm::all()
            .iter()
            .map(|&alg| PcrBank::new(alg))
            .collect();
        Self {
            pcr_banks,
            nv_indices: Vec::new(),
            keys: Vec::new(),
            prng: SimPrng::new(0x5448_4953_4953_5450), // "THISIST P"
            _self_test_state: SelfTestState::NotRun,
            _initialized: true,
            next_handle: 0x8000_0000,
            _lockout_counter: 0,
            _max_lockout: 32,
        }
    }

    /// Allocate the next available handle.
    fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1);
        h
    }

    // --- PCR operations ---

    fn pcr_bank(&self, alg: HashAlgorithm) -> Option<&PcrBank> {
        self.pcr_banks.iter().find(|b| b.algorithm == alg)
    }

    fn pcr_bank_mut(&mut self, alg: HashAlgorithm) -> Option<&mut PcrBank> {
        self.pcr_banks.iter_mut().find(|b| b.algorithm == alg)
    }

    fn pcr_read(&self, alg: HashAlgorithm, index: usize) -> Result<Vec<u8>, TpmError> {
        let bank = self.pcr_bank(alg)
            .ok_or_else(|| TpmError::UnknownAlgorithm(alg.name().to_string()))?;
        bank.read(index)
            .map(|v| v.to_vec())
            .ok_or(TpmError::InvalidPcrIndex(index))
    }

    fn pcr_extend(&mut self, alg: HashAlgorithm, index: usize, data: &[u8]) -> Result<(), TpmError> {
        let bank = self.pcr_bank_mut(alg)
            .ok_or_else(|| TpmError::UnknownAlgorithm(alg.name().to_string()))?;
        bank.extend(index, data)
    }

    fn pcr_read_all(&self, alg: HashAlgorithm) -> Result<Vec<(usize, Vec<u8>)>, TpmError> {
        let bank = self.pcr_bank(alg)
            .ok_or_else(|| TpmError::UnknownAlgorithm(alg.name().to_string()))?;
        let mut results = Vec::new();
        for i in 0..PCR_COUNT {
            if let Some(val) = bank.read(i) {
                results.push((i, val.to_vec()));
            }
        }
        Ok(results)
    }

    // --- NV operations ---

    fn nv_define(
        &mut self,
        index: u32,
        size: usize,
        hash_alg: HashAlgorithm,
        platform: bool,
    ) -> Result<(), TpmError> {
        if self.nv_indices.iter().any(|nv| nv.index == index) {
            return Err(TpmError::NvIndexExists(index));
        }
        if self.nv_indices.len() >= NV_MAX_INDICES {
            return Err(TpmError::NvTooManyIndices);
        }
        let nv = NvIndex::new(index, size, hash_alg, platform)?;
        self.nv_indices.push(nv);
        Ok(())
    }

    fn nv_write(&mut self, index: u32, offset: usize, data: &[u8]) -> Result<(), TpmError> {
        let nv = self.nv_indices.iter_mut()
            .find(|nv| nv.index == index)
            .ok_or(TpmError::NvIndexNotFound(index))?;
        nv.write(offset, data)
    }

    fn nv_read(&self, index: u32, offset: usize, len: usize) -> Result<Vec<u8>, TpmError> {
        let nv = self.nv_indices.iter()
            .find(|nv| nv.index == index)
            .ok_or(TpmError::NvIndexNotFound(index))?;
        nv.read(offset, len).map(|s| s.to_vec())
    }

    fn nv_find(&self, index: u32) -> Option<&NvIndex> {
        self.nv_indices.iter().find(|nv| nv.index == index)
    }

    fn _nv_undefine(&mut self, index: u32) -> Result<(), TpmError> {
        let pos = self.nv_indices.iter()
            .position(|nv| nv.index == index)
            .ok_or(TpmError::NvIndexNotFound(index))?;
        self.nv_indices.remove(pos);
        Ok(())
    }

    // --- Key operations ---

    fn create_primary(
        &mut self,
        key_type: KeyType,
        hierarchy: Hierarchy,
        hash_alg: HashAlgorithm,
    ) -> Result<u32, TpmError> {
        if self.keys.len() >= KEY_MAX_OBJECTS {
            return Err(TpmError::TooManyKeys);
        }
        let handle = self.alloc_handle();
        let key = KeyObject::new_primary(handle, key_type, hierarchy, hash_alg, &mut self.prng);
        self.keys.push(key);
        Ok(handle)
    }

    fn create_child(
        &mut self,
        parent_handle: u32,
        key_type: KeyType,
        hash_alg: HashAlgorithm,
        usage: &str,
    ) -> Result<(Vec<u8>, Vec<u8>), TpmError> {
        // Verify parent exists
        let parent = self.keys.iter()
            .find(|k| k.handle == parent_handle)
            .ok_or(TpmError::KeyNotFound(parent_handle))?;
        let hierarchy = parent._hierarchy;
        if self.keys.len() >= KEY_MAX_OBJECTS {
            return Err(TpmError::TooManyKeys);
        }
        let handle = self.alloc_handle();
        let key = KeyObject::new_child(
            handle, key_type, hierarchy, hash_alg, parent_handle, usage, &mut self.prng,
        );
        let pub_key = key.pub_key.clone();
        let priv_key = key.priv_key.clone();
        self.keys.push(key);
        Ok((pub_key, priv_key))
    }

    fn load_key(
        &mut self,
        parent_handle: u32,
        pub_key: &[u8],
        priv_key: &[u8],
        key_type: KeyType,
        hash_alg: HashAlgorithm,
    ) -> Result<u32, TpmError> {
        // Verify parent exists
        let parent = self.keys.iter()
            .find(|k| k.handle == parent_handle)
            .ok_or(TpmError::KeyNotFound(parent_handle))?;
        let hierarchy = parent._hierarchy;
        if self.keys.len() >= KEY_MAX_OBJECTS {
            return Err(TpmError::TooManyKeys);
        }
        let handle = self.alloc_handle();
        let name = sim_hash(hash_alg, pub_key);
        let key = KeyObject {
            handle,
            key_type,
            _hierarchy: hierarchy,
            hash_alg,
            _attributes: KeyAttributes::default_signing(),
            pub_key: pub_key.to_vec(),
            priv_key: priv_key.to_vec(),
            _parent_handle: parent_handle,
            _is_primary: false,
            _name: name,
        };
        self.keys.push(key);
        Ok(handle)
    }

    fn find_key(&self, handle: u32) -> Option<&KeyObject> {
        self.keys.iter().find(|k| k.handle == handle)
    }

    fn sign(&self, handle: u32, data: &[u8]) -> Result<Vec<u8>, TpmError> {
        let key = self.find_key(handle)
            .ok_or(TpmError::KeyNotFound(handle))?;
        Ok(key.sign(data))
    }

    fn verify(&self, handle: u32, data: &[u8], signature: &[u8]) -> Result<bool, TpmError> {
        let key = self.find_key(handle)
            .ok_or(TpmError::KeyNotFound(handle))?;
        Ok(key.verify(data, signature))
    }

    fn _flush_key(&mut self, handle: u32) -> Result<(), TpmError> {
        let pos = self.keys.iter()
            .position(|k| k.handle == handle)
            .ok_or(TpmError::KeyNotFound(handle))?;
        self.keys.remove(pos);
        Ok(())
    }

    // --- Self-test ---

    fn self_test(&mut self, full: bool) -> Result<(), TpmError> {
        // Simulated self-test always passes.
        if full {
            // Simulate checking every algorithm
            for alg in HashAlgorithm::all() {
                let test_data = b"TPM2 self-test data";
                let hash = sim_hash(*alg, test_data);
                if hash.len() != alg.digest_len() {
                    return Err(TpmError::SelfTestFailed(
                        format!("digest length mismatch for {}", alg.name()),
                    ));
                }
            }
        }
        self._self_test_state = SelfTestState::Passed;
        Ok(())
    }

    fn self_test_state(&self) -> SelfTestState {
        self._self_test_state
    }

    // --- Get random ---

    fn get_random(&mut self, count: usize) -> Vec<u8> {
        let mut buf = vec![0u8; count];
        self.prng.fill_bytes(&mut buf);
        buf
    }

    // --- Clear ---

    fn clear(&mut self) -> Result<(), TpmError> {
        // Reset all PCR banks
        self.pcr_banks = HashAlgorithm::all()
            .iter()
            .map(|&alg| PcrBank::new(alg))
            .collect();
        // Remove all NV indices
        self.nv_indices.clear();
        // Flush all keys
        self.keys.clear();
        // Reset PRNG
        self.prng = SimPrng::new(0x5448_4953_4953_5450);
        self._self_test_state = SelfTestState::NotRun;
        self.next_handle = 0x8000_0000;
        self._lockout_counter = 0;
        Ok(())
    }

    // --- Capability queries ---

    fn get_capability(&self, cap_type: CapabilityType) -> Vec<String> {
        match cap_type {
            CapabilityType::Algorithms => {
                HashAlgorithm::all().iter().map(|a| {
                    format!("  {}:  value: 0x{:04X}  hash-size: {}",
                        a.name(), a.alg_id(), a.digest_len())
                }).collect()
            }
            CapabilityType::Handles => {
                let mut lines = Vec::new();
                for key in &self.keys {
                    lines.push(format!("  0x{:08X}  type: {}  primary: {}",
                        key.handle, key.key_type.name(), key._is_primary));
                }
                for nv in &self.nv_indices {
                    lines.push(format!("  0x{:08X}  nv-index  size: {}",
                        nv.index, nv.size));
                }
                lines
            }
            CapabilityType::Commands => {
                vec![
                    "  TPM2_CC_GetRandom".to_string(),
                    "  TPM2_CC_PCR_Read".to_string(),
                    "  TPM2_CC_PCR_Extend".to_string(),
                    "  TPM2_CC_CreatePrimary".to_string(),
                    "  TPM2_CC_Create".to_string(),
                    "  TPM2_CC_Load".to_string(),
                    "  TPM2_CC_Sign".to_string(),
                    "  TPM2_CC_VerifySignature".to_string(),
                    "  TPM2_CC_NV_DefineSpace".to_string(),
                    "  TPM2_CC_NV_Write".to_string(),
                    "  TPM2_CC_NV_Read".to_string(),
                    "  TPM2_CC_GetCapability".to_string(),
                    "  TPM2_CC_SelfTest".to_string(),
                    "  TPM2_CC_Clear".to_string(),
                    "  TPM2_CC_FlushContext".to_string(),
                    "  TPM2_CC_NV_UndefineSpace".to_string(),
                ]
            }
            CapabilityType::Properties => {
                vec![
                    format!("  TPM2_PT_MANUFACTURER:       {TPM_MANUFACTURER}"),
                    format!("  TPM2_PT_FIRMWARE_VERSION:   {TPM_FIRMWARE_VERSION}"),
                    format!("  TPM2_PT_SPEC_FAMILY:        {TPM_SPEC_FAMILY}"),
                    format!("  TPM2_PT_SPEC_LEVEL:         {TPM_SPEC_LEVEL}"),
                    format!("  TPM2_PT_SPEC_REVISION:      {TPM_SPEC_REVISION}"),
                    format!("  TPM2_PT_MAX_NV_INDEX_SIZE:  {NV_MAX_SIZE}"),
                    format!("  TPM2_PT_MAX_NV_INDICES:     {NV_MAX_INDICES}"),
                    format!("  TPM2_PT_MAX_LOADED_OBJECTS: {KEY_MAX_OBJECTS}"),
                    format!("  TPM2_PT_PCR_COUNT:          {PCR_COUNT}"),
                ]
            }
            CapabilityType::PcrBanks => {
                self.pcr_banks.iter().map(|bank| {
                    let active_count = (0..PCR_COUNT)
                        .filter(|&i| !bank.is_zero(i))
                        .count();
                    format!("  {}:  id: 0x{:04X}  active-pcrs: {}/{}",
                        bank.algorithm.name(), bank.algorithm.alg_id(),
                        active_count, PCR_COUNT)
                }).collect()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Argument parsing helpers
// ---------------------------------------------------------------------------

/// Parse a u32 from a string, supporting decimal and 0x-prefixed hex.
fn parse_u32(s: &str) -> Option<u32> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

/// Parse a usize from a string, supporting decimal and 0x-prefixed hex.
fn parse_usize(s: &str) -> Option<usize> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<usize>().ok()
    }
}

/// Find a flag's value in args: looks for `--flag VALUE` or `--flag=VALUE`.
fn find_flag<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let prefix = format!("{flag}=");
    for (i, arg) in args.iter().enumerate() {
        if arg == flag {
            if i + 1 < args.len() {
                return Some(&args[i + 1]);
            }
        } else if let Some(val) = arg.strip_prefix(&prefix) {
            return Some(val);
        }
    }
    None
}

/// Check if a boolean flag is present in args.
fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

/// Get positional arguments (those not starting with '--').
fn positional_args(args: &[String]) -> Vec<&str> {
    let mut pos = Vec::new();
    let mut skip_next = false;
    for (i, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg.starts_with("--") {
            // If this is a flag that takes a value (no '=' in it), skip next arg too
            if !arg.contains('=') {
                // Check if it's a known value-flag
                let known_value_flags = [
                    "--size", "--hash-algorithm", "--hierarchy", "--key-algorithm",
                    "--parent", "--usage", "--handle", "--offset", "--count",
                    "--index", "--capability", "--input", "--signature", "--ticket",
                    "--public", "--private", "--message", "--format",
                ];
                if known_value_flags.contains(&arg.as_str()) && i + 1 < args.len() {
                    skip_next = true;
                }
            }
            continue;
        }
        pos.push(arg.as_str());
    }
    pos
}

// ---------------------------------------------------------------------------
// Output formatting helpers
// ---------------------------------------------------------------------------

/// Format bytes as colon-separated hex (e.g., "0a:1b:2c:...").
fn format_hex_colon(bytes: &[u8]) -> String {
    let parts: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    parts.join(":")
}

/// Format bytes in a hex dump style (16 bytes per line).
fn format_hex_dump(bytes: &[u8]) -> String {
    let mut lines = Vec::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let offset = i * 16;
        let hex_part: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii_part: String = chunk.iter().map(|&b| {
            if (0x20..=0x7E).contains(&b) { b as char } else { '.' }
        }).collect();
        lines.push(format!("{offset:08x}: {:<48}  {ascii_part}", hex_part.join(" ")));
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

/// `tpm2_getrandom` -- get random bytes from the simulated TPM.
fn cmd_getrandom(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_getrandom [OPTIONS] NUM_BYTES");
        println!();
        println!("Get random bytes from the TPM random number generator.");
        println!();
        println!("Options:");
        println!("  --hex           Output as hex string (default)");
        println!("  --raw           Output as raw bytes");
        println!("  --format FMT    Output format: hex, raw, colon");
        println!("  --help, -h      Show this help");
        println!("  --version       Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_getrandom version {VERSION}");
        return 0;
    }

    let pos = positional_args(args);
    let count = match pos.first() {
        Some(s) => match parse_usize(s) {
            Some(n) => n,
            None => {
                eprintln!("tpm2_getrandom: invalid byte count: {s}");
                return 1;
            }
        },
        None => {
            eprintln!("tpm2_getrandom: missing byte count argument");
            eprintln!("Usage: tpm2_getrandom NUM_BYTES");
            return 1;
        }
    };

    if count == 0 {
        return 0;
    }

    if count > 4096 {
        eprintln!("tpm2_getrandom: requested {count} bytes exceeds maximum 4096");
        return 1;
    }

    let bytes = ctx.get_random(count);

    let format = find_flag(args, "--format").unwrap_or(
        if has_flag(args, "--raw") { "raw" } else { "hex" }
    );

    match format {
        "raw" => {
            // Print raw bytes (as escaped representation for simulation)
            for b in &bytes {
                print!("{}", *b as char);
            }
        }
        "colon" => {
            println!("{}", format_hex_colon(&bytes));
        }
        _ => {
            // hex (default)
            println!("{}", hex_encode(&bytes));
        }
    }

    0
}

/// `tpm2_pcrread` -- read PCR registers.
fn cmd_pcrread(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_pcrread [OPTIONS] [ALG:PCR_LIST]");
        println!();
        println!("Read PCR register values.");
        println!();
        println!("Examples:");
        println!("  tpm2_pcrread sha256:0,1,2");
        println!("  tpm2_pcrread sha1:0+sha256:0");
        println!("  tpm2_pcrread --all");
        println!();
        println!("Options:");
        println!("  --all                  Read all PCRs for all banks");
        println!("  --hash-algorithm ALG   Algorithm to use (default: sha256)");
        println!("  --output FILE          Output file (not implemented in simulation)");
        println!("  --help, -h             Show this help");
        println!("  --version              Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_pcrread version {VERSION}");
        return 0;
    }

    if has_flag(args, "--all") {
        // Print all PCR values for all banks
        for alg in HashAlgorithm::all() {
            println!("  {}:", alg.name());
            match ctx.pcr_read_all(*alg) {
                Ok(pcrs) => {
                    for (idx, val) in &pcrs {
                        println!("    {idx:2}: 0x{}", hex_encode(val));
                    }
                }
                Err(e) => {
                    eprintln!("tpm2_pcrread: {}", e.message());
                    return 1;
                }
            }
        }
        return 0;
    }

    // Parse PCR selection: "sha256:0,1,2" or "sha256:0+sha1:0"
    let pos = positional_args(args);
    if pos.is_empty() {
        // Default: read all sha256 PCRs
        let alg = HashAlgorithm::Sha256;
        println!("  {}:", alg.name());
        match ctx.pcr_read_all(alg) {
            Ok(pcrs) => {
                for (idx, val) in &pcrs {
                    println!("    {idx:2}: 0x{}", hex_encode(val));
                }
            }
            Err(e) => {
                eprintln!("tpm2_pcrread: {}", e.message());
                return 1;
            }
        }
        return 0;
    }

    // Parse selection(s)
    for selection_str in pos {
        let selections: Vec<&str> = selection_str.split('+').collect();
        for sel in selections {
            let parts: Vec<&str> = sel.splitn(2, ':').collect();
            if parts.len() != 2 {
                eprintln!("tpm2_pcrread: invalid PCR selection format: {sel}");
                eprintln!("Expected format: ALG:INDEX[,INDEX...]");
                return 1;
            }
            let alg = match HashAlgorithm::from_str(parts[0]) {
                Some(a) => a,
                None => {
                    eprintln!("tpm2_pcrread: unknown algorithm: {}", parts[0]);
                    return 1;
                }
            };
            let indices: Vec<&str> = parts[1].split(',').collect();
            println!("  {}:", alg.name());
            for idx_str in indices {
                let idx = match parse_usize(idx_str) {
                    Some(i) => i,
                    None => {
                        eprintln!("tpm2_pcrread: invalid PCR index: {idx_str}");
                        return 1;
                    }
                };
                match ctx.pcr_read(alg, idx) {
                    Ok(val) => {
                        println!("    {idx:2}: 0x{}", hex_encode(&val));
                    }
                    Err(e) => {
                        eprintln!("tpm2_pcrread: {}", e.message());
                        return 1;
                    }
                }
            }
        }
    }

    0
}

/// `tpm2_pcrextend` -- extend a PCR register.
fn cmd_pcrextend(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_pcrextend [OPTIONS] PCR_INDEX ALG:HASH_HEX");
        println!();
        println!("Extend a PCR register with a hash value.");
        println!();
        println!("Examples:");
        println!("  tpm2_pcrextend 0 sha256:abc123...");
        println!("  tpm2_pcrextend 7 sha1:da39a3...");
        println!();
        println!("Options:");
        println!("  --help, -h      Show this help");
        println!("  --version       Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_pcrextend version {VERSION}");
        return 0;
    }

    let pos = positional_args(args);
    if pos.len() < 2 {
        eprintln!("tpm2_pcrextend: requires PCR_INDEX and ALG:HASH_HEX arguments");
        return 1;
    }

    let pcr_index = match parse_usize(pos[0]) {
        Some(i) => i,
        None => {
            eprintln!("tpm2_pcrextend: invalid PCR index: {}", pos[0]);
            return 1;
        }
    };

    let parts: Vec<&str> = pos[1].splitn(2, ':').collect();
    if parts.len() != 2 {
        eprintln!("tpm2_pcrextend: expected format ALG:HASH_HEX, got: {}", pos[1]);
        return 1;
    }

    let alg = match HashAlgorithm::from_str(parts[0]) {
        Some(a) => a,
        None => {
            eprintln!("tpm2_pcrextend: unknown algorithm: {}", parts[0]);
            return 1;
        }
    };

    let hash_bytes = match hex_decode(parts[1]) {
        Some(b) => b,
        None => {
            eprintln!("tpm2_pcrextend: invalid hex string: {}", parts[1]);
            return 1;
        }
    };

    if hash_bytes.len() != alg.digest_len() {
        eprintln!(
            "tpm2_pcrextend: hash length {} does not match {} digest length {}",
            hash_bytes.len(), alg.name(), alg.digest_len()
        );
        return 1;
    }

    match ctx.pcr_extend(alg, pcr_index, &hash_bytes) {
        Ok(()) => {
            println!("PCR[{pcr_index}] ({}) extended successfully.", alg.name());
            0
        }
        Err(e) => {
            eprintln!("tpm2_pcrextend: {}", e.message());
            1
        }
    }
}

/// `tpm2_createprimary` -- create a primary key.
fn cmd_createprimary(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_createprimary [OPTIONS]");
        println!();
        println!("Create a primary key in the specified hierarchy.");
        println!();
        println!("Options:");
        println!("  --hierarchy H         Hierarchy: owner, endorsement, platform, null (default: owner)");
        println!("  --hash-algorithm ALG  Hash algorithm (default: sha256)");
        println!("  --key-algorithm ALG   Key algorithm: rsa, rsa2048, rsa3072, ecc, ecc256, ecc384 (default: rsa2048)");
        println!("  --help, -h            Show this help");
        println!("  --version             Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_createprimary version {VERSION}");
        return 0;
    }

    let hierarchy = match find_flag(args, "--hierarchy") {
        Some(h) => match Hierarchy::from_str(h) {
            Some(hier) => hier,
            None => {
                eprintln!("tpm2_createprimary: unknown hierarchy: {h}");
                return 1;
            }
        },
        None => Hierarchy::Owner,
    };

    let hash_alg = match find_flag(args, "--hash-algorithm") {
        Some(a) => match HashAlgorithm::from_str(a) {
            Some(alg) => alg,
            None => {
                eprintln!("tpm2_createprimary: unknown hash algorithm: {a}");
                return 1;
            }
        },
        None => HashAlgorithm::Sha256,
    };

    let key_type = match find_flag(args, "--key-algorithm") {
        Some(k) => match KeyType::from_str(k) {
            Some(kt) => kt,
            None => {
                eprintln!("tpm2_createprimary: unknown key algorithm: {k}");
                return 1;
            }
        },
        None => KeyType::Rsa2048,
    };

    match ctx.create_primary(key_type, hierarchy, hash_alg) {
        Ok(handle) => {
            println!("name-alg:");
            println!("  value: {}", hash_alg.name());
            println!("  raw: 0x{:04X}", hash_alg.alg_id());
            println!("attributes:");
            println!("  value: fixedTPM|fixedParent|sensitiveDO|userWithAuth|restricted|decrypt");
            println!("  raw: 0x{:08X}", KeyAttributes::default_primary().to_bits());
            println!("type:");
            println!("  value: {}", key_type.name());
            println!("hierarchy:");
            println!("  value: {} (0x{:08X})", hierarchy.name(), hierarchy.handle_value());
            println!("handle: 0x{handle:08X}");
            0
        }
        Err(e) => {
            eprintln!("tpm2_createprimary: {}", e.message());
            1
        }
    }
}

/// `tpm2_create` -- create a child key.
fn cmd_create(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_create [OPTIONS]");
        println!();
        println!("Create a child key under a parent primary key.");
        println!();
        println!("Options:");
        println!("  --parent HANDLE       Parent key handle (required)");
        println!("  --hash-algorithm ALG  Hash algorithm (default: sha256)");
        println!("  --key-algorithm ALG   Key algorithm (default: rsa2048)");
        println!("  --usage USAGE         Key usage: sign, storage (default: sign)");
        println!("  --public FILE         Output public portion (simulation: prints hex)");
        println!("  --private FILE        Output private portion (simulation: prints hex)");
        println!("  --help, -h            Show this help");
        println!("  --version             Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_create version {VERSION}");
        return 0;
    }

    let parent_handle = match find_flag(args, "--parent") {
        Some(h) => match parse_u32(h) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_create: invalid parent handle: {h}");
                return 1;
            }
        },
        None => {
            eprintln!("tpm2_create: --parent is required");
            return 1;
        }
    };

    let hash_alg = match find_flag(args, "--hash-algorithm") {
        Some(a) => match HashAlgorithm::from_str(a) {
            Some(alg) => alg,
            None => {
                eprintln!("tpm2_create: unknown hash algorithm: {a}");
                return 1;
            }
        },
        None => HashAlgorithm::Sha256,
    };

    let key_type = match find_flag(args, "--key-algorithm") {
        Some(k) => match KeyType::from_str(k) {
            Some(kt) => kt,
            None => {
                eprintln!("tpm2_create: unknown key algorithm: {k}");
                return 1;
            }
        },
        None => KeyType::Rsa2048,
    };

    let usage = find_flag(args, "--usage").unwrap_or("sign");

    match ctx.create_child(parent_handle, key_type, hash_alg, usage) {
        Ok((pub_key, priv_key)) => {
            println!("name-alg:");
            println!("  value: {}", hash_alg.name());
            println!("  raw: 0x{:04X}", hash_alg.alg_id());
            let attrs = if usage == "sign" || usage == "signing" {
                KeyAttributes::default_signing()
            } else {
                KeyAttributes::default_storage()
            };
            println!("attributes:");
            println!("  raw: 0x{:08X}", attrs.to_bits());
            println!("type:");
            println!("  value: {}", key_type.name());
            println!("public: {}", hex_encode(&pub_key));
            println!("private: {}", hex_encode(&priv_key));
            0
        }
        Err(e) => {
            eprintln!("tpm2_create: {}", e.message());
            1
        }
    }
}

/// `tpm2_load` -- load a key into the TPM.
fn cmd_load(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_load [OPTIONS]");
        println!();
        println!("Load a key into the TPM.");
        println!();
        println!("Options:");
        println!("  --parent HANDLE       Parent key handle (required)");
        println!("  --public HEX          Public key material as hex");
        println!("  --private HEX         Private key material as hex");
        println!("  --hash-algorithm ALG  Hash algorithm (default: sha256)");
        println!("  --key-algorithm ALG   Key algorithm (default: rsa2048)");
        println!("  --help, -h            Show this help");
        println!("  --version             Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_load version {VERSION}");
        return 0;
    }

    let parent_handle = match find_flag(args, "--parent") {
        Some(h) => match parse_u32(h) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_load: invalid parent handle: {h}");
                return 1;
            }
        },
        None => {
            eprintln!("tpm2_load: --parent is required");
            return 1;
        }
    };

    let pub_hex = match find_flag(args, "--public") {
        Some(h) => h.to_string(),
        None => {
            eprintln!("tpm2_load: --public is required");
            return 1;
        }
    };
    let pub_key = match hex_decode(&pub_hex) {
        Some(b) => b,
        None => {
            eprintln!("tpm2_load: invalid hex for --public");
            return 1;
        }
    };

    let priv_hex = match find_flag(args, "--private") {
        Some(h) => h.to_string(),
        None => {
            eprintln!("tpm2_load: --private is required");
            return 1;
        }
    };
    let priv_key = match hex_decode(&priv_hex) {
        Some(b) => b,
        None => {
            eprintln!("tpm2_load: invalid hex for --private");
            return 1;
        }
    };

    let hash_alg = match find_flag(args, "--hash-algorithm") {
        Some(a) => match HashAlgorithm::from_str(a) {
            Some(alg) => alg,
            None => {
                eprintln!("tpm2_load: unknown hash algorithm: {a}");
                return 1;
            }
        },
        None => HashAlgorithm::Sha256,
    };

    let key_type = match find_flag(args, "--key-algorithm") {
        Some(k) => match KeyType::from_str(k) {
            Some(kt) => kt,
            None => {
                eprintln!("tpm2_load: unknown key algorithm: {k}");
                return 1;
            }
        },
        None => KeyType::Rsa2048,
    };

    match ctx.load_key(parent_handle, &pub_key, &priv_key, key_type, hash_alg) {
        Ok(handle) => {
            println!("loaded-key:");
            println!("  handle: 0x{handle:08X}");
            println!("  name: {}", hex_encode(&sim_hash(hash_alg, &pub_key)));
            0
        }
        Err(e) => {
            eprintln!("tpm2_load: {}", e.message());
            1
        }
    }
}

/// `tpm2_sign` -- sign data with a TPM key.
fn cmd_sign(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_sign [OPTIONS]");
        println!();
        println!("Sign data with a loaded TPM key.");
        println!();
        println!("Options:");
        println!("  --handle HANDLE       Key handle to sign with (required)");
        println!("  --message HEX         Message to sign as hex (required)");
        println!("  --hash-algorithm ALG  Hash algorithm for signing (default: sha256)");
        println!("  --format FMT          Output format: hex, colon (default: hex)");
        println!("  --help, -h            Show this help");
        println!("  --version             Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_sign version {VERSION}");
        return 0;
    }

    let handle = match find_flag(args, "--handle") {
        Some(h) => match parse_u32(h) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_sign: invalid handle: {h}");
                return 1;
            }
        },
        None => {
            eprintln!("tpm2_sign: --handle is required");
            return 1;
        }
    };

    let message_hex = match find_flag(args, "--message") {
        Some(m) => m.to_string(),
        None => {
            eprintln!("tpm2_sign: --message is required");
            return 1;
        }
    };
    let message = match hex_decode(&message_hex) {
        Some(b) => b,
        None => {
            eprintln!("tpm2_sign: invalid hex for --message");
            return 1;
        }
    };

    match ctx.sign(handle, &message) {
        Ok(sig) => {
            let format = find_flag(args, "--format").unwrap_or("hex");
            match format {
                "colon" => println!("{}", format_hex_colon(&sig)),
                _ => println!("{}", hex_encode(&sig)),
            }
            0
        }
        Err(e) => {
            eprintln!("tpm2_sign: {}", e.message());
            1
        }
    }
}

/// `tpm2_verifysignature` -- verify a signature.
fn cmd_verifysignature(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_verifysignature [OPTIONS]");
        println!();
        println!("Verify a signature with a loaded TPM key.");
        println!();
        println!("Options:");
        println!("  --handle HANDLE       Key handle to verify with (required)");
        println!("  --message HEX         Original message as hex (required)");
        println!("  --signature HEX       Signature to verify as hex (required)");
        println!("  --help, -h            Show this help");
        println!("  --version             Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_verifysignature version {VERSION}");
        return 0;
    }

    let handle = match find_flag(args, "--handle") {
        Some(h) => match parse_u32(h) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_verifysignature: invalid handle: {h}");
                return 1;
            }
        },
        None => {
            eprintln!("tpm2_verifysignature: --handle is required");
            return 1;
        }
    };

    let message_hex = match find_flag(args, "--message") {
        Some(m) => m.to_string(),
        None => {
            eprintln!("tpm2_verifysignature: --message is required");
            return 1;
        }
    };
    let message = match hex_decode(&message_hex) {
        Some(b) => b,
        None => {
            eprintln!("tpm2_verifysignature: invalid hex for --message");
            return 1;
        }
    };

    let sig_hex = match find_flag(args, "--signature") {
        Some(s) => s.to_string(),
        None => {
            eprintln!("tpm2_verifysignature: --signature is required");
            return 1;
        }
    };
    let signature = match hex_decode(&sig_hex) {
        Some(b) => b,
        None => {
            eprintln!("tpm2_verifysignature: invalid hex for --signature");
            return 1;
        }
    };

    match ctx.verify(handle, &message, &signature) {
        Ok(true) => {
            println!("signature verification: SUCCESS");
            0
        }
        Ok(false) => {
            println!("signature verification: FAILED");
            1
        }
        Err(e) => {
            eprintln!("tpm2_verifysignature: {}", e.message());
            1
        }
    }
}

/// `tpm2_nvdefine` -- define an NV index.
fn cmd_nvdefine(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_nvdefine [OPTIONS] [NV_INDEX]");
        println!();
        println!("Define a Non-Volatile storage index.");
        println!();
        println!("Options:");
        println!("  --index INDEX         NV index (hex or decimal, e.g. 0x01000001)");
        println!("  --size SIZE           Size in bytes (default: 32)");
        println!("  --hash-algorithm ALG  Hash algorithm (default: sha256)");
        println!("  --hierarchy H         Hierarchy: owner, platform (default: owner)");
        println!("  --help, -h            Show this help");
        println!("  --version             Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_nvdefine version {VERSION}");
        return 0;
    }

    // Index can be positional or via --index
    let index = match find_flag(args, "--index") {
        Some(i) => match parse_u32(i) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_nvdefine: invalid NV index: {i}");
                return 1;
            }
        },
        None => {
            let pos = positional_args(args);
            match pos.first() {
                Some(i) => match parse_u32(i) {
                    Some(v) => v,
                    None => {
                        eprintln!("tpm2_nvdefine: invalid NV index: {i}");
                        return 1;
                    }
                },
                None => {
                    eprintln!("tpm2_nvdefine: NV index is required");
                    return 1;
                }
            }
        }
    };

    let size = match find_flag(args, "--size") {
        Some(s) => match parse_usize(s) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_nvdefine: invalid size: {s}");
                return 1;
            }
        },
        None => 32,
    };

    let hash_alg = match find_flag(args, "--hash-algorithm") {
        Some(a) => match HashAlgorithm::from_str(a) {
            Some(alg) => alg,
            None => {
                eprintln!("tpm2_nvdefine: unknown hash algorithm: {a}");
                return 1;
            }
        },
        None => HashAlgorithm::Sha256,
    };

    let platform = match find_flag(args, "--hierarchy") {
        Some(h) => h.eq_ignore_ascii_case("platform"),
        None => false,
    };

    match ctx.nv_define(index, size, hash_alg, platform) {
        Ok(()) => {
            let attrs = if platform {
                NvAttributes::default_platform()
            } else {
                NvAttributes::default_owner()
            };
            println!("nv-index:");
            println!("  0x{index:08X}:");
            println!("    hash-alg:");
            println!("      value: {}", hash_alg.name());
            println!("      raw: 0x{:04X}", hash_alg.alg_id());
            println!("    attributes:");
            println!("      raw: 0x{:08X}", attrs.to_bits());
            println!("    size: {size}");
            0
        }
        Err(e) => {
            eprintln!("tpm2_nvdefine: {}", e.message());
            1
        }
    }
}

/// `tpm2_nvwrite` -- write data to an NV index.
fn cmd_nvwrite(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_nvwrite [OPTIONS] NV_INDEX");
        println!();
        println!("Write data to a Non-Volatile storage index.");
        println!();
        println!("Options:");
        println!("  --index INDEX     NV index (hex or decimal)");
        println!("  --input HEX       Data to write as hex string");
        println!("  --offset OFFSET   Write offset (default: 0)");
        println!("  --help, -h        Show this help");
        println!("  --version         Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_nvwrite version {VERSION}");
        return 0;
    }

    let index = match find_flag(args, "--index") {
        Some(i) => match parse_u32(i) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_nvwrite: invalid NV index: {i}");
                return 1;
            }
        },
        None => {
            let pos = positional_args(args);
            match pos.first() {
                Some(i) => match parse_u32(i) {
                    Some(v) => v,
                    None => {
                        eprintln!("tpm2_nvwrite: invalid NV index: {i}");
                        return 1;
                    }
                },
                None => {
                    eprintln!("tpm2_nvwrite: NV index is required");
                    return 1;
                }
            }
        }
    };

    let data_hex = match find_flag(args, "--input") {
        Some(d) => d.to_string(),
        None => {
            eprintln!("tpm2_nvwrite: --input is required");
            return 1;
        }
    };
    let data = match hex_decode(&data_hex) {
        Some(b) => b,
        None => {
            eprintln!("tpm2_nvwrite: invalid hex for --input");
            return 1;
        }
    };

    let offset = match find_flag(args, "--offset") {
        Some(o) => match parse_usize(o) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_nvwrite: invalid offset: {o}");
                return 1;
            }
        },
        None => 0,
    };

    match ctx.nv_write(index, offset, &data) {
        Ok(()) => {
            println!("NV index 0x{index:08X}: wrote {} bytes at offset {offset}.", data.len());
            0
        }
        Err(e) => {
            eprintln!("tpm2_nvwrite: {}", e.message());
            1
        }
    }
}

/// `tpm2_nvread` -- read data from an NV index.
fn cmd_nvread(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_nvread [OPTIONS] NV_INDEX");
        println!();
        println!("Read data from a Non-Volatile storage index.");
        println!();
        println!("Options:");
        println!("  --index INDEX     NV index (hex or decimal)");
        println!("  --size SIZE       Number of bytes to read (default: entire index)");
        println!("  --offset OFFSET   Read offset (default: 0)");
        println!("  --format FMT      Output format: hex, colon, dump (default: hex)");
        println!("  --help, -h        Show this help");
        println!("  --version         Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_nvread version {VERSION}");
        return 0;
    }

    let index = match find_flag(args, "--index") {
        Some(i) => match parse_u32(i) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_nvread: invalid NV index: {i}");
                return 1;
            }
        },
        None => {
            let pos = positional_args(args);
            match pos.first() {
                Some(i) => match parse_u32(i) {
                    Some(v) => v,
                    None => {
                        eprintln!("tpm2_nvread: invalid NV index: {i}");
                        return 1;
                    }
                },
                None => {
                    eprintln!("tpm2_nvread: NV index is required");
                    return 1;
                }
            }
        }
    };

    let offset = match find_flag(args, "--offset") {
        Some(o) => match parse_usize(o) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_nvread: invalid offset: {o}");
                return 1;
            }
        },
        None => 0,
    };

    // Determine size to read: from flag, or from NV definition
    let read_size = match find_flag(args, "--size") {
        Some(s) => match parse_usize(s) {
            Some(v) => v,
            None => {
                eprintln!("tpm2_nvread: invalid size: {s}");
                return 1;
            }
        },
        None => {
            match ctx.nv_find(index) {
                Some(nv) => {
                    nv.size.saturating_sub(offset)
                }
                None => {
                    eprintln!("tpm2_nvread: NV index 0x{index:08X} not found");
                    return 1;
                }
            }
        }
    };

    match ctx.nv_read(index, offset, read_size) {
        Ok(data) => {
            let format = find_flag(args, "--format").unwrap_or("hex");
            match format {
                "colon" => println!("{}", format_hex_colon(&data)),
                "dump" => println!("{}", format_hex_dump(&data)),
                _ => println!("{}", hex_encode(&data)),
            }
            0
        }
        Err(e) => {
            eprintln!("tpm2_nvread: {}", e.message());
            1
        }
    }
}

/// `tpm2_getcap` -- query TPM capabilities.
fn cmd_getcap(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_getcap [OPTIONS] CAPABILITY");
        println!();
        println!("Query TPM capabilities.");
        println!();
        println!("Capabilities:");
        println!("  algorithms    Supported hash algorithms");
        println!("  handles       Currently loaded handles");
        println!("  commands      Supported commands");
        println!("  properties    TPM properties");
        println!("  pcrs          PCR bank information");
        println!();
        println!("Options:");
        println!("  --capability CAP  Capability to query");
        println!("  --help, -h        Show this help");
        println!("  --version         Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_getcap version {VERSION}");
        return 0;
    }

    let cap_str = match find_flag(args, "--capability") {
        Some(c) => c.to_string(),
        None => {
            let pos = positional_args(args);
            match pos.first() {
                Some(c) => c.to_string(),
                None => {
                    eprintln!("tpm2_getcap: capability argument is required");
                    eprintln!("Available: algorithms, handles, commands, properties, pcrs");
                    return 1;
                }
            }
        }
    };

    let cap_type = match CapabilityType::from_str(&cap_str) {
        Some(ct) => ct,
        None => {
            eprintln!("tpm2_getcap: unknown capability: {cap_str}");
            eprintln!("Available: algorithms, handles, commands, properties, pcrs");
            return 1;
        }
    };

    println!("{}:", cap_type.name());
    for line in ctx.get_capability(cap_type) {
        println!("{line}");
    }

    0
}

/// `tpm2_selftest` -- run TPM self-test.
fn cmd_selftest(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_selftest [OPTIONS]");
        println!();
        println!("Run TPM self-test.");
        println!();
        println!("Options:");
        println!("  --full          Run full self-test (default: incremental)");
        println!("  --help, -h      Show this help");
        println!("  --version       Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_selftest version {VERSION}");
        return 0;
    }

    let full = has_flag(args, "--full");

    match ctx.self_test(full) {
        Ok(()) => {
            let test_type = if full { "full" } else { "incremental" };
            println!("self-test ({test_type}): PASSED");
            println!("status: {}", match ctx.self_test_state() {
                SelfTestState::NotRun => "not-run",
                SelfTestState::Passed => "passed",
                SelfTestState::_Failed => "failed",
            });
            0
        }
        Err(e) => {
            eprintln!("tpm2_selftest: {}", e.message());
            1
        }
    }
}

/// `tpm2_clear` -- clear the TPM.
fn cmd_clear(ctx: &mut TpmContext, args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!("Usage: tpm2_clear [OPTIONS]");
        println!();
        println!("Clear the TPM, removing all keys, NV indices, and resetting PCRs.");
        println!();
        println!("Options:");
        println!("  --hierarchy H   Hierarchy authorization (default: owner)");
        println!("  --help, -h      Show this help");
        println!("  --version       Show version");
        return 0;
    }
    if has_flag(args, "--version") {
        println!("tpm2_clear version {VERSION}");
        return 0;
    }

    let _hierarchy = match find_flag(args, "--hierarchy") {
        Some(h) => match Hierarchy::from_str(h) {
            Some(hier) => hier,
            None => {
                eprintln!("tpm2_clear: unknown hierarchy: {h}");
                return 1;
            }
        },
        None => Hierarchy::Owner,
    };

    match ctx.clear() {
        Ok(()) => {
            println!("TPM cleared successfully.");
            println!("All keys, NV indices, and PCR values have been reset.");
            0
        }
        Err(e) => {
            eprintln!("tpm2_clear: {}", e.message());
            1
        }
    }
}

// ---------------------------------------------------------------------------
// tpm2 dispatcher (main personality)
// ---------------------------------------------------------------------------

/// Show top-level help for the `tpm2` dispatcher.
fn tpm2_help() {
    println!("Usage: tpm2 COMMAND [OPTIONS]");
    println!();
    println!("TPM 2.0 management tool suite (simulated).");
    println!();
    println!("Commands:");
    println!("  getrandom         Get random bytes from TPM");
    println!("  pcrread           Read PCR registers");
    println!("  pcrextend         Extend a PCR register");
    println!("  createprimary     Create a primary key");
    println!("  create            Create a child key");
    println!("  load              Load key into TPM");
    println!("  sign              Sign data with TPM key");
    println!("  verifysignature   Verify a signature");
    println!("  nvdefine          Define NV index");
    println!("  nvwrite           Write to NV index");
    println!("  nvread            Read from NV index");
    println!("  getcap            Query TPM capabilities");
    println!("  selftest          Run TPM self-test");
    println!("  clear             Clear TPM");
    println!();
    println!("Options:");
    println!("  --help, -h        Show this help");
    println!("  --version         Show version");
    println!();
    println!("Use 'tpm2 COMMAND --help' for more information on a command.");
}

/// Dispatch a subcommand from the `tpm2` dispatcher personality.
fn tpm2_dispatch(ctx: &mut TpmContext, rest: Vec<String>) -> i32 {
    if rest.is_empty() || has_flag(&rest, "--help") || has_flag(&rest, "-h") {
        tpm2_help();
        return 0;
    }
    if has_flag(&rest, "--version") {
        println!("tpm2 version {VERSION}");
        return 0;
    }

    let cmd = rest.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = rest.into_iter().skip(1).collect();

    match cmd.as_str() {
        "getrandom" => cmd_getrandom(ctx, &cmd_args),
        "pcrread" => cmd_pcrread(ctx, &cmd_args),
        "pcrextend" => cmd_pcrextend(ctx, &cmd_args),
        "createprimary" => cmd_createprimary(ctx, &cmd_args),
        "create" => cmd_create(ctx, &cmd_args),
        "load" => cmd_load(ctx, &cmd_args),
        "sign" => cmd_sign(ctx, &cmd_args),
        "verifysignature" => cmd_verifysignature(ctx, &cmd_args),
        "nvdefine" => cmd_nvdefine(ctx, &cmd_args),
        "nvwrite" => cmd_nvwrite(ctx, &cmd_args),
        "nvread" => cmd_nvread(ctx, &cmd_args),
        "getcap" => cmd_getcap(ctx, &cmd_args),
        "selftest" => cmd_selftest(ctx, &cmd_args),
        "clear" => cmd_clear(ctx, &cmd_args),
        "help" => { tpm2_help(); 0 }
        other => {
            eprintln!("tpm2: unknown command: {other}");
            eprintln!("Run 'tpm2 --help' for available commands.");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();

    // Borrow-safe personality detection: extract basename from argv[0].
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("tpm2");
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

    let mut ctx = TpmContext::new();

    let exit_code = match prog_name.as_str() {
        "tpm2_getrandom" => cmd_getrandom(&mut ctx, &rest),
        "tpm2_pcrread" => cmd_pcrread(&mut ctx, &rest),
        "tpm2_pcrextend" => cmd_pcrextend(&mut ctx, &rest),
        "tpm2_createprimary" => cmd_createprimary(&mut ctx, &rest),
        "tpm2_create" => cmd_create(&mut ctx, &rest),
        "tpm2_load" => cmd_load(&mut ctx, &rest),
        "tpm2_sign" => cmd_sign(&mut ctx, &rest),
        "tpm2_verifysignature" => cmd_verifysignature(&mut ctx, &rest),
        "tpm2_nvdefine" => cmd_nvdefine(&mut ctx, &rest),
        "tpm2_nvwrite" => cmd_nvwrite(&mut ctx, &rest),
        "tpm2_nvread" => cmd_nvread(&mut ctx, &rest),
        "tpm2_getcap" => cmd_getcap(&mut ctx, &rest),
        "tpm2_selftest" => cmd_selftest(&mut ctx, &rest),
        "tpm2_clear" => cmd_clear(&mut ctx, &rest),
        _ => tpm2_dispatch(&mut ctx, rest),
    };

    process::exit(exit_code);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // === Hex encoding/decoding tests ===

    #[test]
    fn test_hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn test_hex_encode_single_byte() {
        assert_eq!(hex_encode(&[0x00]), "00");
        assert_eq!(hex_encode(&[0xFF]), "ff");
        assert_eq!(hex_encode(&[0xAB]), "ab");
    }

    #[test]
    fn test_hex_encode_multiple_bytes() {
        assert_eq!(hex_encode(&[0xDE, 0xAD, 0xBE, 0xEF]), "deadbeef");
    }

    #[test]
    fn test_hex_decode_empty() {
        assert_eq!(hex_decode(""), Some(vec![]));
    }

    #[test]
    fn test_hex_decode_valid() {
        assert_eq!(hex_decode("deadbeef"), Some(vec![0xDE, 0xAD, 0xBE, 0xEF]));
    }

    #[test]
    fn test_hex_decode_uppercase() {
        assert_eq!(hex_decode("DEADBEEF"), Some(vec![0xDE, 0xAD, 0xBE, 0xEF]));
    }

    #[test]
    fn test_hex_decode_mixed_case() {
        assert_eq!(hex_decode("DeAdBeEf"), Some(vec![0xDE, 0xAD, 0xBE, 0xEF]));
    }

    #[test]
    fn test_hex_decode_odd_length() {
        assert_eq!(hex_decode("abc"), None);
    }

    #[test]
    fn test_hex_decode_invalid_char() {
        assert_eq!(hex_decode("zz"), None);
    }

    #[test]
    fn test_hex_roundtrip() {
        let original = vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
        let encoded = hex_encode(&original);
        let decoded = hex_decode(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    // === PRNG tests ===

    #[test]
    fn test_prng_deterministic() {
        let mut p1 = SimPrng::new(42);
        let mut p2 = SimPrng::new(42);
        for _ in 0..100 {
            assert_eq!(p1.next_u64(), p2.next_u64());
        }
    }

    #[test]
    fn test_prng_different_seeds() {
        let mut p1 = SimPrng::new(1);
        let mut p2 = SimPrng::new(2);
        // Very unlikely to be equal
        assert_ne!(p1.next_u64(), p2.next_u64());
    }

    #[test]
    fn test_prng_zero_seed_uses_default() {
        let mut p = SimPrng::new(0);
        // Should not panic or produce all zeros
        let v = p.next_u64();
        assert_ne!(v, 0);
    }

    #[test]
    fn test_prng_fill_bytes() {
        let mut p = SimPrng::new(99);
        let mut buf = [0u8; 32];
        p.fill_bytes(&mut buf);
        // Not all zeros
        assert!(buf.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_prng_fill_bytes_empty() {
        let mut p = SimPrng::new(99);
        let mut buf = [0u8; 0];
        p.fill_bytes(&mut buf); // Should not panic
    }

    // === Hash simulation tests ===

    #[test]
    fn test_sim_sha1_deterministic() {
        let data = b"test data";
        let h1 = sim_sha1(data);
        let h2 = sim_sha1(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_sim_sha1_length() {
        let h = sim_sha1(b"hello");
        assert_eq!(h.len(), SHA1_DIGEST_LEN);
    }

    #[test]
    fn test_sim_sha256_deterministic() {
        let data = b"test data";
        let h1 = sim_sha256(data);
        let h2 = sim_sha256(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_sim_sha256_length() {
        let h = sim_sha256(b"hello");
        assert_eq!(h.len(), SHA256_DIGEST_LEN);
    }

    #[test]
    fn test_sim_sha384_deterministic() {
        let h1 = sim_sha384(b"abc");
        let h2 = sim_sha384(b"abc");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_sim_sha384_length() {
        let h = sim_sha384(b"hello");
        assert_eq!(h.len(), SHA384_DIGEST_LEN);
    }

    #[test]
    fn test_sim_sha512_deterministic() {
        let h1 = sim_sha512(b"abc");
        let h2 = sim_sha512(b"abc");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_sim_sha512_length() {
        let h = sim_sha512(b"hello");
        assert_eq!(h.len(), SHA512_DIGEST_LEN);
    }

    #[test]
    fn test_sim_hash_different_inputs_differ() {
        let h1 = sim_sha256(b"hello");
        let h2 = sim_sha256(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_sim_hash_empty_input() {
        let h = sim_sha256(b"");
        assert_eq!(h.len(), SHA256_DIGEST_LEN);
    }

    #[test]
    fn test_sim_hash_via_alg_enum() {
        let data = b"test";
        assert_eq!(sim_hash(HashAlgorithm::Sha1, data).len(), SHA1_DIGEST_LEN);
        assert_eq!(sim_hash(HashAlgorithm::Sha256, data).len(), SHA256_DIGEST_LEN);
        assert_eq!(sim_hash(HashAlgorithm::Sha384, data).len(), SHA384_DIGEST_LEN);
        assert_eq!(sim_hash(HashAlgorithm::Sha512, data).len(), SHA512_DIGEST_LEN);
    }

    // === HashAlgorithm tests ===

    #[test]
    fn test_hash_alg_name() {
        assert_eq!(HashAlgorithm::Sha1.name(), "sha1");
        assert_eq!(HashAlgorithm::Sha256.name(), "sha256");
        assert_eq!(HashAlgorithm::Sha384.name(), "sha384");
        assert_eq!(HashAlgorithm::Sha512.name(), "sha512");
    }

    #[test]
    fn test_hash_alg_id() {
        assert_eq!(HashAlgorithm::Sha1.alg_id(), 0x0004);
        assert_eq!(HashAlgorithm::Sha256.alg_id(), 0x000B);
        assert_eq!(HashAlgorithm::Sha384.alg_id(), 0x000C);
        assert_eq!(HashAlgorithm::Sha512.alg_id(), 0x000D);
    }

    #[test]
    fn test_hash_alg_digest_len() {
        assert_eq!(HashAlgorithm::Sha1.digest_len(), 20);
        assert_eq!(HashAlgorithm::Sha256.digest_len(), 32);
        assert_eq!(HashAlgorithm::Sha384.digest_len(), 48);
        assert_eq!(HashAlgorithm::Sha512.digest_len(), 64);
    }

    #[test]
    fn test_hash_alg_from_str_valid() {
        assert_eq!(HashAlgorithm::from_str("sha1"), Some(HashAlgorithm::Sha1));
        assert_eq!(HashAlgorithm::from_str("SHA256"), Some(HashAlgorithm::Sha256));
        assert_eq!(HashAlgorithm::from_str("Sha384"), Some(HashAlgorithm::Sha384));
        assert_eq!(HashAlgorithm::from_str("sha512"), Some(HashAlgorithm::Sha512));
    }

    #[test]
    fn test_hash_alg_from_str_invalid() {
        assert_eq!(HashAlgorithm::from_str("md5"), None);
        assert_eq!(HashAlgorithm::from_str(""), None);
    }

    #[test]
    fn test_hash_alg_all() {
        let all = HashAlgorithm::all();
        assert_eq!(all.len(), 4);
    }

    // === KeyType tests ===

    #[test]
    fn test_key_type_name() {
        assert_eq!(KeyType::Rsa2048.name(), "rsa2048");
        assert_eq!(KeyType::Rsa3072.name(), "rsa3072");
        assert_eq!(KeyType::Ecc256.name(), "ecc256");
        assert_eq!(KeyType::Ecc384.name(), "ecc384");
    }

    #[test]
    fn test_key_type_from_str() {
        assert_eq!(KeyType::from_str("rsa"), Some(KeyType::Rsa2048));
        assert_eq!(KeyType::from_str("rsa2048"), Some(KeyType::Rsa2048));
        assert_eq!(KeyType::from_str("rsa3072"), Some(KeyType::Rsa3072));
        assert_eq!(KeyType::from_str("ecc"), Some(KeyType::Ecc256));
        assert_eq!(KeyType::from_str("ecc256"), Some(KeyType::Ecc256));
        assert_eq!(KeyType::from_str("ecc384"), Some(KeyType::Ecc384));
        assert_eq!(KeyType::from_str("unknown"), None);
    }

    #[test]
    fn test_key_type_pub_key_size() {
        assert_eq!(KeyType::Rsa2048.pub_key_size(), 256);
        assert_eq!(KeyType::Rsa3072.pub_key_size(), 384);
        assert_eq!(KeyType::Ecc256.pub_key_size(), 64);
        assert_eq!(KeyType::Ecc384.pub_key_size(), 96);
    }

    #[test]
    fn test_key_type_priv_key_size() {
        assert_eq!(KeyType::Rsa2048.priv_key_size(), 128);
        assert_eq!(KeyType::Rsa3072.priv_key_size(), 192);
        assert_eq!(KeyType::Ecc256.priv_key_size(), 32);
        assert_eq!(KeyType::Ecc384.priv_key_size(), 48);
    }

    #[test]
    fn test_key_type_sig_size() {
        assert_eq!(KeyType::Rsa2048.sig_size(), 256);
        assert_eq!(KeyType::Rsa3072.sig_size(), 384);
        assert_eq!(KeyType::Ecc256.sig_size(), 64);
        assert_eq!(KeyType::Ecc384.sig_size(), 96);
    }

    // === Hierarchy tests ===

    #[test]
    fn test_hierarchy_name() {
        assert_eq!(Hierarchy::Owner.name(), "owner");
        assert_eq!(Hierarchy::Endorsement.name(), "endorsement");
        assert_eq!(Hierarchy::Platform.name(), "platform");
        assert_eq!(Hierarchy::Null.name(), "null");
    }

    #[test]
    fn test_hierarchy_from_str() {
        assert_eq!(Hierarchy::from_str("owner"), Some(Hierarchy::Owner));
        assert_eq!(Hierarchy::from_str("o"), Some(Hierarchy::Owner));
        assert_eq!(Hierarchy::from_str("endorsement"), Some(Hierarchy::Endorsement));
        assert_eq!(Hierarchy::from_str("e"), Some(Hierarchy::Endorsement));
        assert_eq!(Hierarchy::from_str("platform"), Some(Hierarchy::Platform));
        assert_eq!(Hierarchy::from_str("p"), Some(Hierarchy::Platform));
        assert_eq!(Hierarchy::from_str("null"), Some(Hierarchy::Null));
        assert_eq!(Hierarchy::from_str("n"), Some(Hierarchy::Null));
        assert_eq!(Hierarchy::from_str("0x40000001"), Some(Hierarchy::Owner));
    }

    #[test]
    fn test_hierarchy_from_str_invalid() {
        assert_eq!(Hierarchy::from_str("unknown"), None);
    }

    #[test]
    fn test_hierarchy_handle_value() {
        assert_eq!(Hierarchy::Owner.handle_value(), 0x4000_0001);
        assert_eq!(Hierarchy::Endorsement.handle_value(), 0x4000_000B);
        assert_eq!(Hierarchy::Platform.handle_value(), 0x4000_000C);
        assert_eq!(Hierarchy::Null.handle_value(), 0x4000_0007);
    }

    // === KeyAttributes tests ===

    #[test]
    fn test_key_attributes_default_primary() {
        let attrs = KeyAttributes::default_primary();
        assert!(attrs._fixed_tpm);
        assert!(attrs._fixed_parent);
        assert!(attrs._sensitive_data_origin);
        assert!(attrs._user_with_auth);
        assert!(!attrs._sign_encrypt);
        assert!(attrs._decrypt);
        assert!(attrs._restricted);
    }

    #[test]
    fn test_key_attributes_default_signing() {
        let attrs = KeyAttributes::default_signing();
        assert!(attrs._sign_encrypt);
        assert!(!attrs._decrypt);
        assert!(!attrs._restricted);
    }

    #[test]
    fn test_key_attributes_default_storage() {
        let attrs = KeyAttributes::default_storage();
        assert!(!attrs._sign_encrypt);
        assert!(attrs._decrypt);
        assert!(attrs._restricted);
    }

    #[test]
    fn test_key_attributes_to_bits() {
        let attrs = KeyAttributes::default_primary();
        let bits = attrs.to_bits();
        // Should have fixedTPM, fixedParent, sensitiveDO, userWithAuth, decrypt, restricted
        assert!(bits & (1 << 1) != 0); // fixedTPM
        assert!(bits & (1 << 4) != 0); // fixedParent
        assert!(bits & (1 << 5) != 0); // sensitiveDO
        assert!(bits & (1 << 6) != 0); // userWithAuth
        assert!(bits & (1 << 17) != 0); // decrypt
        assert!(bits & (1 << 16) != 0); // restricted
        assert!(bits & (1 << 18) == 0); // signEncrypt should be off
    }

    // === NvAttributes tests ===

    #[test]
    fn test_nv_attributes_default_owner() {
        let attrs = NvAttributes::default_owner();
        assert!(attrs._owner_write);
        assert!(attrs._owner_read);
        assert!(!attrs._platform_create);
    }

    #[test]
    fn test_nv_attributes_default_platform() {
        let attrs = NvAttributes::default_platform();
        assert!(!attrs._owner_write);
        assert!(attrs._owner_read);
        assert!(attrs._platform_create);
    }

    #[test]
    fn test_nv_attributes_to_bits() {
        let attrs = NvAttributes::default_owner();
        let bits = attrs.to_bits();
        assert!(bits & (1 << 0) != 0); // ownerWrite
        assert!(bits & (1 << 1) != 0); // ownerRead
    }

    // === PCR Bank tests ===

    #[test]
    fn test_pcr_bank_new_all_zeros() {
        let bank = PcrBank::new(HashAlgorithm::Sha256);
        for i in 0..PCR_COUNT {
            assert!(bank.is_zero(i));
            let val = bank.read(i).unwrap();
            assert_eq!(val.len(), SHA256_DIGEST_LEN);
            assert!(val.iter().all(|&b| b == 0));
        }
    }

    #[test]
    fn test_pcr_bank_read_out_of_bounds() {
        let bank = PcrBank::new(HashAlgorithm::Sha256);
        assert!(bank.read(PCR_COUNT).is_none());
        assert!(bank.read(100).is_none());
    }

    #[test]
    fn test_pcr_bank_extend() {
        let mut bank = PcrBank::new(HashAlgorithm::Sha256);
        let data = vec![0xAA; SHA256_DIGEST_LEN];
        bank.extend(0, &data).unwrap();
        assert!(!bank.is_zero(0));
        // Other PCRs should still be zero
        assert!(bank.is_zero(1));
    }

    #[test]
    fn test_pcr_bank_extend_invalid_index() {
        let mut bank = PcrBank::new(HashAlgorithm::Sha256);
        let data = vec![0xAA; SHA256_DIGEST_LEN];
        let result = bank.extend(PCR_COUNT, &data);
        assert!(result.is_err());
    }

    #[test]
    fn test_pcr_bank_extend_deterministic() {
        let mut bank1 = PcrBank::new(HashAlgorithm::Sha256);
        let mut bank2 = PcrBank::new(HashAlgorithm::Sha256);
        let data = vec![0xBB; SHA256_DIGEST_LEN];
        bank1.extend(5, &data).unwrap();
        bank2.extend(5, &data).unwrap();
        assert_eq!(bank1.read(5), bank2.read(5));
    }

    #[test]
    fn test_pcr_bank_reset() {
        let mut bank = PcrBank::new(HashAlgorithm::Sha256);
        let data = vec![0xCC; SHA256_DIGEST_LEN];
        bank.extend(3, &data).unwrap();
        assert!(!bank.is_zero(3));
        bank._reset(3).unwrap();
        assert!(bank.is_zero(3));
    }

    #[test]
    fn test_pcr_bank_reset_invalid_index() {
        let mut bank = PcrBank::new(HashAlgorithm::Sha256);
        let result = bank._reset(PCR_COUNT);
        assert!(result.is_err());
    }

    #[test]
    fn test_pcr_bank_sha1() {
        let bank = PcrBank::new(HashAlgorithm::Sha1);
        let val = bank.read(0).unwrap();
        assert_eq!(val.len(), SHA1_DIGEST_LEN);
    }

    #[test]
    fn test_pcr_bank_sha384() {
        let bank = PcrBank::new(HashAlgorithm::Sha384);
        let val = bank.read(0).unwrap();
        assert_eq!(val.len(), SHA384_DIGEST_LEN);
    }

    #[test]
    fn test_pcr_bank_sha512() {
        let bank = PcrBank::new(HashAlgorithm::Sha512);
        let val = bank.read(0).unwrap();
        assert_eq!(val.len(), SHA512_DIGEST_LEN);
    }

    // === NvIndex tests ===

    #[test]
    fn test_nv_index_new() {
        let nv = NvIndex::new(0x01000001, 64, HashAlgorithm::Sha256, false).unwrap();
        assert_eq!(nv.index, 0x01000001);
        assert_eq!(nv.size, 64);
        assert_eq!(nv.data.len(), 64);
    }

    #[test]
    fn test_nv_index_too_large() {
        let result = NvIndex::new(0x01000001, NV_MAX_SIZE + 1, HashAlgorithm::Sha256, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_nv_index_write_read() {
        let mut nv = NvIndex::new(0x01000001, 64, HashAlgorithm::Sha256, false).unwrap();
        let data = vec![0xAA, 0xBB, 0xCC, 0xDD];
        nv.write(0, &data).unwrap();
        let read = nv.read(0, 4).unwrap();
        assert_eq!(read, &data[..]);
    }

    #[test]
    fn test_nv_index_write_with_offset() {
        let mut nv = NvIndex::new(0x01000001, 64, HashAlgorithm::Sha256, false).unwrap();
        let data = vec![0x11, 0x22];
        nv.write(10, &data).unwrap();
        let read = nv.read(10, 2).unwrap();
        assert_eq!(read, &[0x11, 0x22]);
    }

    #[test]
    fn test_nv_index_write_out_of_bounds() {
        let mut nv = NvIndex::new(0x01000001, 8, HashAlgorithm::Sha256, false).unwrap();
        let data = vec![0xFF; 16]; // Too much data
        let result = nv.write(0, &data);
        assert!(result.is_err());
    }

    #[test]
    fn test_nv_index_read_out_of_bounds() {
        let nv = NvIndex::new(0x01000001, 8, HashAlgorithm::Sha256, false).unwrap();
        let result = nv.read(0, 16);
        assert!(result.is_err());
    }

    #[test]
    fn test_nv_index_written_flag() {
        let mut nv = NvIndex::new(0x01000001, 8, HashAlgorithm::Sha256, false).unwrap();
        assert!(!nv.attributes._written);
        nv.write(0, &[0x01]).unwrap();
        assert!(nv.attributes._written);
    }

    // === TpmContext basic tests ===

    #[test]
    fn test_tpm_context_new() {
        let ctx = TpmContext::new();
        assert_eq!(ctx.pcr_banks.len(), 4); // sha1, sha256, sha384, sha512
        assert!(ctx.nv_indices.is_empty());
        assert!(ctx.keys.is_empty());
        assert_eq!(ctx._self_test_state, SelfTestState::NotRun);
    }

    #[test]
    fn test_tpm_context_alloc_handle() {
        let mut ctx = TpmContext::new();
        let h1 = ctx.alloc_handle();
        let h2 = ctx.alloc_handle();
        assert_eq!(h1, 0x8000_0000);
        assert_eq!(h2, 0x8000_0001);
    }

    // === TpmContext PCR tests ===

    #[test]
    fn test_tpm_pcr_read_all_zeros() {
        let ctx = TpmContext::new();
        let val = ctx.pcr_read(HashAlgorithm::Sha256, 0).unwrap();
        assert!(val.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_tpm_pcr_extend_and_read() {
        let mut ctx = TpmContext::new();
        let data = vec![0x55; SHA256_DIGEST_LEN];
        ctx.pcr_extend(HashAlgorithm::Sha256, 7, &data).unwrap();
        let val = ctx.pcr_read(HashAlgorithm::Sha256, 7).unwrap();
        assert!(!val.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_tpm_pcr_read_all() {
        let ctx = TpmContext::new();
        let all = ctx.pcr_read_all(HashAlgorithm::Sha256).unwrap();
        assert_eq!(all.len(), PCR_COUNT);
    }

    #[test]
    fn test_tpm_pcr_unknown_alg_read() {
        // All 4 algorithms are registered, so this path exercises finding them
        let ctx = TpmContext::new();
        let val = ctx.pcr_read(HashAlgorithm::Sha384, 0).unwrap();
        assert_eq!(val.len(), SHA384_DIGEST_LEN);
    }

    // === TpmContext NV tests ===

    #[test]
    fn test_tpm_nv_define() {
        let mut ctx = TpmContext::new();
        ctx.nv_define(0x01000001, 32, HashAlgorithm::Sha256, false).unwrap();
        assert_eq!(ctx.nv_indices.len(), 1);
    }

    #[test]
    fn test_tpm_nv_define_duplicate() {
        let mut ctx = TpmContext::new();
        ctx.nv_define(0x01000001, 32, HashAlgorithm::Sha256, false).unwrap();
        let result = ctx.nv_define(0x01000001, 32, HashAlgorithm::Sha256, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_tpm_nv_define_too_many() {
        let mut ctx = TpmContext::new();
        for i in 0..NV_MAX_INDICES {
            ctx.nv_define(0x01000000 + i as u32, 8, HashAlgorithm::Sha256, false).unwrap();
        }
        let result = ctx.nv_define(0x02000000, 8, HashAlgorithm::Sha256, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_tpm_nv_write_read() {
        let mut ctx = TpmContext::new();
        ctx.nv_define(0x01000001, 32, HashAlgorithm::Sha256, false).unwrap();
        ctx.nv_write(0x01000001, 0, &[0xAA, 0xBB]).unwrap();
        let data = ctx.nv_read(0x01000001, 0, 2).unwrap();
        assert_eq!(data, vec![0xAA, 0xBB]);
    }

    #[test]
    fn test_tpm_nv_write_not_found() {
        let mut ctx = TpmContext::new();
        let result = ctx.nv_write(0xDEAD, 0, &[0x01]);
        assert!(result.is_err());
    }

    #[test]
    fn test_tpm_nv_read_not_found() {
        let ctx = TpmContext::new();
        let result = ctx.nv_read(0xDEAD, 0, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_tpm_nv_find() {
        let mut ctx = TpmContext::new();
        ctx.nv_define(0x01000001, 32, HashAlgorithm::Sha256, false).unwrap();
        assert!(ctx.nv_find(0x01000001).is_some());
        assert!(ctx.nv_find(0x01000002).is_none());
    }

    #[test]
    fn test_tpm_nv_undefine() {
        let mut ctx = TpmContext::new();
        ctx.nv_define(0x01000001, 32, HashAlgorithm::Sha256, false).unwrap();
        ctx._nv_undefine(0x01000001).unwrap();
        assert!(ctx.nv_find(0x01000001).is_none());
    }

    #[test]
    fn test_tpm_nv_undefine_not_found() {
        let mut ctx = TpmContext::new();
        let result = ctx._nv_undefine(0xDEAD);
        assert!(result.is_err());
    }

    // === TpmContext key tests ===

    #[test]
    fn test_tpm_create_primary() {
        let mut ctx = TpmContext::new();
        let handle = ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        assert!(handle >= 0x8000_0000);
        assert_eq!(ctx.keys.len(), 1);
    }

    #[test]
    fn test_tpm_create_primary_different_types() {
        let mut ctx = TpmContext::new();
        ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        ctx.create_primary(KeyType::Ecc256, Hierarchy::Endorsement, HashAlgorithm::Sha384).unwrap();
        ctx.create_primary(KeyType::Rsa3072, Hierarchy::Platform, HashAlgorithm::Sha512).unwrap();
        assert_eq!(ctx.keys.len(), 3);
    }

    #[test]
    fn test_tpm_create_primary_too_many() {
        let mut ctx = TpmContext::new();
        for _ in 0..KEY_MAX_OBJECTS {
            ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        }
        let result = ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256);
        assert!(result.is_err());
    }

    #[test]
    fn test_tpm_create_child() {
        let mut ctx = TpmContext::new();
        let parent = ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        let (pub_key, priv_key) = ctx.create_child(parent, KeyType::Rsa2048, HashAlgorithm::Sha256, "sign").unwrap();
        assert!(!pub_key.is_empty());
        assert!(!priv_key.is_empty());
    }

    #[test]
    fn test_tpm_create_child_no_parent() {
        let mut ctx = TpmContext::new();
        let result = ctx.create_child(0xDEAD, KeyType::Rsa2048, HashAlgorithm::Sha256, "sign");
        assert!(result.is_err());
    }

    #[test]
    fn test_tpm_load_key() {
        let mut ctx = TpmContext::new();
        let parent = ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        let pub_key = vec![0xAA; 256];
        let priv_key = vec![0xBB; 128];
        let handle = ctx.load_key(parent, &pub_key, &priv_key, KeyType::Rsa2048, HashAlgorithm::Sha256).unwrap();
        assert!(handle >= 0x8000_0000);
    }

    #[test]
    fn test_tpm_load_key_no_parent() {
        let mut ctx = TpmContext::new();
        let pub_key = vec![0xAA; 256];
        let priv_key = vec![0xBB; 128];
        let result = ctx.load_key(0xDEAD, &pub_key, &priv_key, KeyType::Rsa2048, HashAlgorithm::Sha256);
        assert!(result.is_err());
    }

    #[test]
    fn test_tpm_find_key() {
        let mut ctx = TpmContext::new();
        let handle = ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        assert!(ctx.find_key(handle).is_some());
        assert!(ctx.find_key(0xDEAD).is_none());
    }

    #[test]
    fn test_tpm_flush_key() {
        let mut ctx = TpmContext::new();
        let handle = ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        ctx._flush_key(handle).unwrap();
        assert!(ctx.find_key(handle).is_none());
    }

    #[test]
    fn test_tpm_flush_key_not_found() {
        let mut ctx = TpmContext::new();
        let result = ctx._flush_key(0xDEAD);
        assert!(result.is_err());
    }

    // === Sign/Verify tests ===

    #[test]
    fn test_tpm_sign_verify_rsa() {
        let mut ctx = TpmContext::new();
        let handle = ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        let data = b"test message";
        let sig = ctx.sign(handle, data).unwrap();
        assert_eq!(sig.len(), KeyType::Rsa2048.sig_size());
        let valid = ctx.verify(handle, data, &sig).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_tpm_sign_verify_ecc() {
        let mut ctx = TpmContext::new();
        let handle = ctx.create_primary(KeyType::Ecc256, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        let data = b"test message ecc";
        let sig = ctx.sign(handle, data).unwrap();
        assert_eq!(sig.len(), KeyType::Ecc256.sig_size());
        let valid = ctx.verify(handle, data, &sig).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_tpm_verify_wrong_data() {
        let mut ctx = TpmContext::new();
        let handle = ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        let data = b"original message";
        let sig = ctx.sign(handle, data).unwrap();
        let valid = ctx.verify(handle, b"tampered message", &sig).unwrap();
        assert!(!valid);
    }

    #[test]
    fn test_tpm_verify_wrong_sig() {
        let mut ctx = TpmContext::new();
        let handle = ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        let data = b"test message";
        let wrong_sig = vec![0xFF; KeyType::Rsa2048.sig_size()];
        let valid = ctx.verify(handle, data, &wrong_sig).unwrap();
        assert!(!valid);
    }

    #[test]
    fn test_tpm_sign_not_found() {
        let ctx = TpmContext::new();
        let result = ctx.sign(0xDEAD, b"data");
        assert!(result.is_err());
    }

    #[test]
    fn test_tpm_verify_not_found() {
        let ctx = TpmContext::new();
        let result = ctx.verify(0xDEAD, b"data", &[0x00; 256]);
        assert!(result.is_err());
    }

    // === Self-test tests ===

    #[test]
    fn test_tpm_selftest_incremental() {
        let mut ctx = TpmContext::new();
        assert_eq!(ctx.self_test_state(), SelfTestState::NotRun);
        ctx.self_test(false).unwrap();
        assert_eq!(ctx.self_test_state(), SelfTestState::Passed);
    }

    #[test]
    fn test_tpm_selftest_full() {
        let mut ctx = TpmContext::new();
        ctx.self_test(true).unwrap();
        assert_eq!(ctx.self_test_state(), SelfTestState::Passed);
    }

    // === Get random tests ===

    #[test]
    fn test_tpm_get_random_length() {
        let mut ctx = TpmContext::new();
        let r = ctx.get_random(16);
        assert_eq!(r.len(), 16);
    }

    #[test]
    fn test_tpm_get_random_zero() {
        let mut ctx = TpmContext::new();
        let r = ctx.get_random(0);
        assert!(r.is_empty());
    }

    #[test]
    fn test_tpm_get_random_deterministic() {
        let mut ctx1 = TpmContext::new();
        let mut ctx2 = TpmContext::new();
        let r1 = ctx1.get_random(32);
        let r2 = ctx2.get_random(32);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_tpm_get_random_successive_differ() {
        let mut ctx = TpmContext::new();
        let r1 = ctx.get_random(32);
        let r2 = ctx.get_random(32);
        assert_ne!(r1, r2);
    }

    // === Clear tests ===

    #[test]
    fn test_tpm_clear() {
        let mut ctx = TpmContext::new();
        // Set up some state
        ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        ctx.nv_define(0x01000001, 32, HashAlgorithm::Sha256, false).unwrap();
        ctx.pcr_extend(HashAlgorithm::Sha256, 0, &[0xAA; 32]).unwrap();
        ctx.self_test(true).unwrap();

        ctx.clear().unwrap();

        assert!(ctx.keys.is_empty());
        assert!(ctx.nv_indices.is_empty());
        assert_eq!(ctx._self_test_state, SelfTestState::NotRun);
        // PCRs should be zero again
        let val = ctx.pcr_read(HashAlgorithm::Sha256, 0).unwrap();
        assert!(val.iter().all(|&b| b == 0));
    }

    // === Capability tests ===

    #[test]
    fn test_tpm_getcap_algorithms() {
        let ctx = TpmContext::new();
        let lines = ctx.get_capability(CapabilityType::Algorithms);
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_tpm_getcap_commands() {
        let ctx = TpmContext::new();
        let lines = ctx.get_capability(CapabilityType::Commands);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_tpm_getcap_properties() {
        let ctx = TpmContext::new();
        let lines = ctx.get_capability(CapabilityType::Properties);
        assert!(!lines.is_empty());
        // Should contain manufacturer
        assert!(lines.iter().any(|l| l.contains(TPM_MANUFACTURER)));
    }

    #[test]
    fn test_tpm_getcap_pcr_banks() {
        let ctx = TpmContext::new();
        let lines = ctx.get_capability(CapabilityType::PcrBanks);
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_tpm_getcap_handles_empty() {
        let ctx = TpmContext::new();
        let lines = ctx.get_capability(CapabilityType::Handles);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_tpm_getcap_handles_with_keys() {
        let mut ctx = TpmContext::new();
        ctx.create_primary(KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256).unwrap();
        let lines = ctx.get_capability(CapabilityType::Handles);
        assert_eq!(lines.len(), 1);
    }

    // === CapabilityType tests ===

    #[test]
    fn test_capability_type_from_str() {
        assert_eq!(CapabilityType::from_str("algorithms"), Some(CapabilityType::Algorithms));
        assert_eq!(CapabilityType::from_str("algs"), Some(CapabilityType::Algorithms));
        assert_eq!(CapabilityType::from_str("handles"), Some(CapabilityType::Handles));
        assert_eq!(CapabilityType::from_str("commands"), Some(CapabilityType::Commands));
        assert_eq!(CapabilityType::from_str("cmds"), Some(CapabilityType::Commands));
        assert_eq!(CapabilityType::from_str("properties"), Some(CapabilityType::Properties));
        assert_eq!(CapabilityType::from_str("props"), Some(CapabilityType::Properties));
        assert_eq!(CapabilityType::from_str("pcrs"), Some(CapabilityType::PcrBanks));
        assert_eq!(CapabilityType::from_str("unknown"), None);
    }

    #[test]
    fn test_capability_type_name() {
        assert_eq!(CapabilityType::Algorithms.name(), "algorithms");
        assert_eq!(CapabilityType::Handles.name(), "handles");
        assert_eq!(CapabilityType::Commands.name(), "commands");
        assert_eq!(CapabilityType::Properties.name(), "properties");
        assert_eq!(CapabilityType::PcrBanks.name(), "pcrs");
    }

    // === Argument parsing tests ===

    #[test]
    fn test_parse_u32_decimal() {
        assert_eq!(parse_u32("42"), Some(42));
        assert_eq!(parse_u32("0"), Some(0));
    }

    #[test]
    fn test_parse_u32_hex() {
        assert_eq!(parse_u32("0xFF"), Some(255));
        assert_eq!(parse_u32("0x80000000"), Some(0x80000000));
    }

    #[test]
    fn test_parse_u32_invalid() {
        assert_eq!(parse_u32("abc"), None);
        assert_eq!(parse_u32(""), None);
    }

    #[test]
    fn test_parse_usize_decimal() {
        assert_eq!(parse_usize("100"), Some(100));
    }

    #[test]
    fn test_parse_usize_hex() {
        assert_eq!(parse_usize("0x100"), Some(256));
    }

    #[test]
    fn test_parse_usize_invalid() {
        assert_eq!(parse_usize("xyz"), None);
    }

    #[test]
    fn test_find_flag_separate() {
        let args: Vec<String> = vec!["--size".into(), "64".into(), "--other".into()];
        assert_eq!(find_flag(&args, "--size"), Some("64"));
    }

    #[test]
    fn test_find_flag_equals() {
        let args: Vec<String> = vec!["--size=64".into(), "--other".into()];
        assert_eq!(find_flag(&args, "--size"), Some("64"));
    }

    #[test]
    fn test_find_flag_missing() {
        let args: Vec<String> = vec!["--other".into()];
        assert_eq!(find_flag(&args, "--size"), None);
    }

    #[test]
    fn test_has_flag() {
        let args: Vec<String> = vec!["--help".into(), "arg".into()];
        assert!(has_flag(&args, "--help"));
        assert!(!has_flag(&args, "--version"));
    }

    #[test]
    fn test_positional_args() {
        let args: Vec<String> = vec![
            "--size".into(), "64".into(), "myarg".into(), "--help".into(), "other".into(),
        ];
        let pos = positional_args(&args);
        assert_eq!(pos, vec!["myarg", "other"]);
    }

    // === Formatting tests ===

    #[test]
    fn test_format_hex_colon() {
        assert_eq!(format_hex_colon(&[0xAA, 0xBB, 0xCC]), "aa:bb:cc");
    }

    #[test]
    fn test_format_hex_colon_empty() {
        assert_eq!(format_hex_colon(&[]), "");
    }

    #[test]
    fn test_format_hex_dump_single_line() {
        let bytes = vec![0x41, 0x42, 0x43]; // "ABC"
        let dump = format_hex_dump(&bytes);
        assert!(dump.contains("41 42 43"));
        assert!(dump.contains("ABC"));
    }

    #[test]
    fn test_format_hex_dump_multi_line() {
        let bytes: Vec<u8> = (0..32).collect();
        let dump = format_hex_dump(&bytes);
        let lines: Vec<&str> = dump.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_format_hex_dump_non_printable() {
        let bytes = vec![0x00, 0x01, 0x7F, 0xFF];
        let dump = format_hex_dump(&bytes);
        assert!(dump.contains("...."));
    }

    // === TpmError tests ===

    #[test]
    fn test_tpm_error_messages() {
        let e = TpmError::InvalidPcrIndex(99);
        assert!(e.message().contains("99"));

        let e = TpmError::UnknownAlgorithm("md5".into());
        assert!(e.message().contains("md5"));

        let e = TpmError::NvIndexExists(0x01000001);
        assert!(e.message().contains("01000001"));

        let e = TpmError::NvIndexNotFound(0x01000002);
        assert!(e.message().contains("01000002"));

        let e = TpmError::NvSizeTooLarge(9999);
        assert!(e.message().contains("9999"));

        let e = TpmError::NvWriteLocked(0x01000001);
        assert!(e.message().contains("write-locked"));

        let e = TpmError::NvReadLocked(0x01000001);
        assert!(e.message().contains("read-locked"));

        let e = TpmError::NvTooManyIndices;
        assert!(e.message().contains("too many"));

        let e = TpmError::KeyNotFound(0x80000000);
        assert!(e.message().contains("80000000"));

        let e = TpmError::TooManyKeys;
        assert!(e.message().contains("too many"));

        let e = TpmError::SelfTestFailed("hash mismatch".into());
        assert!(e.message().contains("hash mismatch"));

        let e = TpmError::_InvalidHandle(0);
        assert!(e.message().contains("invalid handle"));

        let e = TpmError::_InvalidArgument("bad".into());
        assert!(e.message().contains("bad"));

        let e = TpmError::_NotInitialized;
        assert!(e.message().contains("not initialized"));

        let e = TpmError::_AlreadyCleared;
        assert!(e.message().contains("already cleared"));
    }

    // === NvWriteOutOfBounds / NvReadOutOfBounds error messages ===

    #[test]
    fn test_nv_write_out_of_bounds_message() {
        let e = TpmError::NvWriteOutOfBounds {
            index: 0x01000001,
            offset: 10,
            len: 50,
            size: 32,
        };
        let msg = e.message();
        assert!(msg.contains("01000001"));
        assert!(msg.contains("10"));
        assert!(msg.contains("50"));
        assert!(msg.contains("32"));
    }

    #[test]
    fn test_nv_read_out_of_bounds_message() {
        let e = TpmError::NvReadOutOfBounds {
            index: 0x01000002,
            offset: 5,
            len: 40,
            size: 16,
        };
        let msg = e.message();
        assert!(msg.contains("01000002"));
        assert!(msg.contains("40"));
    }

    // === SelfTestState tests ===

    #[test]
    fn test_self_test_state_eq() {
        assert_eq!(SelfTestState::NotRun, SelfTestState::NotRun);
        assert_eq!(SelfTestState::Passed, SelfTestState::Passed);
        assert_eq!(SelfTestState::_Failed, SelfTestState::_Failed);
        assert_ne!(SelfTestState::NotRun, SelfTestState::Passed);
    }

    // === KeyObject sign/verify tests ===

    #[test]
    fn test_key_object_sign_deterministic() {
        let mut prng = SimPrng::new(42);
        let key = KeyObject::new_primary(
            0x80000000, KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256, &mut prng,
        );
        let sig1 = key.sign(b"hello");
        let sig2 = key.sign(b"hello");
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_key_object_sign_different_messages() {
        let mut prng = SimPrng::new(42);
        let key = KeyObject::new_primary(
            0x80000000, KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256, &mut prng,
        );
        let sig1 = key.sign(b"hello");
        let sig2 = key.sign(b"world");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_key_object_verify_correct() {
        let mut prng = SimPrng::new(42);
        let key = KeyObject::new_primary(
            0x80000000, KeyType::Ecc256, Hierarchy::Owner, HashAlgorithm::Sha256, &mut prng,
        );
        let sig = key.sign(b"test data");
        assert!(key.verify(b"test data", &sig));
    }

    #[test]
    fn test_key_object_verify_tampered() {
        let mut prng = SimPrng::new(42);
        let key = KeyObject::new_primary(
            0x80000000, KeyType::Ecc256, Hierarchy::Owner, HashAlgorithm::Sha256, &mut prng,
        );
        let sig = key.sign(b"test data");
        assert!(!key.verify(b"tampered data", &sig));
    }

    #[test]
    fn test_key_object_child_sign_verify() {
        let mut prng = SimPrng::new(99);
        let key = KeyObject::new_child(
            0x80000001, KeyType::Rsa3072, Hierarchy::Owner,
            HashAlgorithm::Sha384, 0x80000000, "sign", &mut prng,
        );
        let data = b"child key test";
        let sig = key.sign(data);
        assert_eq!(sig.len(), KeyType::Rsa3072.sig_size());
        assert!(key.verify(data, &sig));
    }

    #[test]
    fn test_key_object_storage_usage() {
        let mut prng = SimPrng::new(99);
        let key = KeyObject::new_child(
            0x80000001, KeyType::Rsa2048, Hierarchy::Owner,
            HashAlgorithm::Sha256, 0x80000000, "storage", &mut prng,
        );
        assert_eq!(key._attributes._decrypt, true);
        assert_eq!(key._attributes._sign_encrypt, false);
    }

    // === PCR extend chain test ===

    #[test]
    fn test_pcr_extend_chain() {
        let mut ctx = TpmContext::new();
        let d1 = vec![0x11; SHA256_DIGEST_LEN];
        let d2 = vec![0x22; SHA256_DIGEST_LEN];
        let d3 = vec![0x33; SHA256_DIGEST_LEN];

        ctx.pcr_extend(HashAlgorithm::Sha256, 0, &d1).unwrap();
        let v1 = ctx.pcr_read(HashAlgorithm::Sha256, 0).unwrap();

        ctx.pcr_extend(HashAlgorithm::Sha256, 0, &d2).unwrap();
        let v2 = ctx.pcr_read(HashAlgorithm::Sha256, 0).unwrap();

        ctx.pcr_extend(HashAlgorithm::Sha256, 0, &d3).unwrap();
        let v3 = ctx.pcr_read(HashAlgorithm::Sha256, 0).unwrap();

        // Each extension should produce a different value
        assert_ne!(v1, v2);
        assert_ne!(v2, v3);
        assert_ne!(v1, v3);
    }

    // === Multiple NV indices test ===

    #[test]
    fn test_multiple_nv_indices() {
        let mut ctx = TpmContext::new();
        ctx.nv_define(0x01000001, 16, HashAlgorithm::Sha256, false).unwrap();
        ctx.nv_define(0x01000002, 32, HashAlgorithm::Sha256, false).unwrap();
        ctx.nv_define(0x01000003, 64, HashAlgorithm::Sha256, true).unwrap();

        ctx.nv_write(0x01000001, 0, &[0xAA; 16]).unwrap();
        ctx.nv_write(0x01000002, 0, &[0xBB; 32]).unwrap();
        ctx.nv_write(0x01000003, 0, &[0xCC; 64]).unwrap();

        let d1 = ctx.nv_read(0x01000001, 0, 16).unwrap();
        let d2 = ctx.nv_read(0x01000002, 0, 32).unwrap();
        let d3 = ctx.nv_read(0x01000003, 0, 64).unwrap();

        assert!(d1.iter().all(|&b| b == 0xAA));
        assert!(d2.iter().all(|&b| b == 0xBB));
        assert!(d3.iter().all(|&b| b == 0xCC));
    }

    // === PCR bank isolation test ===

    #[test]
    fn test_pcr_banks_isolated() {
        let mut ctx = TpmContext::new();
        let sha256_data = vec![0xAA; SHA256_DIGEST_LEN];
        ctx.pcr_extend(HashAlgorithm::Sha256, 5, &sha256_data).unwrap();

        // SHA-1 bank PCR 5 should still be zero
        let sha1_val = ctx.pcr_read(HashAlgorithm::Sha1, 5).unwrap();
        assert!(sha1_val.iter().all(|&b| b == 0));

        // SHA-256 bank PCR 5 should not be zero
        let sha256_val = ctx.pcr_read(HashAlgorithm::Sha256, 5).unwrap();
        assert!(!sha256_val.iter().all(|&b| b == 0));
    }

    // === Full workflow integration test ===

    #[test]
    fn test_full_workflow() {
        let mut ctx = TpmContext::new();

        // 1. Self-test
        ctx.self_test(true).unwrap();
        assert_eq!(ctx.self_test_state(), SelfTestState::Passed);

        // 2. Get some random bytes
        let random = ctx.get_random(32);
        assert_eq!(random.len(), 32);

        // 3. Create a primary key
        let primary = ctx.create_primary(
            KeyType::Rsa2048, Hierarchy::Owner, HashAlgorithm::Sha256
        ).unwrap();

        // 4. Create a child signing key
        let (_pub_key, _priv_key) = ctx.create_child(
            primary, KeyType::Ecc256, HashAlgorithm::Sha256, "sign"
        ).unwrap();

        // 5. Sign and verify
        let child_handle = ctx.keys.last().unwrap().handle;
        let message = b"important data";
        let sig = ctx.sign(child_handle, message).unwrap();
        let valid = ctx.verify(child_handle, message, &sig).unwrap();
        assert!(valid);

        // 6. Extend a PCR
        let extend_data = sim_hash(HashAlgorithm::Sha256, message);
        ctx.pcr_extend(HashAlgorithm::Sha256, 0, &extend_data).unwrap();
        let pcr_val = ctx.pcr_read(HashAlgorithm::Sha256, 0).unwrap();
        assert!(!pcr_val.iter().all(|&b| b == 0));

        // 7. NV storage
        ctx.nv_define(0x01500001, 64, HashAlgorithm::Sha256, false).unwrap();
        ctx.nv_write(0x01500001, 0, &sig[..64]).unwrap();
        let nv_data = ctx.nv_read(0x01500001, 0, 64).unwrap();
        assert_eq!(&nv_data[..], &sig[..64]);

        // 8. Clear
        ctx.clear().unwrap();
        assert!(ctx.keys.is_empty());
        assert!(ctx.nv_indices.is_empty());
    }
}
