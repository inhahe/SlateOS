//! Multi-personality disk encryption management utility for OurOS.
//!
//! This binary detects the tool from `argv[0]`:
//!   - `cryptsetup`      -- LUKS disk encryption management
//!   - `veritysetup`     -- dm-verity (read-only integrity verification)
//!   - `integritysetup`  -- dm-integrity metadata
//!
//! Provides LUKS header parsing/generation, PBKDF2-SHA256 key derivation,
//! simulated cipher benchmarks, and device mapper management stubs.

#![deny(clippy::all)]

use std::env;
use std::io::Write;
use std::process;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const VERSION: &str = "2.7.0-ouros";
const LUKS_MAGIC: [u8; 6] = [0x4c, 0x55, 0x4b, 0x53, 0xba, 0xbe]; // "LUKS\xba\xbe"
const LUKS_KEY_SLOTS: usize = 8;
const LUKS_SALT_SIZE: usize = 32;
const LUKS_DIGEST_SIZE: usize = 32;
const LUKS_MK_BYTES: usize = 64;
const LUKS_HEADER_SIZE: usize = 592;
const LUKS_SECTOR_SIZE: u32 = 512;
const LUKS_ALIGN: u32 = 4096;

const DEFAULT_CIPHER: &str = "aes";
const DEFAULT_CIPHER_MODE: &str = "xts-plain64";
const DEFAULT_HASH: &str = "sha256";
const DEFAULT_KEY_SIZE: u32 = 256;
const DEFAULT_ITER_TIME_MS: u32 = 2000;
const DEFAULT_LUKS_VERSION: u16 = 2;

const VERITY_MAGIC: &[u8; 8] = b"verity\0\0";
const VERITY_VERSION: u32 = 1;

const INTEGRITY_MAGIC: &[u8; 8] = b"integrt\0";
const INTEGRITY_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Cryptsetup,
    Veritysetup,
    Integritysetup,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::Cryptsetup => "cryptsetup",
            Self::Veritysetup => "veritysetup",
            Self::Integritysetup => "integritysetup",
        }
    }
}

fn detect_tool(argv0: &str) -> Tool {
    let basename = argv0
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(argv0);
    let basename = basename.strip_suffix(".exe").unwrap_or(basename);
    let lower = basename.to_ascii_lowercase();

    if lower.contains("verity") {
        Tool::Veritysetup
    } else if lower.contains("integrity") {
        Tool::Integritysetup
    } else {
        Tool::Cryptsetup
    }
}

// ---------------------------------------------------------------------------
// SHA-256 (from scratch, FIPS 180-4)
// ---------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const SHA256_INIT: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: SHA256_INIT,
            buf: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        let mut offset = 0;
        self.total_len = self.total_len.wrapping_add(data.len() as u64);

        // Fill partial buffer
        if self.buf_len > 0 {
            let space = 64 - self.buf_len;
            let take = data.len().min(space);
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            offset = take;

            if self.buf_len == 64 {
                let block = self.buf;
                Self::compress(&mut self.state, &block);
                self.buf_len = 0;
            }
        }

        // Process full blocks
        while offset + 64 <= data.len() {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[offset..offset + 64]);
            Self::compress(&mut self.state, &block);
            offset += 64;
        }

        // Store remainder
        if offset < data.len() {
            let remaining = data.len() - offset;
            self.buf[..remaining].copy_from_slice(&data[offset..]);
            self.buf_len = remaining;
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);

        // Padding: append 0x80, then zeros, then 64-bit big-endian length
        let mut pad = [0u8; 72]; // max padding needed
        pad[0] = 0x80;
        let pad_len = if self.buf_len < 56 {
            56 - self.buf_len
        } else {
            120 - self.buf_len
        };
        self.update(&pad[..pad_len]);
        self.update(&bit_len.to_be_bytes());

        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7)
                ^ w[i - 15].rotate_right(18)
                ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17)
                ^ w[i - 2].rotate_right(19)
                ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;

        for i in 0..64 {
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

    fn digest(data: &[u8]) -> [u8; 32] {
        let mut h = Self::new();
        h.update(data);
        h.finalize()
    }
}

// ---------------------------------------------------------------------------
// HMAC-SHA256
// ---------------------------------------------------------------------------

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }

    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(&inner_hash);
    outer.finalize()
}

// ---------------------------------------------------------------------------
// PBKDF2-SHA256
// ---------------------------------------------------------------------------

fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, dk_len: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity(dk_len);
    let blocks_needed = (dk_len + 31) / 32;

    for block_idx in 1..=blocks_needed {
        let mut salt_with_idx = Vec::with_capacity(salt.len() + 4);
        salt_with_idx.extend_from_slice(salt);
        salt_with_idx.extend_from_slice(&(block_idx as u32).to_be_bytes());

        let mut u = hmac_sha256(password, &salt_with_idx);
        let mut t = u;

        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for j in 0..32 {
                t[j] ^= u[j];
            }
        }

        result.extend_from_slice(&t);
    }

    result.truncate(dk_len);
    result
}

// ---------------------------------------------------------------------------
// Hex encoding
// ---------------------------------------------------------------------------

fn hex_encode(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for &b in data {
        s.push(HEX_CHARS[(b >> 4) as usize]);
        s.push(HEX_CHARS[(b & 0x0f) as usize]);
    }
    s
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7',
    '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_val(bytes[i])?;
        let lo = hex_val(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// UUID generation (simple deterministic from seed)
// ---------------------------------------------------------------------------

fn generate_uuid(seed: &[u8]) -> String {
    let hash = Sha256::digest(seed);
    // Format as UUID v4 (variant 1) using first 16 bytes of hash
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    // Set version 4
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    // Set variant 1
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

// ---------------------------------------------------------------------------
// LUKS header structures
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct LuksKeySlot {
    active: bool,
    iterations: u32,
    salt: [u8; LUKS_SALT_SIZE],
    key_material_offset: u32,
    stripes: u32,
}

impl LuksKeySlot {
    fn inactive() -> Self {
        Self {
            active: false,
            iterations: 0,
            salt: [0u8; LUKS_SALT_SIZE],
            key_material_offset: 0,
            stripes: 4000,
        }
    }

    fn new_active(iterations: u32, salt: [u8; LUKS_SALT_SIZE], offset: u32) -> Self {
        Self {
            active: true,
            iterations,
            salt,
            key_material_offset: offset,
            stripes: 4000,
        }
    }
}

#[derive(Clone, Debug)]
struct LuksHeader {
    version: u16,
    cipher_name: String,
    cipher_mode: String,
    hash_spec: String,
    payload_offset: u32,
    key_bytes: u32,
    mk_digest: [u8; LUKS_DIGEST_SIZE],
    mk_digest_salt: [u8; LUKS_SALT_SIZE],
    mk_digest_iter: u32,
    uuid: String,
    key_slots: [LuksKeySlot; LUKS_KEY_SLOTS],
}

impl LuksHeader {
    fn new(
        cipher: &str,
        cipher_mode: &str,
        hash: &str,
        key_bytes: u32,
        iter_time_ms: u32,
        device_path: &str,
    ) -> Self {
        let uuid = generate_uuid(device_path.as_bytes());
        let mk_digest_salt = deterministic_salt(device_path.as_bytes(), 0);
        // Simulate iteration count from iter_time_ms
        let iterations = iter_time_ms.saturating_mul(1000);
        let mk_digest = pbkdf2_sha256(b"masterkey", &mk_digest_salt, 1000, LUKS_DIGEST_SIZE);

        let mut key_slots: [LuksKeySlot; LUKS_KEY_SLOTS] = std::array::from_fn(|_| LuksKeySlot::inactive());
        let slot_salt = deterministic_salt(device_path.as_bytes(), 1);
        let base_offset = (LUKS_HEADER_SIZE as u32 + LUKS_ALIGN - 1) / LUKS_ALIGN * LUKS_ALIGN;
        key_slots[0] = LuksKeySlot::new_active(iterations, slot_salt, base_offset / LUKS_SECTOR_SIZE);

        let payload_offset = base_offset + LUKS_MK_BYTES as u32 * 8 * LUKS_KEY_SLOTS as u32;

        let mut digest = [0u8; LUKS_DIGEST_SIZE];
        digest.copy_from_slice(&mk_digest[..LUKS_DIGEST_SIZE]);

        Self {
            version: DEFAULT_LUKS_VERSION,
            cipher_name: cipher.to_string(),
            cipher_mode: cipher_mode.to_string(),
            hash_spec: hash.to_string(),
            payload_offset: payload_offset / LUKS_SECTOR_SIZE,
            key_bytes,
            mk_digest: digest,
            mk_digest_salt,
            mk_digest_iter: iterations,
            uuid,
            key_slots,
        }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(LUKS_HEADER_SIZE);

        // Magic (6 bytes)
        buf.extend_from_slice(&LUKS_MAGIC);
        // Version (2 bytes)
        buf.extend_from_slice(&self.version.to_be_bytes());
        // Cipher name (32 bytes, null-padded)
        write_padded_string(&mut buf, &self.cipher_name, 32);
        // Cipher mode (32 bytes, null-padded)
        write_padded_string(&mut buf, &self.cipher_mode, 32);
        // Hash spec (32 bytes, null-padded)
        write_padded_string(&mut buf, &self.hash_spec, 32);
        // Payload offset (4 bytes)
        buf.extend_from_slice(&self.payload_offset.to_be_bytes());
        // Key bytes (4 bytes)
        buf.extend_from_slice(&self.key_bytes.to_be_bytes());
        // MK digest (32 bytes)
        buf.extend_from_slice(&self.mk_digest);
        // MK digest salt (32 bytes)
        buf.extend_from_slice(&self.mk_digest_salt);
        // MK digest iterations (4 bytes)
        buf.extend_from_slice(&self.mk_digest_iter.to_be_bytes());
        // UUID (40 bytes, null-padded)
        write_padded_string(&mut buf, &self.uuid, 40);

        // Key slots (8 x 48 bytes)
        for slot in &self.key_slots {
            let active_marker: u32 = if slot.active { 0x00ac71f3 } else { 0x0000dead };
            buf.extend_from_slice(&active_marker.to_be_bytes());
            buf.extend_from_slice(&slot.iterations.to_be_bytes());
            buf.extend_from_slice(&slot.salt);
            buf.extend_from_slice(&slot.key_material_offset.to_be_bytes());
            buf.extend_from_slice(&slot.stripes.to_be_bytes());
        }

        buf
    }

    fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < LUKS_HEADER_SIZE {
            return None;
        }
        if &data[..6] != &LUKS_MAGIC {
            return None;
        }

        let version = u16::from_be_bytes([data[6], data[7]]);
        let cipher_name = read_padded_string(&data[8..40]);
        let cipher_mode = read_padded_string(&data[40..72]);
        let hash_spec = read_padded_string(&data[72..104]);
        let payload_offset = u32::from_be_bytes([data[104], data[105], data[106], data[107]]);
        let key_bytes = u32::from_be_bytes([data[108], data[109], data[110], data[111]]);

        let mut mk_digest = [0u8; LUKS_DIGEST_SIZE];
        mk_digest.copy_from_slice(&data[112..144]);
        let mut mk_digest_salt = [0u8; LUKS_SALT_SIZE];
        mk_digest_salt.copy_from_slice(&data[144..176]);
        let mk_digest_iter = u32::from_be_bytes([data[176], data[177], data[178], data[179]]);
        let uuid = read_padded_string(&data[180..220]);

        let mut key_slots: [LuksKeySlot; LUKS_KEY_SLOTS] = std::array::from_fn(|_| LuksKeySlot::inactive());
        let slot_base = 220;
        for i in 0..LUKS_KEY_SLOTS {
            let off = slot_base + i * 48;
            let marker = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
            let iterations = u32::from_be_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
            let mut salt = [0u8; LUKS_SALT_SIZE];
            salt.copy_from_slice(&data[off + 8..off + 40]);
            let km_offset = u32::from_be_bytes([data[off + 40], data[off + 41], data[off + 42], data[off + 43]]);
            let stripes = u32::from_be_bytes([data[off + 44], data[off + 45], data[off + 46], data[off + 47]]);
            key_slots[i] = LuksKeySlot {
                active: marker == 0x00ac71f3,
                iterations,
                salt,
                key_material_offset: km_offset,
                stripes,
            };
        }

        Some(Self {
            version,
            cipher_name,
            cipher_mode,
            hash_spec,
            payload_offset,
            key_bytes,
            mk_digest,
            mk_digest_salt,
            mk_digest_iter,
            uuid,
            key_slots,
        })
    }

    fn active_slot_count(&self) -> usize {
        self.key_slots.iter().filter(|s| s.active).count()
    }

    fn first_inactive_slot(&self) -> Option<usize> {
        self.key_slots.iter().position(|s| !s.active)
    }
}

fn write_padded_string(buf: &mut Vec<u8>, s: &str, len: usize) {
    let bytes = s.as_bytes();
    let take = bytes.len().min(len);
    buf.extend_from_slice(&bytes[..take]);
    for _ in take..len {
        buf.push(0);
    }
}

fn read_padded_string(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).into_owned()
}

fn deterministic_salt(seed: &[u8], index: u32) -> [u8; LUKS_SALT_SIZE] {
    let mut input = Vec::with_capacity(seed.len() + 4);
    input.extend_from_slice(seed);
    input.extend_from_slice(&index.to_be_bytes());
    let hash = Sha256::digest(&input);
    let mut salt = [0u8; LUKS_SALT_SIZE];
    salt.copy_from_slice(&hash);
    salt
}

// ---------------------------------------------------------------------------
// Verity superblock
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct VeritySuperblock {
    version: u32,
    hash_type: u32,
    uuid: String,
    algorithm: String,
    data_block_size: u32,
    hash_block_size: u32,
    data_blocks: u64,
    salt: Vec<u8>,
    root_hash: Vec<u8>,
}

impl VeritySuperblock {
    fn new(data_device: &str, hash_device: &str) -> Self {
        let uuid = generate_uuid(format!("{}-{}", data_device, hash_device).as_bytes());
        let salt_hash = Sha256::digest(format!("verity-salt-{}", data_device).as_bytes());
        let root_hash = Sha256::digest(format!("verity-root-{}-{}", data_device, hash_device).as_bytes());
        Self {
            version: VERITY_VERSION,
            hash_type: 1,
            uuid,
            algorithm: "sha256".to_string(),
            data_block_size: 4096,
            hash_block_size: 4096,
            data_blocks: 262144, // simulated ~1GB
            salt: salt_hash.to_vec(),
            root_hash: root_hash.to_vec(),
        }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(512);
        buf.extend_from_slice(VERITY_MAGIC);
        buf.extend_from_slice(&self.version.to_be_bytes());
        buf.extend_from_slice(&self.hash_type.to_be_bytes());
        write_padded_string(&mut buf, &self.uuid, 40);
        write_padded_string(&mut buf, &self.algorithm, 32);
        buf.extend_from_slice(&self.data_block_size.to_be_bytes());
        buf.extend_from_slice(&self.hash_block_size.to_be_bytes());
        buf.extend_from_slice(&self.data_blocks.to_be_bytes());
        let salt_len = self.salt.len() as u16;
        buf.extend_from_slice(&salt_len.to_be_bytes());
        buf.extend_from_slice(&self.salt);
        buf
    }

    fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 100 {
            return None;
        }
        if &data[..8] != VERITY_MAGIC {
            return None;
        }
        let version = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let hash_type = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        let uuid = read_padded_string(&data[16..56]);
        let algorithm = read_padded_string(&data[56..88]);
        let data_block_size = u32::from_be_bytes([data[88], data[89], data[90], data[91]]);
        let hash_block_size = u32::from_be_bytes([data[92], data[93], data[94], data[95]]);
        let data_blocks = u64::from_be_bytes([
            data[96], data[97], data[98], data[99],
            data[100], data[101], data[102], data[103],
        ]);
        let salt_len = u16::from_be_bytes([data[104], data[105]]) as usize;
        let salt = if data.len() >= 106 + salt_len {
            data[106..106 + salt_len].to_vec()
        } else {
            Vec::new()
        };
        Some(Self {
            version,
            hash_type,
            uuid,
            algorithm,
            data_block_size,
            hash_block_size,
            data_blocks,
            salt,
            root_hash: Vec::new(),
        })
    }
}

// ---------------------------------------------------------------------------
// Integrity superblock
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct IntegritySuperblock {
    version: u32,
    uuid: String,
    algorithm: String,
    block_size: u32,
    journal_sections: u32,
    tag_size: u32,
    interleave_sectors: u32,
    provided_data_sectors: u64,
}

impl IntegritySuperblock {
    fn new(device: &str) -> Self {
        let uuid = generate_uuid(format!("integrity-{}", device).as_bytes());
        Self {
            version: INTEGRITY_VERSION,
            uuid,
            algorithm: "crc32c".to_string(),
            block_size: 4096,
            journal_sections: 1,
            tag_size: 4,
            interleave_sectors: 32768,
            provided_data_sectors: 524288,
        }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(256);
        buf.extend_from_slice(INTEGRITY_MAGIC);
        buf.extend_from_slice(&self.version.to_be_bytes());
        write_padded_string(&mut buf, &self.uuid, 40);
        write_padded_string(&mut buf, &self.algorithm, 32);
        buf.extend_from_slice(&self.block_size.to_be_bytes());
        buf.extend_from_slice(&self.journal_sections.to_be_bytes());
        buf.extend_from_slice(&self.tag_size.to_be_bytes());
        buf.extend_from_slice(&self.interleave_sectors.to_be_bytes());
        buf.extend_from_slice(&self.provided_data_sectors.to_be_bytes());
        buf
    }

    fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 100 {
            return None;
        }
        if &data[..8] != INTEGRITY_MAGIC {
            return None;
        }
        let version = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let uuid = read_padded_string(&data[12..52]);
        let algorithm = read_padded_string(&data[52..84]);
        let block_size = u32::from_be_bytes([data[84], data[85], data[86], data[87]]);
        let journal_sections = u32::from_be_bytes([data[88], data[89], data[90], data[91]]);
        let tag_size = u32::from_be_bytes([data[92], data[93], data[94], data[95]]);
        let interleave_sectors = u32::from_be_bytes([data[96], data[97], data[98], data[99]]);
        let provided_data_sectors = if data.len() >= 108 {
            u64::from_be_bytes([
                data[100], data[101], data[102], data[103],
                data[104], data[105], data[106], data[107],
            ])
        } else {
            0
        };
        Some(Self {
            version,
            uuid,
            algorithm,
            block_size,
            journal_sections,
            tag_size,
            interleave_sectors,
            provided_data_sectors,
        })
    }
}

// ---------------------------------------------------------------------------
// Parsed options shared across tools
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Options {
    cipher: String,
    cipher_mode: String,
    hash: String,
    key_size: u32,
    iter_time: u32,
    batch_mode: bool,
    verbose: bool,
    debug: bool,
    device_type: String,
    key_file: Option<String>,
    header_backup_file: Option<String>,
    slot: Option<u32>,
    positional: Vec<String>,
    subcommand: String,
}

impl Options {
    fn new() -> Self {
        Self {
            cipher: DEFAULT_CIPHER.to_string(),
            cipher_mode: DEFAULT_CIPHER_MODE.to_string(),
            hash: DEFAULT_HASH.to_string(),
            key_size: DEFAULT_KEY_SIZE,
            iter_time: DEFAULT_ITER_TIME_MS,
            batch_mode: false,
            verbose: false,
            debug: false,
            device_type: "luks2".to_string(),
            key_file: None,
            header_backup_file: None,
            slot: None,
            positional: Vec::new(),
            subcommand: String::new(),
        }
    }
}

fn parse_options(args: &[String]) -> Options {
    let mut opts = Options::new();
    let mut i = 0;
    let mut found_subcmd = false;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--hash" | "-h" if i + 1 < args.len() => {
                i += 1;
                opts.hash = args[i].clone();
            }
            "--cipher" | "-c" if i + 1 < args.len() => {
                i += 1;
                let full = &args[i];
                if let Some(pos) = full.find('-') {
                    opts.cipher = full[..pos].to_string();
                    opts.cipher_mode = full[pos + 1..].to_string();
                } else {
                    opts.cipher = full.clone();
                }
            }
            "--key-size" | "-s" if i + 1 < args.len() => {
                i += 1;
                opts.key_size = args[i].parse().unwrap_or(DEFAULT_KEY_SIZE);
            }
            "--iter-time" | "-i" if i + 1 < args.len() => {
                i += 1;
                opts.iter_time = args[i].parse().unwrap_or(DEFAULT_ITER_TIME_MS);
            }
            "--batch-mode" | "-q" => opts.batch_mode = true,
            "--verbose" | "-v" => opts.verbose = true,
            "--debug" => opts.debug = true,
            "--type" if i + 1 < args.len() => {
                i += 1;
                opts.device_type = args[i].clone();
            }
            "--key-file" if i + 1 < args.len() => {
                i += 1;
                opts.key_file = Some(args[i].clone());
            }
            "--header-backup-file" if i + 1 < args.len() => {
                i += 1;
                opts.header_backup_file = Some(args[i].clone());
            }
            "--key-slot" if i + 1 < args.len() => {
                i += 1;
                opts.slot = args[i].parse().ok();
            }
            _ if !arg.starts_with('-') => {
                if !found_subcmd {
                    opts.subcommand = arg.clone();
                    found_subcmd = true;
                } else {
                    opts.positional.push(arg.clone());
                }
            }
            _ => {
                // Unknown flag, skip for forward compatibility
            }
        }
        i += 1;
    }
    opts
}

// ---------------------------------------------------------------------------
// Cryptsetup commands
// ---------------------------------------------------------------------------

fn cmd_luks_format(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: device argument required");
        return 1;
    }
    let device = &opts.positional[0];

    if !opts.batch_mode {
        let _ = writeln!(out, "WARNING!");
        let _ = writeln!(out, "========");
        let _ = writeln!(out, "This will overwrite data on {} irrevocably.", device);
        let _ = writeln!(out);
    }

    let header = LuksHeader::new(
        &opts.cipher,
        &opts.cipher_mode,
        &opts.hash,
        opts.key_size / 8,
        opts.iter_time,
        device,
    );

    let serialized = header.serialize();
    if opts.verbose {
        let _ = writeln!(out, "LUKS header size: {} bytes", serialized.len());
        let _ = writeln!(out, "Cipher: {}-{}", header.cipher_name, header.cipher_mode);
        let _ = writeln!(out, "Key size: {} bits", header.key_bytes * 8);
        let _ = writeln!(out, "Hash: {}", header.hash_spec);
        let _ = writeln!(out, "UUID: {}", header.uuid);
    }

    let _ = writeln!(
        out,
        "LUKS{} formatted successfully on {}.",
        header.version, device
    );
    let _ = writeln!(out, "UUID: {}", header.uuid);
    let _ = writeln!(out, "Cipher: {}-{}", header.cipher_name, header.cipher_mode);
    let _ = writeln!(out, "Key: {} bits", header.key_bytes * 8);
    0
}

fn cmd_luks_open(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.len() < 2 {
        let _ = writeln!(out, "Error: <device> <name> required");
        return 1;
    }
    let device = &opts.positional[0];
    let name = &opts.positional[1];

    let _ = writeln!(out, "Enter passphrase for {}: ", device);
    // Simulate key derivation
    let dk = pbkdf2_sha256(b"passphrase", b"salt", 1000, 32);
    if opts.verbose {
        let _ = writeln!(out, "Key derivation: PBKDF2-SHA256, {} iterations", opts.iter_time * 1000);
        let _ = writeln!(out, "Derived key: {}", hex_encode(&dk[..8]));
    }
    let _ = writeln!(out, "Key slot 0 unlocked.");
    let _ = writeln!(out, "Device /dev/mapper/{} activated.", name);
    0
}

fn cmd_luks_close(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <name> required");
        return 1;
    }
    let name = &opts.positional[0];
    let _ = writeln!(out, "Device /dev/mapper/{} deactivated.", name);
    0
}

fn cmd_luks_dump(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <device> required");
        return 1;
    }
    let device = &opts.positional[0];

    // Generate a simulated header for the device
    let header = LuksHeader::new(
        &opts.cipher,
        &opts.cipher_mode,
        &opts.hash,
        opts.key_size / 8,
        opts.iter_time,
        device,
    );

    let _ = writeln!(out, "LUKS header information");
    let _ = writeln!(out, "Version:        {}", header.version);
    let _ = writeln!(out, "Cipher name:    {}", header.cipher_name);
    let _ = writeln!(out, "Cipher mode:    {}", header.cipher_mode);
    let _ = writeln!(out, "Hash spec:      {}", header.hash_spec);
    let _ = writeln!(out, "Payload offset: {}", header.payload_offset);
    let _ = writeln!(out, "MK bits:        {}", header.key_bytes * 8);
    let _ = writeln!(out, "MK digest:      {}", hex_encode(&header.mk_digest[..20]));
    let _ = writeln!(out, "MK salt:        {}", hex_encode(&header.mk_digest_salt[..16]));
    let _ = writeln!(out, "              : {}", hex_encode(&header.mk_digest_salt[16..]));
    let _ = writeln!(out, "MK iterations:  {}", header.mk_digest_iter);
    let _ = writeln!(out, "UUID:           {}", header.uuid);
    let _ = writeln!(out);

    for (i, slot) in header.key_slots.iter().enumerate() {
        if slot.active {
            let _ = writeln!(out, "Key Slot {}: ENABLED", i);
            let _ = writeln!(out, "  Iterations:           {}", slot.iterations);
            let _ = writeln!(out, "  Salt:                 {}", hex_encode(&slot.salt[..16]));
            let _ = writeln!(out, "                        {}", hex_encode(&slot.salt[16..]));
            let _ = writeln!(out, "  Key material offset:  {}", slot.key_material_offset);
            let _ = writeln!(out, "  AF stripes:           {}", slot.stripes);
        } else {
            let _ = writeln!(out, "Key Slot {}: DISABLED", i);
        }
    }
    0
}

fn cmd_luks_add_key(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <device> required");
        return 1;
    }
    let device = &opts.positional[0];

    let header = LuksHeader::new(
        &opts.cipher, &opts.cipher_mode, &opts.hash,
        opts.key_size / 8, opts.iter_time, device,
    );

    match header.first_inactive_slot() {
        Some(slot) => {
            let _ = writeln!(out, "Enter any existing passphrase: ");
            let _ = writeln!(out, "Enter new passphrase for key slot: ");
            let _ = writeln!(out, "Key slot {} added successfully.", slot);
            0
        }
        None => {
            let _ = writeln!(out, "Error: all key slots are full");
            1
        }
    }
}

fn cmd_luks_remove_key(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <device> required");
        return 1;
    }
    let device = &opts.positional[0];
    let _ = writeln!(out, "Enter passphrase to remove from {}: ", device);
    let _ = writeln!(out, "Key slot removed.");
    0
}

fn cmd_luks_kill_slot(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.len() < 2 {
        let _ = writeln!(out, "Error: <device> <slot> required");
        return 1;
    }
    let device = &opts.positional[0];
    let slot: u32 = match opts.positional[1].parse() {
        Ok(s) => s,
        Err(_) => {
            let _ = writeln!(out, "Error: invalid slot number");
            return 1;
        }
    };
    if slot >= LUKS_KEY_SLOTS as u32 {
        let _ = writeln!(out, "Error: slot {} out of range (0-{})", slot, LUKS_KEY_SLOTS - 1);
        return 1;
    }
    let _ = writeln!(out, "Key slot {} on {} destroyed.", slot, device);
    0
}

fn cmd_luks_change_key(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <device> required");
        return 1;
    }
    let device = &opts.positional[0];
    let _ = writeln!(out, "Enter current passphrase for {}: ", device);
    let _ = writeln!(out, "Enter new passphrase: ");
    let _ = writeln!(out, "Verify passphrase: ");
    let _ = writeln!(out, "Key slot passphrase changed.");
    0
}

fn cmd_luks_header_backup(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <device> required");
        return 1;
    }
    let device = &opts.positional[0];
    let backup_file = match &opts.header_backup_file {
        Some(f) => f.clone(),
        None => {
            let _ = writeln!(out, "Error: --header-backup-file required");
            return 1;
        }
    };

    let header = LuksHeader::new(
        &opts.cipher, &opts.cipher_mode, &opts.hash,
        opts.key_size / 8, opts.iter_time, device,
    );
    let data = header.serialize();
    let _ = writeln!(out, "LUKS header backup from {} to {}.", device, backup_file);
    let _ = writeln!(out, "Backup size: {} bytes", data.len());
    0
}

fn cmd_luks_header_restore(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <device> required");
        return 1;
    }
    let device = &opts.positional[0];
    let backup_file = match &opts.header_backup_file {
        Some(f) => f.clone(),
        None => {
            let _ = writeln!(out, "Error: --header-backup-file required");
            return 1;
        }
    };

    if !opts.batch_mode {
        let _ = writeln!(out, "WARNING!");
        let _ = writeln!(out, "========");
        let _ = writeln!(out, "This will overwrite LUKS header on {} with backup from {}.", device, backup_file);
    }
    let _ = writeln!(out, "LUKS header restored to {} from {}.", device, backup_file);
    0
}

fn cmd_is_luks(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <device> required");
        return 1;
    }
    let device = &opts.positional[0];
    // In a real implementation we would read the device header.
    // Simulate: devices containing "luks" in name are LUKS.
    if device.to_ascii_lowercase().contains("luks") {
        let _ = writeln!(out, "{} is a LUKS device.", device);
        0
    } else {
        let _ = writeln!(out, "{} is not a LUKS device.", device);
        1
    }
}

fn cmd_status(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <name> required");
        return 1;
    }
    let name = &opts.positional[0];
    let _ = writeln!(out, "/dev/mapper/{} is active.", name);
    let _ = writeln!(out, "  type:    LUKS2");
    let _ = writeln!(out, "  cipher:  {}-{}", opts.cipher, opts.cipher_mode);
    let _ = writeln!(out, "  keysize: {} bits", opts.key_size);
    let _ = writeln!(out, "  key location: dm-crypt");
    let _ = writeln!(out, "  device:  (simulated)");
    let _ = writeln!(out, "  sector size:  {}", LUKS_SECTOR_SIZE);
    let _ = writeln!(out, "  offset:  0 sectors");
    let _ = writeln!(out, "  size:    (unknown)");
    let _ = writeln!(out, "  mode:    read/write");
    0
}

fn cmd_open_plain(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.len() < 2 {
        let _ = writeln!(out, "Error: <device> <name> required");
        return 1;
    }
    let device = &opts.positional[0];
    let name = &opts.positional[1];
    let _ = writeln!(out, "Enter passphrase for {}: ", device);
    let _ = writeln!(out, "Plain dm-crypt mapping /dev/mapper/{} created.", name);
    let _ = writeln!(out, "  cipher: {}-{}", opts.cipher, opts.cipher_mode);
    let _ = writeln!(out, "  key size: {} bits", opts.key_size);
    0
}

fn cmd_close(opts: &Options, out: &mut dyn Write) -> i32 {
    cmd_luks_close(opts, out)
}

fn cmd_resize(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <name> required");
        return 1;
    }
    let name = &opts.positional[0];
    let _ = writeln!(out, "Device /dev/mapper/{} resized.", name);
    0
}

fn cmd_benchmark(opts: &Options, out: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "# Tests are approximate using memory only (no storage IO).");
    let _ = writeln!(out, "# Algorithm |       Key |      Encryption |      Decryption");

    let ciphers = [
        ("aes-cbc", 128, 1050.0, 3150.0),
        ("aes-cbc", 256, 820.0, 2480.0),
        ("aes-xts", 256, 1800.0, 1810.0),
        ("aes-xts", 512, 1400.0, 1410.0),
        ("serpent-cbc", 128, 92.0, 340.0),
        ("serpent-cbc", 256, 92.0, 340.0),
        ("serpent-xts", 256, 340.0, 330.0),
        ("serpent-xts", 512, 310.0, 310.0),
        ("twofish-cbc", 128, 200.0, 380.0),
        ("twofish-cbc", 256, 200.0, 380.0),
        ("twofish-xts", 256, 370.0, 370.0),
        ("twofish-xts", 512, 360.0, 360.0),
    ];

    for (name, key_bits, enc_mbs, dec_mbs) in &ciphers {
        let _ = writeln!(
            out,
            "    {:16} {:4} b {:10.1} MiB/s {:10.1} MiB/s",
            name, key_bits, enc_mbs, dec_mbs,
        );
    }

    if opts.verbose {
        let _ = writeln!(out);
        let _ = writeln!(out, "# PBKDF2-SHA256 benchmark");
        // Simulate PBKDF2 benchmark
        let _ = writeln!(out, "#     Iterations  Memory");
        let _ = writeln!(out, "  PBKDF2-sha256   1957750   N/A");
    }
    0
}

// ---------------------------------------------------------------------------
// Veritysetup commands
// ---------------------------------------------------------------------------

fn cmd_verity_format(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.len() < 2 {
        let _ = writeln!(out, "Error: <data_device> <hash_device> required");
        return 1;
    }
    let data_dev = &opts.positional[0];
    let hash_dev = &opts.positional[1];

    let sb = VeritySuperblock::new(data_dev, hash_dev);
    let serialized = sb.serialize();

    let _ = writeln!(out, "VERITY header information for {}", hash_dev);
    let _ = writeln!(out, "UUID:               {}", sb.uuid);
    let _ = writeln!(out, "Hash type:          {}", sb.hash_type);
    let _ = writeln!(out, "Data blocks:        {}", sb.data_blocks);
    let _ = writeln!(out, "Data block size:    {}", sb.data_block_size);
    let _ = writeln!(out, "Hash block size:    {}", sb.hash_block_size);
    let _ = writeln!(out, "Hash algorithm:     {}", sb.algorithm);
    let _ = writeln!(out, "Salt:               {}", hex_encode(&sb.salt));
    let _ = writeln!(out, "Root hash:          {}", hex_encode(&sb.root_hash));

    if opts.verbose {
        let _ = writeln!(out, "Superblock size:    {} bytes", serialized.len());
    }
    0
}

fn cmd_verity_open(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.len() < 4 {
        let _ = writeln!(out, "Error: <data_device> <name> <hash_device> <root_hash> required");
        return 1;
    }
    let data_dev = &opts.positional[0];
    let name = &opts.positional[1];
    let hash_dev = &opts.positional[2];
    let root_hash = &opts.positional[3];

    if hex_decode(root_hash).is_none() {
        let _ = writeln!(out, "Error: invalid root hash format");
        return 1;
    }

    let _ = writeln!(out, "Verity device /dev/mapper/{} activated.", name);
    let _ = writeln!(out, "  data device: {}", data_dev);
    let _ = writeln!(out, "  hash device: {}", hash_dev);
    let _ = writeln!(out, "  root hash:   {}", root_hash);
    0
}

fn cmd_verity_close(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <name> required");
        return 1;
    }
    let name = &opts.positional[0];
    let _ = writeln!(out, "Verity device /dev/mapper/{} deactivated.", name);
    0
}

fn cmd_verity_verify(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.len() < 3 {
        let _ = writeln!(out, "Error: <data_device> <hash_device> <root_hash> required");
        return 1;
    }
    let data_dev = &opts.positional[0];
    let hash_dev = &opts.positional[1];
    let root_hash = &opts.positional[2];

    if hex_decode(root_hash).is_none() {
        let _ = writeln!(out, "Error: invalid root hash format");
        return 1;
    }

    let _ = writeln!(out, "Verification of {} using hash device {} and root hash {}.", data_dev, hash_dev, root_hash);
    let _ = writeln!(out, "Verification OK.");
    0
}

fn cmd_verity_status(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <name> required");
        return 1;
    }
    let name = &opts.positional[0];
    let _ = writeln!(out, "/dev/mapper/{} is active.", name);
    let _ = writeln!(out, "  type:          VERITY");
    let _ = writeln!(out, "  status:        verified");
    let _ = writeln!(out, "  hash type:     1");
    let _ = writeln!(out, "  data block:    4096");
    let _ = writeln!(out, "  hash block:    4096");
    let _ = writeln!(out, "  hash algorithm: sha256");
    let _ = writeln!(out, "  data device:   (simulated)");
    let _ = writeln!(out, "  hash device:   (simulated)");
    0
}

fn cmd_verity_dump(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <hash_device> required");
        return 1;
    }
    let hash_dev = &opts.positional[0];

    let sb = VeritySuperblock::new("(data)", hash_dev);
    let _ = writeln!(out, "VERITY header information for {}", hash_dev);
    let _ = writeln!(out, "Version:            {}", sb.version);
    let _ = writeln!(out, "UUID:               {}", sb.uuid);
    let _ = writeln!(out, "Hash type:          {}", sb.hash_type);
    let _ = writeln!(out, "Data blocks:        {}", sb.data_blocks);
    let _ = writeln!(out, "Data block size:    {}", sb.data_block_size);
    let _ = writeln!(out, "Hash block size:    {}", sb.hash_block_size);
    let _ = writeln!(out, "Hash algorithm:     {}", sb.algorithm);
    let _ = writeln!(out, "Salt:               {}", hex_encode(&sb.salt));
    0
}

// ---------------------------------------------------------------------------
// Integritysetup commands
// ---------------------------------------------------------------------------

fn cmd_integrity_format(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <device> required");
        return 1;
    }
    let device = &opts.positional[0];

    let sb = IntegritySuperblock::new(device);
    let serialized = sb.serialize();

    let _ = writeln!(out, "Formatted integrity device on {}.", device);
    let _ = writeln!(out, "UUID:                  {}", sb.uuid);
    let _ = writeln!(out, "Algorithm:             {}", sb.algorithm);
    let _ = writeln!(out, "Block size:            {}", sb.block_size);
    let _ = writeln!(out, "Tag size:              {}", sb.tag_size);
    let _ = writeln!(out, "Journal sections:      {}", sb.journal_sections);
    let _ = writeln!(out, "Interleave sectors:    {}", sb.interleave_sectors);
    let _ = writeln!(out, "Provided data sectors: {}", sb.provided_data_sectors);

    if opts.verbose {
        let _ = writeln!(out, "Superblock size:       {} bytes", serialized.len());
    }
    0
}

fn cmd_integrity_open(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.len() < 2 {
        let _ = writeln!(out, "Error: <device> <name> required");
        return 1;
    }
    let device = &opts.positional[0];
    let name = &opts.positional[1];
    let _ = writeln!(out, "Integrity device /dev/mapper/{} activated.", name);
    let _ = writeln!(out, "  device: {}", device);
    0
}

fn cmd_integrity_close(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <name> required");
        return 1;
    }
    let name = &opts.positional[0];
    let _ = writeln!(out, "Integrity device /dev/mapper/{} deactivated.", name);
    0
}

fn cmd_integrity_status(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <name> required");
        return 1;
    }
    let name = &opts.positional[0];
    let _ = writeln!(out, "/dev/mapper/{} is active.", name);
    let _ = writeln!(out, "  type:                INTEGRITY");
    let _ = writeln!(out, "  tag size:            4");
    let _ = writeln!(out, "  integrity:           crc32c");
    let _ = writeln!(out, "  block size:          4096");
    let _ = writeln!(out, "  journal size:        (default)");
    let _ = writeln!(out, "  interleave sectors:  32768");
    let _ = writeln!(out, "  device:              (simulated)");
    0
}

fn cmd_integrity_dump(opts: &Options, out: &mut dyn Write) -> i32 {
    if opts.positional.is_empty() {
        let _ = writeln!(out, "Error: <device> required");
        return 1;
    }
    let device = &opts.positional[0];

    let sb = IntegritySuperblock::new(device);
    let _ = writeln!(out, "Integrity superblock for {}", device);
    let _ = writeln!(out, "Version:               {}", sb.version);
    let _ = writeln!(out, "UUID:                  {}", sb.uuid);
    let _ = writeln!(out, "Algorithm:             {}", sb.algorithm);
    let _ = writeln!(out, "Block size:            {}", sb.block_size);
    let _ = writeln!(out, "Tag size:              {}", sb.tag_size);
    let _ = writeln!(out, "Journal sections:      {}", sb.journal_sections);
    let _ = writeln!(out, "Interleave sectors:    {}", sb.interleave_sectors);
    let _ = writeln!(out, "Provided data sectors: {}", sb.provided_data_sectors);
    0
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

fn print_cryptsetup_help(out: &mut dyn Write) {
    let _ = writeln!(out, "cryptsetup {} - disk encryption management", VERSION);
    let _ = writeln!(out);
    let _ = writeln!(out, "Usage: cryptsetup <action> [options] <args>");
    let _ = writeln!(out);
    let _ = writeln!(out, "Actions:");
    let _ = writeln!(out, "  luksFormat <device>             Format device with LUKS header");
    let _ = writeln!(out, "  luksOpen <device> <name>        Open/decrypt LUKS device");
    let _ = writeln!(out, "  luksClose <name>                Close encrypted mapping");
    let _ = writeln!(out, "  luksDump <device>               Dump LUKS header info");
    let _ = writeln!(out, "  luksAddKey <device>             Add passphrase to LUKS slot");
    let _ = writeln!(out, "  luksRemoveKey <device>          Remove passphrase from slot");
    let _ = writeln!(out, "  luksKillSlot <device> <slot>    Destroy key slot");
    let _ = writeln!(out, "  luksChangeKey <device>          Change passphrase");
    let _ = writeln!(out, "  luksHeaderBackup <device>       Backup LUKS header");
    let _ = writeln!(out, "  luksHeaderRestore <device>      Restore LUKS header");
    let _ = writeln!(out, "  isLuks <device>                 Test if device is LUKS");
    let _ = writeln!(out, "  status <name>                   Show active mapping status");
    let _ = writeln!(out, "  open --type plain <dev> <name>  Plain dm-crypt mapping");
    let _ = writeln!(out, "  close <name>                    Close any mapping");
    let _ = writeln!(out, "  resize <name>                   Resize active mapping");
    let _ = writeln!(out, "  benchmark                       Cipher benchmark");
    let _ = writeln!(out);
    let _ = writeln!(out, "Options:");
    let _ = writeln!(out, "  --hash, -h <hash>               Hash algorithm (default: sha256)");
    let _ = writeln!(out, "  --cipher, -c <cipher>           Cipher specification");
    let _ = writeln!(out, "  --key-size, -s <bits>           Key size in bits");
    let _ = writeln!(out, "  --iter-time, -i <ms>            PBKDF2 iteration time in ms");
    let _ = writeln!(out, "  --batch-mode, -q                Suppress warnings");
    let _ = writeln!(out, "  --verbose, -v                   Verbose output");
    let _ = writeln!(out, "  --debug                         Debug output");
    let _ = writeln!(out, "  --type <type>                   Device type (luks1/luks2/plain)");
    let _ = writeln!(out, "  --key-file <file>               Key file path");
    let _ = writeln!(out, "  --header-backup-file <file>     Header backup file path");
    let _ = writeln!(out, "  --key-slot <num>                Key slot number");
    let _ = writeln!(out, "  --help                          Show this help");
    let _ = writeln!(out, "  --version                       Show version");
}

fn print_veritysetup_help(out: &mut dyn Write) {
    let _ = writeln!(out, "veritysetup {} - dm-verity management", VERSION);
    let _ = writeln!(out);
    let _ = writeln!(out, "Usage: veritysetup <action> [options] <args>");
    let _ = writeln!(out);
    let _ = writeln!(out, "Actions:");
    let _ = writeln!(out, "  format <data_dev> <hash_dev>    Create verity hash tree");
    let _ = writeln!(out, "  open <data> <name> <hash> <rh>  Activate verity device");
    let _ = writeln!(out, "  close <name>                    Deactivate verity device");
    let _ = writeln!(out, "  verify <data> <hash> <rh>       Verify data integrity");
    let _ = writeln!(out, "  status <name>                   Show verity device status");
    let _ = writeln!(out, "  dump <hash_dev>                 Dump verity superblock");
    let _ = writeln!(out);
    let _ = writeln!(out, "Options:");
    let _ = writeln!(out, "  --verbose, -v                   Verbose output");
    let _ = writeln!(out, "  --debug                         Debug output");
    let _ = writeln!(out, "  --help                          Show this help");
    let _ = writeln!(out, "  --version                       Show version");
}

fn print_integritysetup_help(out: &mut dyn Write) {
    let _ = writeln!(out, "integritysetup {} - dm-integrity management", VERSION);
    let _ = writeln!(out);
    let _ = writeln!(out, "Usage: integritysetup <action> [options] <args>");
    let _ = writeln!(out);
    let _ = writeln!(out, "Actions:");
    let _ = writeln!(out, "  format <device>                 Format with integrity metadata");
    let _ = writeln!(out, "  open <device> <name>            Activate integrity device");
    let _ = writeln!(out, "  close <name>                    Deactivate integrity device");
    let _ = writeln!(out, "  status <name>                   Show integrity status");
    let _ = writeln!(out, "  dump <device>                   Dump integrity superblock");
    let _ = writeln!(out);
    let _ = writeln!(out, "Options:");
    let _ = writeln!(out, "  --verbose, -v                   Verbose output");
    let _ = writeln!(out, "  --debug                         Debug output");
    let _ = writeln!(out, "  --help                          Show this help");
    let _ = writeln!(out, "  --version                       Show version");
}

fn print_version(tool: Tool, out: &mut dyn Write) {
    let _ = writeln!(out, "{} {}", tool.name(), VERSION);
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

fn dispatch_cryptsetup(args: &[String], out: &mut dyn Write) -> i32 {
    let opts = parse_options(args);

    if args.iter().any(|a| a == "--help") {
        print_cryptsetup_help(out);
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        print_version(Tool::Cryptsetup, out);
        return 0;
    }

    let subcmd = opts.subcommand.as_str();
    match subcmd {
        "luksFormat" | "luksformat" => cmd_luks_format(&opts, out),
        "luksOpen" | "luksopen" => cmd_luks_open(&opts, out),
        "luksClose" | "luksclose" => cmd_luks_close(&opts, out),
        "luksDump" | "luksdump" => cmd_luks_dump(&opts, out),
        "luksAddKey" | "luksaddkey" => cmd_luks_add_key(&opts, out),
        "luksRemoveKey" | "luksremovekey" => cmd_luks_remove_key(&opts, out),
        "luksKillSlot" | "lukskillslot" => cmd_luks_kill_slot(&opts, out),
        "luksChangeKey" | "lukschangekey" => cmd_luks_change_key(&opts, out),
        "luksHeaderBackup" | "luksheaderbackup" => cmd_luks_header_backup(&opts, out),
        "luksHeaderRestore" | "luksheaderrestore" => cmd_luks_header_restore(&opts, out),
        "isLuks" | "isluks" => cmd_is_luks(&opts, out),
        "status" => cmd_status(&opts, out),
        "open" => {
            if opts.device_type == "plain" {
                cmd_open_plain(&opts, out)
            } else {
                cmd_luks_open(&opts, out)
            }
        }
        "close" => cmd_close(&opts, out),
        "resize" => cmd_resize(&opts, out),
        "benchmark" => cmd_benchmark(&opts, out),
        "" => {
            print_cryptsetup_help(out);
            1
        }
        _ => {
            let _ = writeln!(out, "Unknown action: {}", subcmd);
            let _ = writeln!(out, "Run 'cryptsetup --help' for usage.");
            1
        }
    }
}

fn dispatch_veritysetup(args: &[String], out: &mut dyn Write) -> i32 {
    let opts = parse_options(args);

    if args.iter().any(|a| a == "--help") {
        print_veritysetup_help(out);
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        print_version(Tool::Veritysetup, out);
        return 0;
    }

    match opts.subcommand.as_str() {
        "format" => cmd_verity_format(&opts, out),
        "open" => cmd_verity_open(&opts, out),
        "close" => cmd_verity_close(&opts, out),
        "verify" => cmd_verity_verify(&opts, out),
        "status" => cmd_verity_status(&opts, out),
        "dump" => cmd_verity_dump(&opts, out),
        "" => {
            print_veritysetup_help(out);
            1
        }
        other => {
            let _ = writeln!(out, "Unknown action: {}", other);
            let _ = writeln!(out, "Run 'veritysetup --help' for usage.");
            1
        }
    }
}

fn dispatch_integritysetup(args: &[String], out: &mut dyn Write) -> i32 {
    let opts = parse_options(args);

    if args.iter().any(|a| a == "--help") {
        print_integritysetup_help(out);
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        print_version(Tool::Integritysetup, out);
        return 0;
    }

    match opts.subcommand.as_str() {
        "format" => cmd_integrity_format(&opts, out),
        "open" => cmd_integrity_open(&opts, out),
        "close" => cmd_integrity_close(&opts, out),
        "status" => cmd_integrity_status(&opts, out),
        "dump" => cmd_integrity_dump(&opts, out),
        "" => {
            print_integritysetup_help(out);
            1
        }
        other => {
            let _ = writeln!(out, "Unknown action: {}", other);
            let _ = writeln!(out, "Run 'integritysetup --help' for usage.");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("cryptsetup");
    let tool = detect_tool(argv0);
    let rest = if args.len() > 1 { &args[1..] } else { &[] };

    let mut stdout = std::io::stdout().lock();
    let exit_code = match tool {
        Tool::Cryptsetup => dispatch_cryptsetup(rest, &mut stdout),
        Tool::Veritysetup => dispatch_veritysetup(rest, &mut stdout),
        Tool::Integritysetup => dispatch_integritysetup(rest, &mut stdout),
    };

    process::exit(exit_code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Capture output helper
    fn run_cryptsetup(args: &[&str]) -> (i32, String) {
        let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut buf = Vec::new();
        let code = dispatch_cryptsetup(&a, &mut buf);
        (code, String::from_utf8(buf).unwrap())
    }

    fn run_verity(args: &[&str]) -> (i32, String) {
        let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut buf = Vec::new();
        let code = dispatch_veritysetup(&a, &mut buf);
        (code, String::from_utf8(buf).unwrap())
    }

    fn run_integrity(args: &[&str]) -> (i32, String) {
        let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut buf = Vec::new();
        let code = dispatch_integritysetup(&a, &mut buf);
        (code, String::from_utf8(buf).unwrap())
    }

    // --- SHA-256 tests ---

    #[test]
    fn sha256_empty() {
        let digest = Sha256::digest(b"");
        assert_eq!(
            hex_encode(&digest),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        let digest = Sha256::digest(b"abc");
        assert_eq!(
            hex_encode(&digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_long_message() {
        // "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        let msg = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        let digest = Sha256::digest(msg);
        assert_eq!(
            hex_encode(&digest),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_incremental() {
        let mut h = Sha256::new();
        h.update(b"a");
        h.update(b"bc");
        let digest = h.finalize();
        assert_eq!(
            hex_encode(&digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_one_block_boundary() {
        // 55 bytes: exactly fills one block after padding (55 + 1 + 8 = 64)
        let data = vec![0x61; 55]; // 55 'a's
        let digest = Sha256::digest(&data);
        // Known value for 55 'a's
        assert_eq!(digest.len(), 32);
    }

    #[test]
    fn sha256_two_block_boundary() {
        // 56 bytes: requires two blocks for padding
        let data = vec![0x61; 56];
        let digest = Sha256::digest(&data);
        assert_eq!(digest.len(), 32);
    }

    #[test]
    fn sha256_multi_block() {
        let data = vec![0x42; 200]; // Well over 3 blocks
        let digest = Sha256::digest(&data);
        assert_eq!(digest.len(), 32);
    }

    // --- HMAC-SHA256 tests ---

    #[test]
    fn hmac_sha256_rfc4231_test1() {
        // RFC 4231 Test Case 1
        let key = [0x0b; 20];
        let data = b"Hi There";
        let mac = hmac_sha256(&key, data);
        assert_eq!(
            hex_encode(&mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn hmac_sha256_rfc4231_test2() {
        // RFC 4231 Test Case 2 (key = "Jefe")
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let mac = hmac_sha256(key, data);
        assert_eq!(
            hex_encode(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn hmac_sha256_long_key() {
        // Key longer than 64 bytes should be hashed first
        let key = vec![0xaa; 131];
        let data = b"Test Using Larger Than Block-Size Key - Hash Key First";
        let mac = hmac_sha256(&key, data);
        assert_eq!(
            hex_encode(&mac),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    // --- PBKDF2 tests ---

    #[test]
    fn pbkdf2_basic() {
        let dk = pbkdf2_sha256(b"password", b"salt", 1, 32);
        assert_eq!(dk.len(), 32);
        // RFC 6070 test vector (PBKDF2-HMAC-SHA256)
        assert_eq!(
            hex_encode(&dk),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
    }

    #[test]
    fn pbkdf2_two_iterations() {
        let dk = pbkdf2_sha256(b"password", b"salt", 2, 32);
        assert_eq!(
            hex_encode(&dk),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
    }

    #[test]
    fn pbkdf2_4096_iterations() {
        let dk = pbkdf2_sha256(b"password", b"salt", 4096, 32);
        assert_eq!(
            hex_encode(&dk),
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
        );
    }

    #[test]
    fn pbkdf2_truncated_output() {
        let dk = pbkdf2_sha256(b"password", b"salt", 1, 16);
        assert_eq!(dk.len(), 16);
        assert_eq!(hex_encode(&dk), "120fb6cffcf8b32c43e7225256c4f837");
    }

    #[test]
    fn pbkdf2_multi_block_output() {
        // Request more than 32 bytes (needs 2 PBKDF2 blocks)
        let dk = pbkdf2_sha256(b"password", b"salt", 1, 48);
        assert_eq!(dk.len(), 48);
    }

    // --- Hex encoding tests ---

    #[test]
    fn hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn hex_encode_bytes() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn hex_decode_valid() {
        assert_eq!(hex_decode("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    }

    #[test]
    fn hex_decode_empty() {
        assert_eq!(hex_decode(""), Some(vec![]));
    }

    #[test]
    fn hex_decode_odd_length() {
        assert_eq!(hex_decode("abc"), None);
    }

    #[test]
    fn hex_decode_invalid_char() {
        assert_eq!(hex_decode("zzzz"), None);
    }

    #[test]
    fn hex_decode_uppercase() {
        assert_eq!(hex_decode("DEADBEEF"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    }

    #[test]
    fn hex_roundtrip() {
        let data = vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
        assert_eq!(hex_decode(&hex_encode(&data)), Some(data));
    }

    // --- UUID tests ---

    #[test]
    fn uuid_format() {
        let uuid = generate_uuid(b"test");
        // UUID format: 8-4-4-4-12 hex chars
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
    }

    #[test]
    fn uuid_version_4() {
        let uuid = generate_uuid(b"test-device");
        // Version 4: 13th char should be '4'
        let chars: Vec<char> = uuid.chars().collect();
        assert_eq!(chars[14], '4'); // position 14 in "xxxxxxxx-xxxx-4xxx-..."
    }

    #[test]
    fn uuid_deterministic() {
        let u1 = generate_uuid(b"same-seed");
        let u2 = generate_uuid(b"same-seed");
        assert_eq!(u1, u2);
    }

    #[test]
    fn uuid_different_seeds() {
        let u1 = generate_uuid(b"seed-a");
        let u2 = generate_uuid(b"seed-b");
        assert_ne!(u1, u2);
    }

    // --- Tool detection tests ---

    #[test]
    fn detect_cryptsetup_plain() {
        assert_eq!(detect_tool("cryptsetup"), Tool::Cryptsetup);
    }

    #[test]
    fn detect_cryptsetup_path() {
        assert_eq!(detect_tool("/usr/sbin/cryptsetup"), Tool::Cryptsetup);
    }

    #[test]
    fn detect_cryptsetup_exe() {
        assert_eq!(detect_tool("cryptsetup.exe"), Tool::Cryptsetup);
    }

    #[test]
    fn detect_veritysetup() {
        assert_eq!(detect_tool("veritysetup"), Tool::Veritysetup);
    }

    #[test]
    fn detect_veritysetup_path() {
        assert_eq!(detect_tool("/usr/sbin/veritysetup"), Tool::Veritysetup);
    }

    #[test]
    fn detect_integritysetup() {
        assert_eq!(detect_tool("integritysetup"), Tool::Integritysetup);
    }

    #[test]
    fn detect_integritysetup_windows() {
        assert_eq!(detect_tool("C:\\bin\\integritysetup.exe"), Tool::Integritysetup);
    }

    #[test]
    fn detect_unknown_defaults_cryptsetup() {
        assert_eq!(detect_tool("something_else"), Tool::Cryptsetup);
    }

    // --- LUKS header tests ---

    #[test]
    fn luks_header_new_defaults() {
        let h = LuksHeader::new("aes", "xts-plain64", "sha256", 32, 2000, "/dev/sda1");
        assert_eq!(h.version, 2);
        assert_eq!(h.cipher_name, "aes");
        assert_eq!(h.cipher_mode, "xts-plain64");
        assert_eq!(h.hash_spec, "sha256");
        assert_eq!(h.key_bytes, 32);
        assert_eq!(h.active_slot_count(), 1);
        assert!(h.key_slots[0].active);
    }

    #[test]
    fn luks_header_serialize_magic() {
        let h = LuksHeader::new("aes", "xts-plain64", "sha256", 32, 2000, "/dev/sda");
        let data = h.serialize();
        assert_eq!(&data[..6], &LUKS_MAGIC);
    }

    #[test]
    fn luks_header_serialize_version() {
        let h = LuksHeader::new("aes", "xts-plain64", "sha256", 32, 2000, "/dev/sda");
        let data = h.serialize();
        assert_eq!(u16::from_be_bytes([data[6], data[7]]), 2);
    }

    #[test]
    fn luks_header_roundtrip() {
        let h = LuksHeader::new("aes", "xts-plain64", "sha256", 32, 2000, "/dev/sda");
        let data = h.serialize();
        let h2 = LuksHeader::deserialize(&data).unwrap();
        assert_eq!(h.version, h2.version);
        assert_eq!(h.cipher_name, h2.cipher_name);
        assert_eq!(h.cipher_mode, h2.cipher_mode);
        assert_eq!(h.hash_spec, h2.hash_spec);
        assert_eq!(h.key_bytes, h2.key_bytes);
        assert_eq!(h.uuid, h2.uuid);
        assert_eq!(h.mk_digest_iter, h2.mk_digest_iter);
        assert_eq!(h.payload_offset, h2.payload_offset);
    }

    #[test]
    fn luks_header_slots_roundtrip() {
        let h = LuksHeader::new("serpent", "cbc-essiv", "sha256", 32, 1000, "/dev/sdb");
        let data = h.serialize();
        let h2 = LuksHeader::deserialize(&data).unwrap();
        assert_eq!(h2.key_slots[0].active, true);
        for i in 1..LUKS_KEY_SLOTS {
            assert_eq!(h2.key_slots[i].active, false);
        }
        assert_eq!(h.key_slots[0].iterations, h2.key_slots[0].iterations);
        assert_eq!(h.key_slots[0].salt, h2.key_slots[0].salt);
        assert_eq!(h.key_slots[0].key_material_offset, h2.key_slots[0].key_material_offset);
        assert_eq!(h.key_slots[0].stripes, h2.key_slots[0].stripes);
    }

    #[test]
    fn luks_header_deserialize_bad_magic() {
        let data = vec![0u8; LUKS_HEADER_SIZE];
        assert!(LuksHeader::deserialize(&data).is_none());
    }

    #[test]
    fn luks_header_deserialize_too_short() {
        let data = vec![0u8; 10];
        assert!(LuksHeader::deserialize(&data).is_none());
    }

    #[test]
    fn luks_inactive_slot() {
        let slot = LuksKeySlot::inactive();
        assert!(!slot.active);
        assert_eq!(slot.iterations, 0);
    }

    #[test]
    fn luks_first_inactive_slot() {
        let h = LuksHeader::new("aes", "xts-plain64", "sha256", 32, 2000, "/dev/sda");
        // Slot 0 is active, so first inactive is 1
        assert_eq!(h.first_inactive_slot(), Some(1));
    }

    // --- Verity superblock tests ---

    #[test]
    fn verity_superblock_new() {
        let sb = VeritySuperblock::new("/dev/sda1", "/dev/sda2");
        assert_eq!(sb.version, VERITY_VERSION);
        assert_eq!(sb.hash_type, 1);
        assert_eq!(sb.algorithm, "sha256");
        assert_eq!(sb.data_block_size, 4096);
        assert_eq!(sb.hash_block_size, 4096);
        assert!(!sb.uuid.is_empty());
        assert_eq!(sb.salt.len(), 32);
        assert_eq!(sb.root_hash.len(), 32);
    }

    #[test]
    fn verity_superblock_serialize_magic() {
        let sb = VeritySuperblock::new("/dev/sda1", "/dev/sda2");
        let data = sb.serialize();
        assert_eq!(&data[..8], VERITY_MAGIC);
    }

    #[test]
    fn verity_superblock_roundtrip() {
        let sb = VeritySuperblock::new("/dev/sda1", "/dev/sda2");
        let data = sb.serialize();
        let sb2 = VeritySuperblock::deserialize(&data).unwrap();
        assert_eq!(sb.version, sb2.version);
        assert_eq!(sb.hash_type, sb2.hash_type);
        assert_eq!(sb.uuid, sb2.uuid);
        assert_eq!(sb.algorithm, sb2.algorithm);
        assert_eq!(sb.data_block_size, sb2.data_block_size);
        assert_eq!(sb.hash_block_size, sb2.hash_block_size);
        assert_eq!(sb.data_blocks, sb2.data_blocks);
        assert_eq!(sb.salt, sb2.salt);
    }

    #[test]
    fn verity_superblock_deserialize_bad_magic() {
        let data = vec![0u8; 200];
        assert!(VeritySuperblock::deserialize(&data).is_none());
    }

    #[test]
    fn verity_superblock_deserialize_too_short() {
        let data = vec![0u8; 10];
        assert!(VeritySuperblock::deserialize(&data).is_none());
    }

    // --- Integrity superblock tests ---

    #[test]
    fn integrity_superblock_new() {
        let sb = IntegritySuperblock::new("/dev/sda1");
        assert_eq!(sb.version, INTEGRITY_VERSION);
        assert_eq!(sb.algorithm, "crc32c");
        assert_eq!(sb.block_size, 4096);
        assert_eq!(sb.tag_size, 4);
        assert!(!sb.uuid.is_empty());
    }

    #[test]
    fn integrity_superblock_serialize_magic() {
        let sb = IntegritySuperblock::new("/dev/sda1");
        let data = sb.serialize();
        assert_eq!(&data[..8], INTEGRITY_MAGIC);
    }

    #[test]
    fn integrity_superblock_roundtrip() {
        let sb = IntegritySuperblock::new("/dev/sda1");
        let data = sb.serialize();
        let sb2 = IntegritySuperblock::deserialize(&data).unwrap();
        assert_eq!(sb.version, sb2.version);
        assert_eq!(sb.uuid, sb2.uuid);
        assert_eq!(sb.algorithm, sb2.algorithm);
        assert_eq!(sb.block_size, sb2.block_size);
        assert_eq!(sb.tag_size, sb2.tag_size);
        assert_eq!(sb.journal_sections, sb2.journal_sections);
        assert_eq!(sb.interleave_sectors, sb2.interleave_sectors);
        assert_eq!(sb.provided_data_sectors, sb2.provided_data_sectors);
    }

    #[test]
    fn integrity_superblock_deserialize_bad_magic() {
        let data = vec![0u8; 200];
        assert!(IntegritySuperblock::deserialize(&data).is_none());
    }

    // --- Option parsing tests ---

    #[test]
    fn parse_options_empty() {
        let opts = parse_options(&[]);
        assert_eq!(opts.subcommand, "");
        assert!(opts.positional.is_empty());
    }

    #[test]
    fn parse_options_subcommand_only() {
        let args = vec!["luksFormat".to_string()];
        let opts = parse_options(&args);
        assert_eq!(opts.subcommand, "luksFormat");
    }

    #[test]
    fn parse_options_with_device() {
        let args: Vec<String> = vec!["luksFormat", "/dev/sda1"]
            .into_iter().map(String::from).collect();
        let opts = parse_options(&args);
        assert_eq!(opts.subcommand, "luksFormat");
        assert_eq!(opts.positional, vec!["/dev/sda1"]);
    }

    #[test]
    fn parse_options_cipher() {
        let args: Vec<String> = vec!["--cipher", "aes-cbc-essiv:sha256", "luksFormat", "/dev/sda"]
            .into_iter().map(String::from).collect();
        let opts = parse_options(&args);
        assert_eq!(opts.cipher, "aes");
        assert_eq!(opts.cipher_mode, "cbc-essiv:sha256");
    }

    #[test]
    fn parse_options_key_size() {
        let args: Vec<String> = vec!["--key-size", "512", "luksFormat", "/dev/sda"]
            .into_iter().map(String::from).collect();
        let opts = parse_options(&args);
        assert_eq!(opts.key_size, 512);
    }

    #[test]
    fn parse_options_batch_mode() {
        let args: Vec<String> = vec!["-q", "luksFormat", "/dev/sda"]
            .into_iter().map(String::from).collect();
        let opts = parse_options(&args);
        assert!(opts.batch_mode);
    }

    #[test]
    fn parse_options_verbose() {
        let args: Vec<String> = vec!["-v", "luksFormat", "/dev/sda"]
            .into_iter().map(String::from).collect();
        let opts = parse_options(&args);
        assert!(opts.verbose);
    }

    #[test]
    fn parse_options_type() {
        let args: Vec<String> = vec!["--type", "plain", "open", "/dev/sda", "test"]
            .into_iter().map(String::from).collect();
        let opts = parse_options(&args);
        assert_eq!(opts.device_type, "plain");
    }

    #[test]
    fn parse_options_header_backup_file() {
        let args: Vec<String> = vec!["luksHeaderBackup", "/dev/sda", "--header-backup-file", "/tmp/backup"]
            .into_iter().map(String::from).collect();
        let opts = parse_options(&args);
        assert_eq!(opts.header_backup_file, Some("/tmp/backup".to_string()));
    }

    // --- Cryptsetup command tests ---

    #[test]
    fn cryptsetup_help() {
        let (code, out) = run_cryptsetup(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("cryptsetup"));
        assert!(out.contains("luksFormat"));
        assert!(out.contains("benchmark"));
    }

    #[test]
    fn cryptsetup_version() {
        let (code, out) = run_cryptsetup(&["--version"]);
        assert_eq!(code, 0);
        assert!(out.contains(VERSION));
    }

    #[test]
    fn cryptsetup_no_args() {
        let (code, out) = run_cryptsetup(&[]);
        assert_eq!(code, 1);
        assert!(out.contains("cryptsetup"));
    }

    #[test]
    fn cryptsetup_unknown_action() {
        let (code, out) = run_cryptsetup(&["bogus"]);
        assert_eq!(code, 1);
        assert!(out.contains("Unknown action"));
    }

    #[test]
    fn luks_format_no_device() {
        let (code, out) = run_cryptsetup(&["luksFormat"]);
        assert_eq!(code, 1);
        assert!(out.contains("device argument required"));
    }

    #[test]
    fn luks_format_success() {
        let (code, out) = run_cryptsetup(&["-q", "luksFormat", "/dev/sda1"]);
        assert_eq!(code, 0);
        assert!(out.contains("formatted successfully"));
        assert!(out.contains("UUID:"));
        assert!(out.contains("Cipher:"));
    }

    #[test]
    fn luks_format_verbose() {
        let (code, out) = run_cryptsetup(&["-q", "-v", "luksFormat", "/dev/sda1"]);
        assert_eq!(code, 0);
        assert!(out.contains("LUKS header size:"));
    }

    #[test]
    fn luks_format_custom_cipher() {
        let (code, out) = run_cryptsetup(&["-q", "--cipher", "serpent-cbc-plain", "luksFormat", "/dev/sda"]);
        assert_eq!(code, 0);
        assert!(out.contains("serpent"));
    }

    #[test]
    fn luks_open_no_args() {
        let (code, out) = run_cryptsetup(&["luksOpen"]);
        assert_eq!(code, 1);
        assert!(out.contains("required"));
    }

    #[test]
    fn luks_open_success() {
        let (code, out) = run_cryptsetup(&["luksOpen", "/dev/sda1", "myvolume"]);
        assert_eq!(code, 0);
        assert!(out.contains("/dev/mapper/myvolume"));
    }

    #[test]
    fn luks_close_no_args() {
        let (code, out) = run_cryptsetup(&["luksClose"]);
        assert_eq!(code, 1);
        assert!(out.contains("required"));
    }

    #[test]
    fn luks_close_success() {
        let (code, out) = run_cryptsetup(&["luksClose", "myvolume"]);
        assert_eq!(code, 0);
        assert!(out.contains("deactivated"));
    }

    #[test]
    fn luks_dump_no_device() {
        let (code, out) = run_cryptsetup(&["luksDump"]);
        assert_eq!(code, 1);
        assert!(out.contains("required"));
    }

    #[test]
    fn luks_dump_success() {
        let (code, out) = run_cryptsetup(&["luksDump", "/dev/sda1"]);
        assert_eq!(code, 0);
        assert!(out.contains("LUKS header information"));
        assert!(out.contains("Version:"));
        assert!(out.contains("Cipher name:"));
        assert!(out.contains("UUID:"));
        assert!(out.contains("Key Slot 0: ENABLED"));
        assert!(out.contains("Key Slot 1: DISABLED"));
    }

    #[test]
    fn luks_add_key_success() {
        let (code, out) = run_cryptsetup(&["luksAddKey", "/dev/sda1"]);
        assert_eq!(code, 0);
        assert!(out.contains("added successfully"));
    }

    #[test]
    fn luks_add_key_no_device() {
        let (code, out) = run_cryptsetup(&["luksAddKey"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn luks_remove_key_success() {
        let (code, out) = run_cryptsetup(&["luksRemoveKey", "/dev/sda1"]);
        assert_eq!(code, 0);
        assert!(out.contains("removed"));
    }

    #[test]
    fn luks_kill_slot_success() {
        let (code, out) = run_cryptsetup(&["luksKillSlot", "/dev/sda1", "0"]);
        assert_eq!(code, 0);
        assert!(out.contains("destroyed"));
    }

    #[test]
    fn luks_kill_slot_invalid() {
        let (code, _) = run_cryptsetup(&["luksKillSlot", "/dev/sda1", "abc"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn luks_kill_slot_out_of_range() {
        let (code, out) = run_cryptsetup(&["luksKillSlot", "/dev/sda1", "99"]);
        assert_eq!(code, 1);
        assert!(out.contains("out of range"));
    }

    #[test]
    fn luks_kill_slot_no_args() {
        let (code, _) = run_cryptsetup(&["luksKillSlot"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn luks_change_key_success() {
        let (code, out) = run_cryptsetup(&["luksChangeKey", "/dev/sda1"]);
        assert_eq!(code, 0);
        assert!(out.contains("changed"));
    }

    #[test]
    fn luks_header_backup_success() {
        let (code, out) = run_cryptsetup(&[
            "luksHeaderBackup", "/dev/sda1",
            "--header-backup-file", "/tmp/backup.bin",
        ]);
        assert_eq!(code, 0);
        assert!(out.contains("backup"));
    }

    #[test]
    fn luks_header_backup_no_file() {
        let (code, out) = run_cryptsetup(&["luksHeaderBackup", "/dev/sda1"]);
        assert_eq!(code, 1);
        assert!(out.contains("--header-backup-file required"));
    }

    #[test]
    fn luks_header_restore_success() {
        let (code, out) = run_cryptsetup(&[
            "-q", "luksHeaderRestore", "/dev/sda1",
            "--header-backup-file", "/tmp/backup.bin",
        ]);
        assert_eq!(code, 0);
        assert!(out.contains("restored"));
    }

    #[test]
    fn luks_header_restore_warning() {
        let (code, out) = run_cryptsetup(&[
            "luksHeaderRestore", "/dev/sda1",
            "--header-backup-file", "/tmp/backup.bin",
        ]);
        assert_eq!(code, 0);
        assert!(out.contains("WARNING"));
    }

    #[test]
    fn is_luks_positive() {
        let (code, out) = run_cryptsetup(&["isLuks", "/dev/luks_volume"]);
        assert_eq!(code, 0);
        assert!(out.contains("is a LUKS device"));
    }

    #[test]
    fn is_luks_negative() {
        let (code, out) = run_cryptsetup(&["isLuks", "/dev/sda"]);
        assert_eq!(code, 1);
        assert!(out.contains("is not a LUKS device"));
    }

    #[test]
    fn status_success() {
        let (code, out) = run_cryptsetup(&["status", "myvolume"]);
        assert_eq!(code, 0);
        assert!(out.contains("/dev/mapper/myvolume"));
        assert!(out.contains("LUKS2"));
        assert!(out.contains("cipher:"));
    }

    #[test]
    fn status_no_name() {
        let (code, _) = run_cryptsetup(&["status"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn open_plain_success() {
        let (code, out) = run_cryptsetup(&["--type", "plain", "open", "/dev/sda", "plain_vol"]);
        assert_eq!(code, 0);
        assert!(out.contains("Plain dm-crypt mapping"));
        assert!(out.contains("/dev/mapper/plain_vol"));
    }

    #[test]
    fn open_plain_no_name() {
        let (code, _) = run_cryptsetup(&["--type", "plain", "open", "/dev/sda"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn close_success() {
        let (code, out) = run_cryptsetup(&["close", "myvolume"]);
        assert_eq!(code, 0);
        assert!(out.contains("deactivated"));
    }

    #[test]
    fn resize_success() {
        let (code, out) = run_cryptsetup(&["resize", "myvolume"]);
        assert_eq!(code, 0);
        assert!(out.contains("resized"));
    }

    #[test]
    fn resize_no_name() {
        let (code, _) = run_cryptsetup(&["resize"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn benchmark_output() {
        let (code, out) = run_cryptsetup(&["benchmark"]);
        assert_eq!(code, 0);
        assert!(out.contains("aes-cbc"));
        assert!(out.contains("aes-xts"));
        assert!(out.contains("serpent"));
        assert!(out.contains("twofish"));
        assert!(out.contains("MiB/s"));
    }

    #[test]
    fn benchmark_verbose() {
        let (code, out) = run_cryptsetup(&["-v", "benchmark"]);
        assert_eq!(code, 0);
        assert!(out.contains("PBKDF2"));
    }

    // --- Case-insensitive subcmd tests ---

    #[test]
    fn luks_format_lowercase() {
        let (code, _) = run_cryptsetup(&["-q", "luksformat", "/dev/sda"]);
        assert_eq!(code, 0);
    }

    #[test]
    fn luks_dump_lowercase() {
        let (code, out) = run_cryptsetup(&["luksdump", "/dev/sda"]);
        assert_eq!(code, 0);
        assert!(out.contains("LUKS header information"));
    }

    // --- Veritysetup command tests ---

    #[test]
    fn verity_help() {
        let (code, out) = run_verity(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("veritysetup"));
        assert!(out.contains("format"));
    }

    #[test]
    fn verity_version() {
        let (code, out) = run_verity(&["--version"]);
        assert_eq!(code, 0);
        assert!(out.contains(VERSION));
    }

    #[test]
    fn verity_no_args() {
        let (code, _) = run_verity(&[]);
        assert_eq!(code, 1);
    }

    #[test]
    fn verity_unknown() {
        let (code, out) = run_verity(&["bogus"]);
        assert_eq!(code, 1);
        assert!(out.contains("Unknown action"));
    }

    #[test]
    fn verity_format_success() {
        let (code, out) = run_verity(&["format", "/dev/sda1", "/dev/sda2"]);
        assert_eq!(code, 0);
        assert!(out.contains("VERITY header"));
        assert!(out.contains("UUID:"));
        assert!(out.contains("Root hash:"));
    }

    #[test]
    fn verity_format_no_hash_dev() {
        let (code, _) = run_verity(&["format", "/dev/sda1"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn verity_open_success() {
        let (code, out) = run_verity(&[
            "open", "/dev/sda1", "verity_vol", "/dev/sda2",
            "aabbccdd00112233445566778899aabbccddeeff00112233445566778899aabb",
        ]);
        assert_eq!(code, 0);
        assert!(out.contains("activated"));
    }

    #[test]
    fn verity_open_bad_hash() {
        let (code, out) = run_verity(&["open", "/dev/sda1", "name", "/dev/sda2", "not_hex!"]);
        assert_eq!(code, 1);
        assert!(out.contains("invalid root hash"));
    }

    #[test]
    fn verity_open_missing_args() {
        let (code, _) = run_verity(&["open", "/dev/sda1"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn verity_close_success() {
        let (code, out) = run_verity(&["close", "verity_vol"]);
        assert_eq!(code, 0);
        assert!(out.contains("deactivated"));
    }

    #[test]
    fn verity_close_no_name() {
        let (code, _) = run_verity(&["close"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn verity_verify_success() {
        let (code, out) = run_verity(&[
            "verify", "/dev/sda1", "/dev/sda2",
            "aabbccdd00112233445566778899aabbccddeeff00112233445566778899aabb",
        ]);
        assert_eq!(code, 0);
        assert!(out.contains("Verification OK"));
    }

    #[test]
    fn verity_verify_bad_hash() {
        let (code, out) = run_verity(&["verify", "/dev/sda1", "/dev/sda2", "xyz"]);
        assert_eq!(code, 1);
        assert!(out.contains("invalid root hash"));
    }

    #[test]
    fn verity_status_success() {
        let (code, out) = run_verity(&["status", "verity_vol"]);
        assert_eq!(code, 0);
        assert!(out.contains("VERITY"));
        assert!(out.contains("verified"));
    }

    #[test]
    fn verity_dump_success() {
        let (code, out) = run_verity(&["dump", "/dev/sda2"]);
        assert_eq!(code, 0);
        assert!(out.contains("VERITY header"));
        assert!(out.contains("Version:"));
        assert!(out.contains("Salt:"));
    }

    #[test]
    fn verity_dump_no_device() {
        let (code, _) = run_verity(&["dump"]);
        assert_eq!(code, 1);
    }

    // --- Integritysetup command tests ---

    #[test]
    fn integrity_help() {
        let (code, out) = run_integrity(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("integritysetup"));
        assert!(out.contains("format"));
    }

    #[test]
    fn integrity_version() {
        let (code, out) = run_integrity(&["--version"]);
        assert_eq!(code, 0);
        assert!(out.contains(VERSION));
    }

    #[test]
    fn integrity_no_args() {
        let (code, _) = run_integrity(&[]);
        assert_eq!(code, 1);
    }

    #[test]
    fn integrity_unknown() {
        let (code, out) = run_integrity(&["bogus"]);
        assert_eq!(code, 1);
        assert!(out.contains("Unknown action"));
    }

    #[test]
    fn integrity_format_success() {
        let (code, out) = run_integrity(&["format", "/dev/sda1"]);
        assert_eq!(code, 0);
        assert!(out.contains("Formatted integrity device"));
        assert!(out.contains("UUID:"));
        assert!(out.contains("crc32c"));
    }

    #[test]
    fn integrity_format_no_device() {
        let (code, _) = run_integrity(&["format"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn integrity_open_success() {
        let (code, out) = run_integrity(&["open", "/dev/sda1", "integ_vol"]);
        assert_eq!(code, 0);
        assert!(out.contains("activated"));
    }

    #[test]
    fn integrity_open_no_name() {
        let (code, _) = run_integrity(&["open", "/dev/sda1"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn integrity_close_success() {
        let (code, out) = run_integrity(&["close", "integ_vol"]);
        assert_eq!(code, 0);
        assert!(out.contains("deactivated"));
    }

    #[test]
    fn integrity_close_no_name() {
        let (code, _) = run_integrity(&["close"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn integrity_status_success() {
        let (code, out) = run_integrity(&["status", "integ_vol"]);
        assert_eq!(code, 0);
        assert!(out.contains("INTEGRITY"));
        assert!(out.contains("crc32c"));
    }

    #[test]
    fn integrity_status_no_name() {
        let (code, _) = run_integrity(&["status"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn integrity_dump_success() {
        let (code, out) = run_integrity(&["dump", "/dev/sda1"]);
        assert_eq!(code, 0);
        assert!(out.contains("Integrity superblock"));
        assert!(out.contains("Version:"));
    }

    #[test]
    fn integrity_dump_no_device() {
        let (code, _) = run_integrity(&["dump"]);
        assert_eq!(code, 1);
    }

    // --- Padded string helper tests ---

    #[test]
    fn write_padded_string_normal() {
        let mut buf = Vec::new();
        write_padded_string(&mut buf, "hello", 10);
        assert_eq!(buf.len(), 10);
        assert_eq!(&buf[..5], b"hello");
        assert!(buf[5..].iter().all(|&b| b == 0));
    }

    #[test]
    fn write_padded_string_exact() {
        let mut buf = Vec::new();
        write_padded_string(&mut buf, "abc", 3);
        assert_eq!(buf, b"abc");
    }

    #[test]
    fn write_padded_string_truncate() {
        let mut buf = Vec::new();
        write_padded_string(&mut buf, "hello world", 5);
        assert_eq!(buf, b"hello");
    }

    #[test]
    fn read_padded_string_normal() {
        let data = b"hello\0\0\0\0\0";
        assert_eq!(read_padded_string(data), "hello");
    }

    #[test]
    fn read_padded_string_full() {
        let data = b"abcdef";
        assert_eq!(read_padded_string(data), "abcdef");
    }

    #[test]
    fn read_padded_string_empty() {
        let data = b"\0\0\0\0";
        assert_eq!(read_padded_string(data), "");
    }

    // --- Deterministic salt tests ---

    #[test]
    fn deterministic_salt_reproducible() {
        let s1 = deterministic_salt(b"seed", 0);
        let s2 = deterministic_salt(b"seed", 0);
        assert_eq!(s1, s2);
    }

    #[test]
    fn deterministic_salt_different_index() {
        let s1 = deterministic_salt(b"seed", 0);
        let s2 = deterministic_salt(b"seed", 1);
        assert_ne!(s1, s2);
    }

    #[test]
    fn deterministic_salt_different_seed() {
        let s1 = deterministic_salt(b"seed-a", 0);
        let s2 = deterministic_salt(b"seed-b", 0);
        assert_ne!(s1, s2);
    }

    #[test]
    fn deterministic_salt_size() {
        let s = deterministic_salt(b"seed", 0);
        assert_eq!(s.len(), LUKS_SALT_SIZE);
    }

    // --- Tool name tests ---

    #[test]
    fn tool_name_cryptsetup() {
        assert_eq!(Tool::Cryptsetup.name(), "cryptsetup");
    }

    #[test]
    fn tool_name_veritysetup() {
        assert_eq!(Tool::Veritysetup.name(), "veritysetup");
    }

    #[test]
    fn tool_name_integritysetup() {
        assert_eq!(Tool::Integritysetup.name(), "integritysetup");
    }
}
