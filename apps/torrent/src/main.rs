//! `BitTorrent` client application
//!
//! Features:
//! - Bencode parser (encode/decode)
//! - .torrent file parsing (single and multi-file)
//! - SHA-1 info hash computation
//! - Peer wire protocol messages (BEP 3)
//! - Piece management with bitfield tracking
//! - Tracker announce/scrape (HTTP)
//! - Magnet link parsing (BEP 9)
//! - Download/upload speed tracking
//! - Bandwidth throttling
//! - Peer discovery and management
//! - Multi-tab UI with transfer list, details, peers, files, trackers

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]

use std::collections::BTreeMap;
use std::fmt;

// ─── Bencode ─────────────────────────────────────────────────────────

/// Bencode value types per BEP 3
#[derive(Debug, Clone, PartialEq)]
pub enum BencodeValue {
    Integer(i64),
    Bytes(Vec<u8>),
    List(Vec<BencodeValue>),
    Dict(BTreeMap<String, BencodeValue>),
}

impl BencodeValue {
    /// Try to get as integer
    #[must_use]
    pub fn as_int(&self) -> Option<i64> {
        if let Self::Integer(n) = self {
            Some(*n)
        } else {
            None
        }
    }

    /// Try to get as byte slice
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        if let Self::Bytes(b) = self {
            Some(b)
        } else {
            None
        }
    }

    /// Try to get as UTF-8 string
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        self.as_bytes().and_then(|b| std::str::from_utf8(b).ok())
    }

    /// Try to get as list
    #[must_use]
    pub fn as_list(&self) -> Option<&[BencodeValue]> {
        if let Self::List(l) = self {
            Some(l)
        } else {
            None
        }
    }

    /// Try to get as dict
    #[must_use]
    pub fn as_dict(&self) -> Option<&BTreeMap<String, BencodeValue>> {
        if let Self::Dict(d) = self {
            Some(d)
        } else {
            None
        }
    }
}

/// Bencode parser
pub struct BencodeParser;

impl BencodeParser {
    /// Parse a bencode value from bytes
    pub fn parse(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        if data.is_empty() {
            return Err("empty input".to_string());
        }

        match data.first() {
            Some(b'i') => Self::parse_integer(data),
            Some(b'l') => Self::parse_list(data),
            Some(b'd') => Self::parse_dict(data),
            Some(b'0'..=b'9') => Self::parse_bytes(data),
            Some(c) => Err(format!("unexpected byte: {c}")),
            None => Err("unexpected end of input".to_string()),
        }
    }

    fn parse_integer(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        // i<integer>e
        let end = data
            .iter()
            .position(|&b| b == b'e')
            .ok_or("unterminated integer")?;
        let num_str = std::str::from_utf8(data.get(1..end).unwrap_or_default())
            .map_err(|e| format!("invalid integer UTF-8: {e}"))?;
        let n: i64 = num_str
            .parse()
            .map_err(|e| format!("invalid integer: {e}"))?;
        Ok((BencodeValue::Integer(n), end.saturating_add(1)))
    }

    fn parse_bytes(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        // <length>:<content>
        let colon = data
            .iter()
            .position(|&b| b == b':')
            .ok_or("missing colon in byte string")?;
        let len_str = std::str::from_utf8(data.get(..colon).unwrap_or_default())
            .map_err(|e| format!("invalid length UTF-8: {e}"))?;
        let len: usize = len_str
            .parse()
            .map_err(|e| format!("invalid length: {e}"))?;
        let start = colon.saturating_add(1);
        let end = start.saturating_add(len);
        if end > data.len() {
            return Err("byte string extends past end of input".to_string());
        }
        let bytes = data.get(start..end).unwrap_or_default().to_vec();
        Ok((BencodeValue::Bytes(bytes), end))
    }

    fn parse_list(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        // l<values>e
        let mut items = Vec::new();
        let mut pos = 1; // skip 'l'
        loop {
            if pos >= data.len() {
                return Err("unterminated list".to_string());
            }
            if data.get(pos) == Some(&b'e') {
                return Ok((BencodeValue::List(items), pos.saturating_add(1)));
            }
            let (val, consumed) = Self::parse(data.get(pos..).unwrap_or_default())?;
            items.push(val);
            pos = pos.saturating_add(consumed);
        }
    }

    fn parse_dict(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        // d<key><value>...e
        let mut map = BTreeMap::new();
        let mut pos = 1; // skip 'd'
        loop {
            if pos >= data.len() {
                return Err("unterminated dict".to_string());
            }
            if data.get(pos) == Some(&b'e') {
                return Ok((BencodeValue::Dict(map), pos.saturating_add(1)));
            }
            // Key must be a byte string
            let (key_val, key_consumed) = Self::parse(data.get(pos..).unwrap_or_default())?;
            let key = match key_val {
                BencodeValue::Bytes(b) => {
                    String::from_utf8(b).map_err(|e| format!("dict key not UTF-8: {e}"))?
                }
                _ => return Err("dict key must be a byte string".to_string()),
            };
            pos = pos.saturating_add(key_consumed);
            let (val, val_consumed) = Self::parse(data.get(pos..).unwrap_or_default())?;
            map.insert(key, val);
            pos = pos.saturating_add(val_consumed);
        }
    }
}

/// Encode a bencode value to bytes
#[must_use]
pub fn bencode_encode(val: &BencodeValue) -> Vec<u8> {
    let mut out = Vec::new();
    bencode_encode_into(val, &mut out);
    out
}

fn bencode_encode_into(val: &BencodeValue, out: &mut Vec<u8>) {
    match val {
        BencodeValue::Integer(n) => {
            out.push(b'i');
            out.extend_from_slice(n.to_string().as_bytes());
            out.push(b'e');
        }
        BencodeValue::Bytes(b) => {
            out.extend_from_slice(b.len().to_string().as_bytes());
            out.push(b':');
            out.extend_from_slice(b);
        }
        BencodeValue::List(items) => {
            out.push(b'l');
            for item in items {
                bencode_encode_into(item, out);
            }
            out.push(b'e');
        }
        BencodeValue::Dict(map) => {
            out.push(b'd');
            for (key, val) in map {
                out.extend_from_slice(key.len().to_string().as_bytes());
                out.push(b':');
                out.extend_from_slice(key.as_bytes());
                bencode_encode_into(val, out);
            }
            out.push(b'e');
        }
    }
}

// ─── SHA-1 ───────────────────────────────────────────────────────────

/// Minimal SHA-1 implementation for info hash computation
pub struct Sha1 {
    h: [u32; 5],
    buffer: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha1 {
    const H0: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];

    #[must_use]
    pub fn new() -> Self {
        Self {
            h: Self::H0,
            buffer: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0;
        self.total_len = self.total_len.wrapping_add(data.len() as u64);

        // Fill buffer first
        if self.buf_len > 0 {
            let space = 64usize.saturating_sub(self.buf_len);
            let copy_len = space.min(data.len());
            if let (Some(dst), Some(src)) = (
                self.buffer
                    .get_mut(self.buf_len..self.buf_len.saturating_add(copy_len)),
                data.get(..copy_len),
            ) {
                dst.copy_from_slice(src);
            }
            self.buf_len = self.buf_len.saturating_add(copy_len);
            offset = copy_len;

            if self.buf_len == 64 {
                let block = self.buffer;
                self.process_block(&block);
                self.buf_len = 0;
            }
        }

        // Process full blocks
        while offset.saturating_add(64) <= data.len() {
            let mut block = [0u8; 64];
            if let Some(src) = data.get(offset..offset.saturating_add(64)) {
                block.copy_from_slice(src);
            }
            self.process_block(&block);
            offset = offset.saturating_add(64);
        }

        // Buffer remainder
        let remaining = data.len().saturating_sub(offset);
        if remaining > 0 {
            if let (Some(dst), Some(src)) = (self.buffer.get_mut(..remaining), data.get(offset..)) {
                dst.copy_from_slice(src);
            }
            self.buf_len = remaining;
        }
    }

    fn process_block(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16usize {
            let base = i.saturating_mul(4);
            w[i] = u32::from_be_bytes([
                block.get(base).copied().unwrap_or(0),
                block.get(base.saturating_add(1)).copied().unwrap_or(0),
                block.get(base.saturating_add(2)).copied().unwrap_or(0),
                block.get(base.saturating_add(3)).copied().unwrap_or(0),
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i.saturating_sub(3)]
                ^ w[i.saturating_sub(8)]
                ^ w[i.saturating_sub(14)]
                ^ w[i.saturating_sub(16)])
            .rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = self.h;

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDCu32),
                _ => (b ^ c ^ d, 0xCA62_C1D6u32),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
    }

    #[must_use]
    pub fn finalize(mut self) -> [u8; 20] {
        let bit_len = self.total_len.wrapping_mul(8);

        // Padding
        self.update(&[0x80]);
        while self.buf_len != 56 {
            self.update(&[0x00]);
        }
        self.update(&bit_len.to_be_bytes());

        let mut result = [0u8; 20];
        for (i, &h) in self.h.iter().enumerate() {
            let bytes = h.to_be_bytes();
            let base = i.saturating_mul(4);
            if let Some(dst) = result.get_mut(base..base.saturating_add(4)) {
                dst.copy_from_slice(&bytes);
            }
        }
        result
    }

    /// Compute SHA-1 of data in one call
    #[must_use]
    pub fn digest(data: &[u8]) -> [u8; 20] {
        let mut sha = Self::new();
        sha.update(data);
        sha.finalize()
    }
}

/// Format a SHA-1 hash as hex string
#[must_use]
pub fn hex_encode(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len().saturating_mul(2));
    for &b in data {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Parse hex string to bytes
#[must_use]
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut result = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_digit(bytes.get(i).copied().unwrap_or(0))?;
        let lo = hex_digit(bytes.get(i.saturating_add(1)).copied().unwrap_or(0))?;
        result.push(hi.wrapping_shl(4) | lo);
        i = i.saturating_add(2);
    }
    Some(result)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(b.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(b.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

// ─── URL encoding ────────────────────────────────────────────────────

/// URL-encode binary data (for tracker announces)
#[must_use]
pub fn url_encode_bytes(data: &[u8]) -> String {
    let mut out = String::new();
    for &b in data {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ─── Torrent Metadata ────────────────────────────────────────────────

/// A file within a torrent
#[derive(Debug, Clone)]
pub struct TorrentFile {
    pub path: String,
    pub length: u64,
    pub md5sum: Option<String>,
}

/// Torrent metadata parsed from .torrent file
#[derive(Debug, Clone)]
pub struct TorrentMetainfo {
    pub info_hash: [u8; 20],
    pub name: String,
    pub piece_length: u64,
    pub pieces: Vec<[u8; 20]>,
    pub files: Vec<TorrentFile>,
    pub total_size: u64,
    pub announce: String,
    pub announce_list: Vec<Vec<String>>,
    pub creation_date: Option<i64>,
    pub comment: Option<String>,
    pub created_by: Option<String>,
    pub is_private: bool,
}

impl TorrentMetainfo {
    /// Parse a .torrent file from bencode data
    pub fn from_bencode(data: &[u8]) -> Result<Self, String> {
        let (root, _) = BencodeParser::parse(data)?;
        let dict = root.as_dict().ok_or("torrent root must be a dict")?;

        let announce = dict
            .get("announce")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let announce_list = if let Some(al) = dict.get("announce-list").and_then(|v| v.as_list()) {
            al.iter()
                .filter_map(|tier| {
                    tier.as_list().map(|urls| {
                        urls.iter()
                            .filter_map(|u| u.as_str().map(String::from))
                            .collect()
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let creation_date = dict.get("creation date").and_then(BencodeValue::as_int);
        let comment = dict
            .get("comment")
            .and_then(|v| v.as_str())
            .map(String::from);
        let created_by = dict
            .get("created by")
            .and_then(|v| v.as_str())
            .map(String::from);

        let info = dict.get("info").ok_or("missing 'info' dict")?;
        let info_dict = info.as_dict().ok_or("info must be a dict")?;

        // Compute info hash from the bencoded info dict
        let info_bytes = bencode_encode(info);
        let info_hash = Sha1::digest(&info_bytes);

        let name = info_dict
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("missing torrent name")?
            .to_string();

        let piece_length = info_dict
            .get("piece length")
            .and_then(BencodeValue::as_int)
            .ok_or("missing piece length")? as u64;

        let pieces_bytes = info_dict
            .get("pieces")
            .and_then(|v| v.as_bytes())
            .ok_or("missing pieces")?;

        if pieces_bytes.len() % 20 != 0 {
            return Err("pieces length not multiple of 20".to_string());
        }

        let pieces: Vec<[u8; 20]> = pieces_bytes
            .chunks_exact(20)
            .map(|chunk| {
                let mut hash = [0u8; 20];
                hash.copy_from_slice(chunk);
                hash
            })
            .collect();

        let is_private = info_dict.get("private").and_then(BencodeValue::as_int) == Some(1);

        // Single file or multi-file?
        let files = if let Some(files_list) = info_dict.get("files").and_then(|v| v.as_list()) {
            // Multi-file torrent
            files_list
                .iter()
                .filter_map(|f| {
                    let fd = f.as_dict()?;
                    let length = fd.get("length")?.as_int()? as u64;
                    let path_parts: Vec<&str> = fd
                        .get("path")?
                        .as_list()?
                        .iter()
                        .filter_map(|p| p.as_str())
                        .collect();
                    let path = if path_parts.is_empty() {
                        "unknown".to_string()
                    } else {
                        path_parts.join("/")
                    };
                    let md5sum = fd.get("md5sum").and_then(|v| v.as_str()).map(String::from);
                    Some(TorrentFile {
                        path,
                        length,
                        md5sum,
                    })
                })
                .collect()
        } else {
            // Single file torrent
            let length = info_dict
                .get("length")
                .and_then(BencodeValue::as_int)
                .ok_or("missing file length")? as u64;
            let md5sum = info_dict
                .get("md5sum")
                .and_then(|v| v.as_str())
                .map(String::from);
            vec![TorrentFile {
                path: name.clone(),
                length,
                md5sum,
            }]
        };

        let total_size: u64 = files.iter().map(|f| f.length).sum();

        Ok(Self {
            info_hash,
            name,
            piece_length,
            pieces,
            files,
            total_size,
            announce,
            announce_list,
            creation_date,
            comment,
            created_by,
            is_private,
        })
    }

    /// Number of pieces
    #[must_use]
    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// Size of a specific piece (last piece may be smaller)
    #[must_use]
    pub fn piece_size(&self, index: usize) -> u64 {
        if index.saturating_add(1) < self.pieces.len() {
            self.piece_length
        } else {
            let remainder = self.total_size % self.piece_length;
            if remainder == 0 {
                self.piece_length
            } else {
                remainder
            }
        }
    }
}

// ─── Magnet Link Parsing ─────────────────────────────────────────────

/// Parsed magnet link (BEP 9)
#[derive(Debug, Clone)]
pub struct MagnetLink {
    pub info_hash: [u8; 20],
    pub display_name: Option<String>,
    pub trackers: Vec<String>,
    pub web_seeds: Vec<String>,
    pub exact_length: Option<u64>,
}

impl MagnetLink {
    /// Parse a magnet: URI
    pub fn parse(uri: &str) -> Result<Self, String> {
        if !uri.starts_with("magnet:?") {
            return Err("not a magnet URI".to_string());
        }

        let query = uri.get(8..).unwrap_or("");
        let mut info_hash = None;
        let mut display_name = None;
        let mut trackers = Vec::new();
        let mut web_seeds = Vec::new();
        let mut exact_length = None;

        for param in query.split('&') {
            let (key, value) = if let Some(eq) = param.find('=') {
                (
                    param.get(..eq).unwrap_or(""),
                    param.get(eq.saturating_add(1)..).unwrap_or(""),
                )
            } else {
                continue;
            };

            match key {
                "xt" => {
                    // urn:btih:<hex or base32>
                    if let Some(hash_str) = value.strip_prefix("urn:btih:") {
                        if hash_str.len() == 40 {
                            // Hex
                            let bytes = hex_decode(hash_str).ok_or("invalid hex in magnet")?;
                            if bytes.len() != 20 {
                                return Err("info hash must be 20 bytes".to_string());
                            }
                            let mut h = [0u8; 20];
                            h.copy_from_slice(&bytes);
                            info_hash = Some(h);
                        } else if hash_str.len() == 32 {
                            // Base32
                            let decoded =
                                base32_decode(hash_str).ok_or("invalid base32 in magnet")?;
                            if decoded.len() != 20 {
                                return Err("info hash must be 20 bytes".to_string());
                            }
                            let mut h = [0u8; 20];
                            h.copy_from_slice(&decoded);
                            info_hash = Some(h);
                        } else {
                            return Err(format!("unexpected info hash length: {}", hash_str.len()));
                        }
                    }
                }
                "dn" => {
                    display_name = Some(url_decode(value));
                }
                "tr" => {
                    trackers.push(url_decode(value));
                }
                "ws" => {
                    web_seeds.push(url_decode(value));
                }
                "xl" => {
                    exact_length = value.parse().ok();
                }
                _ => {} // Ignore unknown parameters
            }
        }

        let info_hash = info_hash.ok_or("missing info hash (xt=urn:btih:...)")?;

        Ok(Self {
            info_hash,
            display_name,
            trackers,
            web_seeds,
            exact_length,
        })
    }

    /// Generate magnet URI string
    #[must_use]
    pub fn to_uri(&self) -> String {
        let mut uri = format!("magnet:?xt=urn:btih:{}", hex_encode(&self.info_hash));
        if let Some(ref name) = self.display_name {
            uri.push_str(&format!("&dn={}", url_encode_str(name)));
        }
        for tr in &self.trackers {
            uri.push_str(&format!("&tr={}", url_encode_str(tr)));
        }
        uri
    }
}

/// Simple URL decoding
fn url_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes.get(i) == Some(&b'%')
            && i.saturating_add(2) < bytes.len()
            && let (Some(hi), Some(lo)) = (
                hex_digit(bytes.get(i.saturating_add(1)).copied().unwrap_or(0)),
                hex_digit(bytes.get(i.saturating_add(2)).copied().unwrap_or(0)),
            )
        {
            result.push(hi.wrapping_shl(4) | lo);
            i = i.saturating_add(3);
            continue;
        }
        if bytes.get(i) == Some(&b'+') {
            result.push(b' ');
        } else {
            result.push(bytes.get(i).copied().unwrap_or(0));
        }
        i = i.saturating_add(1);
    }
    String::from_utf8(result).unwrap_or_default()
}

/// Simple URL encoding for strings
fn url_encode_str(s: &str) -> String {
    url_encode_bytes(s.as_bytes())
}

/// Simple base32 decoder (RFC 4648)
fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let upper = s.to_uppercase();
    let input = upper.as_bytes();
    let mut bits: u64 = 0;
    let mut bit_count: u32 = 0;
    let mut result = Vec::new();

    for &b in input {
        if b == b'=' {
            break;
        }
        let val = alphabet.iter().position(|&c| c == b)? as u64;
        bits = bits.wrapping_shl(5) | val;
        bit_count = bit_count.saturating_add(5);
        if bit_count >= 8 {
            bit_count = bit_count.saturating_sub(8);
            result.push((bits >> bit_count) as u8);
            bits &= (1u64 << bit_count).wrapping_sub(1);
        }
    }

    Some(result)
}

// ─── Peer Wire Protocol ──────────────────────────────────────────────

/// Peer wire protocol message types (BEP 3)
#[derive(Debug, Clone, PartialEq)]
pub enum PeerMessage {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have {
        piece_index: u32,
    },
    Bitfield(Vec<u8>),
    Request {
        index: u32,
        begin: u32,
        length: u32,
    },
    Piece {
        index: u32,
        begin: u32,
        data: Vec<u8>,
    },
    Cancel {
        index: u32,
        begin: u32,
        length: u32,
    },
    Port {
        port: u16,
    },
    // BEP 10: Extension protocol
    Extended {
        id: u8,
        payload: Vec<u8>,
    },
}

impl PeerMessage {
    /// Message type ID
    #[must_use]
    pub fn message_id(&self) -> Option<u8> {
        match self {
            Self::KeepAlive => None,
            Self::Choke => Some(0),
            Self::Unchoke => Some(1),
            Self::Interested => Some(2),
            Self::NotInterested => Some(3),
            Self::Have { .. } => Some(4),
            Self::Bitfield(_) => Some(5),
            Self::Request { .. } => Some(6),
            Self::Piece { .. } => Some(7),
            Self::Cancel { .. } => Some(8),
            Self::Port { .. } => Some(9),
            Self::Extended { .. } => Some(20),
        }
    }

    /// Encode message to bytes (with 4-byte length prefix)
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::KeepAlive => vec![0, 0, 0, 0],
            Self::Choke | Self::Unchoke | Self::Interested | Self::NotInterested => {
                let id = self.message_id().unwrap_or(0);
                vec![0, 0, 0, 1, id]
            }
            Self::Have { piece_index } => {
                let mut buf = Vec::with_capacity(9);
                buf.extend_from_slice(&5u32.to_be_bytes());
                buf.push(4);
                buf.extend_from_slice(&piece_index.to_be_bytes());
                buf
            }
            Self::Bitfield(bits) => {
                let len = 1u32.saturating_add(bits.len() as u32);
                let mut buf = Vec::with_capacity(4usize.saturating_add(len as usize));
                buf.extend_from_slice(&len.to_be_bytes());
                buf.push(5);
                buf.extend_from_slice(bits);
                buf
            }
            Self::Request {
                index,
                begin,
                length,
            }
            | Self::Cancel {
                index,
                begin,
                length,
            } => {
                let id = self.message_id().unwrap_or(0);
                let mut buf = Vec::with_capacity(17);
                buf.extend_from_slice(&13u32.to_be_bytes());
                buf.push(id);
                buf.extend_from_slice(&index.to_be_bytes());
                buf.extend_from_slice(&begin.to_be_bytes());
                buf.extend_from_slice(&length.to_be_bytes());
                buf
            }
            Self::Piece { index, begin, data } => {
                let len = 9u32.saturating_add(data.len() as u32);
                let mut buf = Vec::with_capacity(4usize.saturating_add(len as usize));
                buf.extend_from_slice(&len.to_be_bytes());
                buf.push(7);
                buf.extend_from_slice(&index.to_be_bytes());
                buf.extend_from_slice(&begin.to_be_bytes());
                buf.extend_from_slice(data);
                buf
            }
            Self::Port { port } => {
                let mut buf = Vec::with_capacity(7);
                buf.extend_from_slice(&3u32.to_be_bytes());
                buf.push(9);
                buf.extend_from_slice(&port.to_be_bytes());
                buf
            }
            Self::Extended { id, payload } => {
                let len = 2u32.saturating_add(payload.len() as u32);
                let mut buf = Vec::with_capacity(4usize.saturating_add(len as usize));
                buf.extend_from_slice(&len.to_be_bytes());
                buf.push(20);
                buf.push(*id);
                buf.extend_from_slice(payload);
                buf
            }
        }
    }

    /// Decode a message from bytes (without length prefix, just payload)
    pub fn decode(data: &[u8]) -> Result<Self, String> {
        if data.is_empty() {
            return Ok(Self::KeepAlive);
        }

        let id = data.first().copied().unwrap_or(0);
        let payload = data.get(1..).unwrap_or_default();

        match id {
            0 => Ok(Self::Choke),
            1 => Ok(Self::Unchoke),
            2 => Ok(Self::Interested),
            3 => Ok(Self::NotInterested),
            4 => {
                if payload.len() < 4 {
                    return Err("have message too short".to_string());
                }
                let piece_index = u32::from_be_bytes([
                    payload.first().copied().unwrap_or(0),
                    payload.get(1).copied().unwrap_or(0),
                    payload.get(2).copied().unwrap_or(0),
                    payload.get(3).copied().unwrap_or(0),
                ]);
                Ok(Self::Have { piece_index })
            }
            5 => Ok(Self::Bitfield(payload.to_vec())),
            6 => {
                if payload.len() < 12 {
                    return Err("request message too short".to_string());
                }
                Ok(Self::Request {
                    index: u32::from_be_bytes([
                        payload.first().copied().unwrap_or(0),
                        payload.get(1).copied().unwrap_or(0),
                        payload.get(2).copied().unwrap_or(0),
                        payload.get(3).copied().unwrap_or(0),
                    ]),
                    begin: u32::from_be_bytes([
                        payload.get(4).copied().unwrap_or(0),
                        payload.get(5).copied().unwrap_or(0),
                        payload.get(6).copied().unwrap_or(0),
                        payload.get(7).copied().unwrap_or(0),
                    ]),
                    length: u32::from_be_bytes([
                        payload.get(8).copied().unwrap_or(0),
                        payload.get(9).copied().unwrap_or(0),
                        payload.get(10).copied().unwrap_or(0),
                        payload.get(11).copied().unwrap_or(0),
                    ]),
                })
            }
            7 => {
                if payload.len() < 8 {
                    return Err("piece message too short".to_string());
                }
                Ok(Self::Piece {
                    index: u32::from_be_bytes([
                        payload.first().copied().unwrap_or(0),
                        payload.get(1).copied().unwrap_or(0),
                        payload.get(2).copied().unwrap_or(0),
                        payload.get(3).copied().unwrap_or(0),
                    ]),
                    begin: u32::from_be_bytes([
                        payload.get(4).copied().unwrap_or(0),
                        payload.get(5).copied().unwrap_or(0),
                        payload.get(6).copied().unwrap_or(0),
                        payload.get(7).copied().unwrap_or(0),
                    ]),
                    data: payload.get(8..).unwrap_or_default().to_vec(),
                })
            }
            8 => {
                if payload.len() < 12 {
                    return Err("cancel message too short".to_string());
                }
                Ok(Self::Cancel {
                    index: u32::from_be_bytes([
                        payload.first().copied().unwrap_or(0),
                        payload.get(1).copied().unwrap_or(0),
                        payload.get(2).copied().unwrap_or(0),
                        payload.get(3).copied().unwrap_or(0),
                    ]),
                    begin: u32::from_be_bytes([
                        payload.get(4).copied().unwrap_or(0),
                        payload.get(5).copied().unwrap_or(0),
                        payload.get(6).copied().unwrap_or(0),
                        payload.get(7).copied().unwrap_or(0),
                    ]),
                    length: u32::from_be_bytes([
                        payload.get(8).copied().unwrap_or(0),
                        payload.get(9).copied().unwrap_or(0),
                        payload.get(10).copied().unwrap_or(0),
                        payload.get(11).copied().unwrap_or(0),
                    ]),
                })
            }
            9 => {
                if payload.len() < 2 {
                    return Err("port message too short".to_string());
                }
                Ok(Self::Port {
                    port: u16::from_be_bytes([
                        payload.first().copied().unwrap_or(0),
                        payload.get(1).copied().unwrap_or(0),
                    ]),
                })
            }
            20 => {
                let ext_id = payload.first().copied().unwrap_or(0);
                Ok(Self::Extended {
                    id: ext_id,
                    payload: payload.get(1..).unwrap_or_default().to_vec(),
                })
            }
            _ => Err(format!("unknown message id: {id}")),
        }
    }
}

/// Handshake message (not length-prefixed like other messages)
#[derive(Debug, Clone)]
pub struct Handshake {
    pub protocol: String,
    pub reserved: [u8; 8],
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

impl Handshake {
    /// Standard `BitTorrent` protocol name
    pub const PROTOCOL: &str = "BitTorrent protocol";

    /// Create a new handshake
    #[must_use]
    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        let mut reserved = [0u8; 8];
        // BEP 10: set extension protocol bit
        if let Some(byte) = reserved.get_mut(5) {
            *byte |= 0x10;
        }
        Self {
            protocol: Self::PROTOCOL.to_string(),
            reserved,
            info_hash,
            peer_id,
        }
    }

    /// Encode handshake to bytes
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let pstr = self.protocol.as_bytes();
        let mut buf = Vec::with_capacity(1usize.saturating_add(pstr.len()).saturating_add(48));
        buf.push(pstr.len() as u8);
        buf.extend_from_slice(pstr);
        buf.extend_from_slice(&self.reserved);
        buf.extend_from_slice(&self.info_hash);
        buf.extend_from_slice(&self.peer_id);
        buf
    }

    /// Decode handshake from bytes
    pub fn decode(data: &[u8]) -> Result<Self, String> {
        if data.is_empty() {
            return Err("empty handshake".to_string());
        }
        let pstr_len = data.first().copied().unwrap_or(0) as usize;
        let expected_len = 1usize.saturating_add(pstr_len).saturating_add(48);
        if data.len() < expected_len {
            return Err(format!(
                "handshake too short: {} < {expected_len}",
                data.len()
            ));
        }

        let protocol = std::str::from_utf8(
            data.get(1..1usize.saturating_add(pstr_len))
                .unwrap_or_default(),
        )
        .map_err(|e| format!("invalid protocol string: {e}"))?
        .to_string();

        let reserved_start = 1usize.saturating_add(pstr_len);
        let mut reserved = [0u8; 8];
        if let Some(src) = data.get(reserved_start..reserved_start.saturating_add(8)) {
            reserved.copy_from_slice(src);
        }

        let hash_start = reserved_start.saturating_add(8);
        let mut info_hash = [0u8; 20];
        if let Some(src) = data.get(hash_start..hash_start.saturating_add(20)) {
            info_hash.copy_from_slice(src);
        }

        let peer_start = hash_start.saturating_add(20);
        let mut peer_id = [0u8; 20];
        if let Some(src) = data.get(peer_start..peer_start.saturating_add(20)) {
            peer_id.copy_from_slice(src);
        }

        Ok(Self {
            protocol,
            reserved,
            info_hash,
            peer_id,
        })
    }

    /// Check if peer supports extension protocol (BEP 10)
    #[must_use]
    pub fn supports_extensions(&self) -> bool {
        self.reserved.get(5).is_some_and(|b| b & 0x10 != 0)
    }
}

// ─── Piece Management ────────────────────────────────────────────────

/// Track which pieces we have and need
#[derive(Debug, Clone)]
pub struct PieceTracker {
    /// Bitfield of completed pieces
    bitfield: Vec<u8>,
    /// Total number of pieces
    piece_count: usize,
    /// Number of completed pieces
    completed: usize,
    /// Pieces currently being downloaded
    in_progress: Vec<bool>,
    /// Per-piece priority (0 = skip, 1 = low, 5 = normal, 10 = high)
    priorities: Vec<u8>,
}

impl PieceTracker {
    #[must_use]
    pub fn new(piece_count: usize) -> Self {
        let byte_count = piece_count.saturating_add(7) / 8;
        Self {
            bitfield: vec![0u8; byte_count],
            piece_count,
            completed: 0,
            in_progress: vec![false; piece_count],
            priorities: vec![5; piece_count], // Normal priority
        }
    }

    /// Check if we have a piece
    #[must_use]
    pub fn has_piece(&self, index: usize) -> bool {
        let byte_idx = index / 8;
        let bit_idx = 7usize.saturating_sub(index % 8);
        self.bitfield
            .get(byte_idx)
            .is_some_and(|b| (b >> bit_idx) & 1 == 1)
    }

    /// Mark a piece as completed
    pub fn set_piece(&mut self, index: usize) {
        if index >= self.piece_count || self.has_piece(index) {
            return;
        }
        let byte_idx = index / 8;
        let bit_idx = 7usize.saturating_sub(index % 8);
        if let Some(byte) = self.bitfield.get_mut(byte_idx) {
            *byte |= 1u8 << bit_idx;
        }
        self.completed = self.completed.saturating_add(1);
        if let Some(ip) = self.in_progress.get_mut(index) {
            *ip = false;
        }
    }

    /// Mark piece as in-progress
    pub fn set_in_progress(&mut self, index: usize) {
        if let Some(ip) = self.in_progress.get_mut(index) {
            *ip = true;
        }
    }

    /// Clear in-progress flag (e.g., on peer disconnect)
    pub fn clear_in_progress(&mut self, index: usize) {
        if let Some(ip) = self.in_progress.get_mut(index) {
            *ip = false;
        }
    }

    /// Set piece priority
    pub fn set_priority(&mut self, index: usize, priority: u8) {
        if let Some(p) = self.priorities.get_mut(index) {
            *p = priority;
        }
    }

    /// Get our bitfield for sending to peers
    #[must_use]
    pub fn bitfield(&self) -> &[u8] {
        &self.bitfield
    }

    /// Number of completed pieces
    #[must_use]
    pub fn completed_count(&self) -> usize {
        self.completed
    }

    /// Total number of pieces
    #[must_use]
    pub fn total_count(&self) -> usize {
        self.piece_count
    }

    /// Is download complete?
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.completed == self.piece_count
    }

    /// Completion percentage
    #[must_use]
    pub fn progress(&self) -> f64 {
        if self.piece_count == 0 {
            return 100.0;
        }
        (self.completed as f64 / self.piece_count as f64) * 100.0
    }

    /// Pick next piece to download using rarest-first with peer availability
    #[must_use]
    pub fn pick_piece(&self, peer_bitfield: &[u8], availability: &[u32]) -> Option<usize> {
        let mut best_index = None;
        let mut best_avail = u32::MAX;
        let mut best_priority = 0u8;

        for i in 0..self.piece_count {
            // Skip pieces we already have, are in progress, or are set to skip priority
            if self.has_piece(i)
                || self.in_progress.get(i).copied().unwrap_or(false)
                || self.priorities.get(i).copied().unwrap_or(0) == 0
            {
                continue;
            }

            // Check if peer has this piece
            let byte_idx = i / 8;
            let bit_idx = 7usize.saturating_sub(i % 8);
            let peer_has = peer_bitfield
                .get(byte_idx)
                .is_some_and(|b| (b >> bit_idx) & 1 == 1);
            if !peer_has {
                continue;
            }

            let avail = availability.get(i).copied().unwrap_or(0);
            let priority = self.priorities.get(i).copied().unwrap_or(5);

            // Pick by priority first, then rarest
            if priority > best_priority || (priority == best_priority && avail < best_avail) {
                best_index = Some(i);
                best_avail = avail;
                best_priority = priority;
            }
        }

        best_index
    }
}

// ─── Peer Management ─────────────────────────────────────────────────

/// Peer connection state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PeerState {
    Connecting,
    Handshaking,
    Connected,
    Disconnected,
}

impl fmt::Display for PeerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connecting => write!(f, "Connecting"),
            Self::Handshaking => write!(f, "Handshaking"),
            Self::Connected => write!(f, "Connected"),
            Self::Disconnected => write!(f, "Disconnected"),
        }
    }
}

/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub address: String,
    pub port: u16,
    pub peer_id: Option<[u8; 20]>,
    pub client_name: String,
    pub state: PeerState,
    pub am_choking: bool,
    pub am_interested: bool,
    pub peer_choking: bool,
    pub peer_interested: bool,
    pub bitfield: Vec<u8>,
    pub download_rate: u64, // bytes per second
    pub upload_rate: u64,   // bytes per second
    pub downloaded: u64,    // total bytes downloaded from this peer
    pub uploaded: u64,      // total bytes uploaded to this peer
    pub requests_pending: u32,
    pub connection_time: u64, // seconds since connected
    pub country: String,
    pub supports_extensions: bool,
}

impl PeerInfo {
    #[must_use]
    pub fn new(address: &str, port: u16) -> Self {
        Self {
            address: address.to_string(),
            port,
            peer_id: None,
            client_name: "Unknown".to_string(),
            state: PeerState::Connecting,
            am_choking: true,
            am_interested: false,
            peer_choking: true,
            peer_interested: false,
            bitfield: Vec::new(),
            download_rate: 0,
            upload_rate: 0,
            downloaded: 0,
            uploaded: 0,
            requests_pending: 0,
            connection_time: 0,
            country: String::new(),
            supports_extensions: false,
        }
    }

    /// Identify client from peer ID (Azureus-style and Shadow-style)
    #[must_use]
    pub fn identify_client(peer_id: &[u8; 20]) -> String {
        // Azureus-style: -XX0000-
        if peer_id.first() == Some(&b'-') && peer_id.get(7) == Some(&b'-') {
            let client_code =
                std::str::from_utf8(peer_id.get(1..3).unwrap_or_default()).unwrap_or("??");
            let version =
                std::str::from_utf8(peer_id.get(3..7).unwrap_or_default()).unwrap_or("????");
            let name = match client_code {
                "qB" => "qBittorrent",
                "UT" => "µTorrent",
                "TR" => "Transmission",
                "DE" => "Deluge",
                "AZ" => "Azureus/Vuze",
                "LT" => "libtorrent",
                "lt" => "libtorrent (rasterbar)",
                "BC" => "BitComet",
                "BT" => "BitTorrent",
                "KT" => "KTorrent",
                "RB" => "RuBitTorrent",
                "FL" => "Flashget",
                "BI" => "BiglyBT",
                _ => client_code,
            };
            return format!("{name} {version}");
        }

        // Shadow-style: single letter + 3 digit version
        if let Some(&first) = peer_id.first()
            && first.is_ascii_alphabetic()
        {
            let name = match first {
                b'A' => "ABC",
                b'M' => "Mainline",
                b'O' => "Osprey",
                b'S' => "Shadow",
                b'T' => "BitTornado",
                _ => "Unknown",
            };
            return name.to_string();
        }

        "Unknown".to_string()
    }
}

// ─── Tracker Communication ───────────────────────────────────────────

/// Tracker announce event types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackerEvent {
    None,
    Started,
    Stopped,
    Completed,
}

impl fmt::Display for TrackerEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, ""),
            Self::Started => write!(f, "started"),
            Self::Stopped => write!(f, "stopped"),
            Self::Completed => write!(f, "completed"),
        }
    }
}

/// Tracker announce request parameters
#[derive(Debug, Clone)]
pub struct AnnounceRequest {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub compact: bool,
    pub event: TrackerEvent,
    pub numwant: Option<u32>,
}

impl AnnounceRequest {
    /// Build announce URL with query parameters
    #[must_use]
    pub fn build_url(&self, tracker_url: &str) -> String {
        let sep = if tracker_url.contains('?') { "&" } else { "?" };
        let mut url = format!(
            "{tracker_url}{sep}info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={}&compact={}",
            url_encode_bytes(&self.info_hash),
            url_encode_bytes(&self.peer_id),
            self.port,
            self.uploaded,
            self.downloaded,
            self.left,
            i32::from(self.compact),
        );

        if self.event != TrackerEvent::None {
            url.push_str(&format!("&event={}", self.event));
        }

        if let Some(numwant) = self.numwant {
            url.push_str(&format!("&numwant={numwant}"));
        }

        url
    }
}

/// Parsed tracker announce response
#[derive(Debug, Clone)]
pub struct AnnounceResponse {
    pub interval: u64,
    pub min_interval: Option<u64>,
    pub tracker_id: Option<String>,
    pub complete: u32,             // seeders
    pub incomplete: u32,           // leechers
    pub peers: Vec<(String, u16)>, // (ip, port)
    pub warning_message: Option<String>,
    pub failure_reason: Option<String>,
}

impl AnnounceResponse {
    /// Parse tracker response from bencode
    pub fn from_bencode(data: &[u8]) -> Result<Self, String> {
        let (root, _) = BencodeParser::parse(data)?;
        let dict = root.as_dict().ok_or("tracker response must be a dict")?;

        if let Some(failure) = dict.get("failure reason").and_then(|v| v.as_str()) {
            return Ok(Self {
                interval: 0,
                min_interval: None,
                tracker_id: None,
                complete: 0,
                incomplete: 0,
                peers: Vec::new(),
                warning_message: None,
                failure_reason: Some(failure.to_string()),
            });
        }

        let interval = dict
            .get("interval")
            .and_then(BencodeValue::as_int)
            .unwrap_or(1800) as u64;
        let min_interval = dict
            .get("min interval")
            .and_then(BencodeValue::as_int)
            .map(|n| n as u64);
        let tracker_id = dict
            .get("tracker id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let complete = dict
            .get("complete")
            .and_then(BencodeValue::as_int)
            .unwrap_or(0) as u32;
        let incomplete = dict
            .get("incomplete")
            .and_then(BencodeValue::as_int)
            .unwrap_or(0) as u32;
        let warning_message = dict
            .get("warning message")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Parse peers: compact (binary) or dict format
        let peers = if let Some(peers_bytes) = dict.get("peers").and_then(|v| v.as_bytes()) {
            // Compact format: 6 bytes per peer (4 IP + 2 port)
            peers_bytes
                .chunks_exact(6)
                .map(|chunk| {
                    let ip = format!(
                        "{}.{}.{}.{}",
                        chunk.first().copied().unwrap_or(0),
                        chunk.get(1).copied().unwrap_or(0),
                        chunk.get(2).copied().unwrap_or(0),
                        chunk.get(3).copied().unwrap_or(0),
                    );
                    let port = u16::from_be_bytes([
                        chunk.get(4).copied().unwrap_or(0),
                        chunk.get(5).copied().unwrap_or(0),
                    ]);
                    (ip, port)
                })
                .collect()
        } else if let Some(peers_list) = dict.get("peers").and_then(|v| v.as_list()) {
            // Dict format
            peers_list
                .iter()
                .filter_map(|p| {
                    let pd = p.as_dict()?;
                    let ip = pd.get("ip")?.as_str()?.to_string();
                    let port = pd.get("port")?.as_int()? as u16;
                    Some((ip, port))
                })
                .collect()
        } else {
            Vec::new()
        };

        Ok(Self {
            interval,
            min_interval,
            tracker_id,
            complete,
            incomplete,
            peers,
            warning_message,
            failure_reason: None,
        })
    }
}

/// Tracker scrape response for a single torrent
#[derive(Debug, Clone)]
pub struct ScrapeInfo {
    pub complete: u32,
    pub incomplete: u32,
    pub downloaded: u32,
    pub name: Option<String>,
}

// ─── Torrent State ───────────────────────────────────────────────────

/// Overall torrent download state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TorrentState {
    Queued,
    CheckingFiles,
    Downloading,
    Seeding,
    Paused,
    Error,
    Complete,
    Allocating,
    Metadata, // For magnet links, waiting for metadata
}

impl fmt::Display for TorrentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queued => write!(f, "Queued"),
            Self::CheckingFiles => write!(f, "Checking"),
            Self::Downloading => write!(f, "Downloading"),
            Self::Seeding => write!(f, "Seeding"),
            Self::Paused => write!(f, "Paused"),
            Self::Error => write!(f, "Error"),
            Self::Complete => write!(f, "Complete"),
            Self::Allocating => write!(f, "Allocating"),
            Self::Metadata => write!(f, "Metadata"),
        }
    }
}

/// Speed tracking with rolling average
#[derive(Debug, Clone)]
pub struct SpeedTracker {
    samples: Vec<u64>,
    sample_interval_ms: u64,
    max_samples: usize,
    total_bytes: u64,
}

impl SpeedTracker {
    #[must_use]
    pub fn new(max_samples: usize, interval_ms: u64) -> Self {
        Self {
            samples: Vec::with_capacity(max_samples),
            sample_interval_ms: interval_ms,
            max_samples,
            total_bytes: 0,
        }
    }

    /// Record bytes transferred in the current interval
    pub fn add_sample(&mut self, bytes: u64) {
        if self.samples.len() >= self.max_samples {
            self.samples.remove(0);
        }
        self.samples.push(bytes);
        self.total_bytes = self.total_bytes.saturating_add(bytes);
    }

    /// Current speed in bytes per second
    #[must_use]
    pub fn speed_bps(&self) -> u64 {
        if self.samples.is_empty() || self.sample_interval_ms == 0 {
            return 0;
        }
        let sum: u64 = self.samples.iter().sum();
        let duration_s = (self.samples.len() as u64).saturating_mul(self.sample_interval_ms) / 1000;
        if duration_s == 0 {
            return 0;
        }
        sum / duration_s
    }

    /// Total bytes transferred
    #[must_use]
    pub fn total(&self) -> u64 {
        self.total_bytes
    }
}

/// Bandwidth limiter
#[derive(Debug, Clone)]
pub struct BandwidthLimiter {
    /// Maximum bytes per second (0 = unlimited)
    pub limit_bps: u64,
    /// Bytes used in current second
    bytes_used: u64,
    /// Timestamp of current second
    current_second: u64,
}

impl BandwidthLimiter {
    #[must_use]
    pub fn new(limit_bps: u64) -> Self {
        Self {
            limit_bps,
            bytes_used: 0,
            current_second: 0,
        }
    }

    /// Request to send/receive `count` bytes. Returns how many are allowed.
    pub fn request(&mut self, count: u64, now_seconds: u64) -> u64 {
        if self.limit_bps == 0 {
            return count; // Unlimited
        }

        if now_seconds != self.current_second {
            self.current_second = now_seconds;
            self.bytes_used = 0;
        }

        let remaining = self.limit_bps.saturating_sub(self.bytes_used);
        let allowed = count.min(remaining);
        self.bytes_used = self.bytes_used.saturating_add(allowed);
        allowed
    }

    /// Set new limit
    pub fn set_limit(&mut self, limit_bps: u64) {
        self.limit_bps = limit_bps;
    }

    /// Check if unlimited
    #[must_use]
    pub fn is_unlimited(&self) -> bool {
        self.limit_bps == 0
    }
}

// ─── File Priority and Selection ─────────────────────────────────────

/// Priority for individual files within a torrent
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilePriority {
    Skip,
    Low,
    Normal,
    High,
}

impl fmt::Display for FilePriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Skip => write!(f, "Skip"),
            Self::Low => write!(f, "Low"),
            Self::Normal => write!(f, "Normal"),
            Self::High => write!(f, "High"),
        }
    }
}

/// Managed torrent with all state
#[derive(Debug, Clone)]
pub struct ManagedTorrent {
    pub id: u32,
    pub metainfo: Option<TorrentMetainfo>,
    pub magnet: Option<MagnetLink>,
    pub name: String,
    pub state: TorrentState,
    pub pieces: PieceTracker,
    pub peers: Vec<PeerInfo>,
    pub trackers: Vec<TrackerEntry>,
    pub download_speed: SpeedTracker,
    pub upload_speed: SpeedTracker,
    pub downloaded: u64,
    pub uploaded: u64,
    pub total_size: u64,
    pub save_path: String,
    pub added_time: u64,
    pub completed_time: Option<u64>,
    pub file_priorities: Vec<FilePriority>,
    pub download_limit: BandwidthLimiter,
    pub upload_limit: BandwidthLimiter,
    pub seed_ratio_limit: Option<f64>,
    pub sequential_download: bool,
    pub error_message: Option<String>,
    pub label: String,
}

/// Tracker entry with status
#[derive(Debug, Clone)]
pub struct TrackerEntry {
    pub url: String,
    pub tier: u32,
    pub status: TrackerStatus,
    pub seeders: u32,
    pub leechers: u32,
    pub last_announce: Option<u64>,
    pub next_announce: Option<u64>,
    pub announce_count: u32,
    pub error_message: Option<String>,
}

/// Tracker status
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackerStatus {
    NotContacted,
    Working,
    Updating,
    Error,
    Disabled,
}

impl fmt::Display for TrackerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotContacted => write!(f, "Not contacted"),
            Self::Working => write!(f, "Working"),
            Self::Updating => write!(f, "Updating"),
            Self::Error => write!(f, "Error"),
            Self::Disabled => write!(f, "Disabled"),
        }
    }
}

impl ManagedTorrent {
    /// Create from a parsed torrent file
    #[must_use]
    pub fn from_metainfo(id: u32, meta: TorrentMetainfo, save_path: &str) -> Self {
        let piece_count = meta.piece_count();
        let file_count = meta.files.len();
        let total_size = meta.total_size;
        let name = meta.name.clone();

        let trackers = if meta.announce_list.is_empty() {
            vec![TrackerEntry {
                url: meta.announce.clone(),
                tier: 0,
                status: TrackerStatus::NotContacted,
                seeders: 0,
                leechers: 0,
                last_announce: None,
                next_announce: None,
                announce_count: 0,
                error_message: None,
            }]
        } else {
            meta.announce_list
                .iter()
                .enumerate()
                .flat_map(|(tier, urls)| {
                    urls.iter().map(move |url| TrackerEntry {
                        url: url.clone(),
                        tier: tier as u32,
                        status: TrackerStatus::NotContacted,
                        seeders: 0,
                        leechers: 0,
                        last_announce: None,
                        next_announce: None,
                        announce_count: 0,
                        error_message: None,
                    })
                })
                .collect()
        };

        Self {
            id,
            metainfo: Some(meta),
            magnet: None,
            name,
            state: TorrentState::Queued,
            pieces: PieceTracker::new(piece_count),
            peers: Vec::new(),
            trackers,
            download_speed: SpeedTracker::new(30, 1000),
            upload_speed: SpeedTracker::new(30, 1000),
            downloaded: 0,
            uploaded: 0,
            total_size,
            save_path: save_path.to_string(),
            added_time: 0,
            completed_time: None,
            file_priorities: vec![FilePriority::Normal; file_count],
            download_limit: BandwidthLimiter::new(0),
            upload_limit: BandwidthLimiter::new(0),
            seed_ratio_limit: None,
            sequential_download: false,
            error_message: None,
            label: String::new(),
        }
    }

    /// Create from a magnet link
    #[must_use]
    pub fn from_magnet(id: u32, magnet: MagnetLink, save_path: &str) -> Self {
        let name = magnet.display_name.clone().unwrap_or_else(|| {
            hex_encode(&magnet.info_hash)
                .get(..16)
                .unwrap_or("unknown")
                .to_string()
        });

        let trackers: Vec<TrackerEntry> = magnet
            .trackers
            .iter()
            .enumerate()
            .map(|(i, url)| TrackerEntry {
                url: url.clone(),
                tier: i as u32,
                status: TrackerStatus::NotContacted,
                seeders: 0,
                leechers: 0,
                last_announce: None,
                next_announce: None,
                announce_count: 0,
                error_message: None,
            })
            .collect();

        Self {
            id,
            metainfo: None,
            magnet: Some(magnet),
            name,
            state: TorrentState::Metadata,
            pieces: PieceTracker::new(0),
            peers: Vec::new(),
            trackers,
            download_speed: SpeedTracker::new(30, 1000),
            upload_speed: SpeedTracker::new(30, 1000),
            downloaded: 0,
            uploaded: 0,
            total_size: 0,
            save_path: save_path.to_string(),
            added_time: 0,
            completed_time: None,
            file_priorities: Vec::new(),
            download_limit: BandwidthLimiter::new(0),
            upload_limit: BandwidthLimiter::new(0),
            seed_ratio_limit: None,
            sequential_download: false,
            error_message: None,
            label: String::new(),
        }
    }

    /// Progress percentage
    #[must_use]
    pub fn progress(&self) -> f64 {
        if self.total_size == 0 {
            return 0.0;
        }
        (self.downloaded as f64 / self.total_size as f64) * 100.0
    }

    /// Share ratio
    #[must_use]
    pub fn ratio(&self) -> f64 {
        if self.downloaded == 0 {
            return 0.0;
        }
        self.uploaded as f64 / self.downloaded as f64
    }

    /// ETA in seconds based on current download speed
    #[must_use]
    pub fn eta_seconds(&self) -> Option<u64> {
        let speed = self.download_speed.speed_bps();
        if speed == 0 {
            return None;
        }
        let remaining = self.total_size.saturating_sub(self.downloaded);
        Some(remaining / speed)
    }

    /// Pause the torrent
    pub fn pause(&mut self) {
        if self.state == TorrentState::Downloading || self.state == TorrentState::Seeding {
            self.state = TorrentState::Paused;
        }
    }

    /// Resume the torrent
    pub fn resume(&mut self) {
        if self.state == TorrentState::Paused {
            self.state = if self.pieces.is_complete() {
                TorrentState::Seeding
            } else {
                TorrentState::Downloading
            };
        }
    }

    /// Toggle sequential download mode
    pub fn toggle_sequential(&mut self) {
        self.sequential_download = !self.sequential_download;
    }
}

// ─── Client Settings ─────────────────────────────────────────────────

/// Global client settings
#[derive(Debug, Clone)]
pub struct ClientSettings {
    pub listen_port: u16,
    pub max_active_downloads: u32,
    pub max_active_seeds: u32,
    pub max_active_torrents: u32,
    pub max_connections_global: u32,
    pub max_connections_per_torrent: u32,
    pub max_uploads_per_torrent: u32,
    pub global_download_limit: u64, // bytes/s, 0 = unlimited
    pub global_upload_limit: u64,
    pub default_save_path: String,
    pub dht_enabled: bool,
    pub pex_enabled: bool, // Peer exchange
    pub lsd_enabled: bool, // Local service discovery
    pub encryption_mode: EncryptionMode,
    pub seed_ratio_limit: Option<f64>,
    pub seed_time_limit: Option<u64>, // seconds
    pub pre_allocate_storage: bool,
    pub check_hash_on_completion: bool,
    pub enable_utp: bool, // µTP (micro Transport Protocol)
    pub proxy_type: ProxyType,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub scheduler_enabled: bool,
    pub schedule_download_rate: u64,
    pub schedule_upload_rate: u64,
}

/// Encryption mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EncryptionMode {
    Disabled,
    Prefer,
    Require,
}

impl fmt::Display for EncryptionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => write!(f, "Disabled"),
            Self::Prefer => write!(f, "Prefer"),
            Self::Require => write!(f, "Require"),
        }
    }
}

/// Proxy type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProxyType {
    None,
    Socks4,
    Socks5,
    Http,
}

impl fmt::Display for ProxyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Socks4 => write!(f, "SOCKS4"),
            Self::Socks5 => write!(f, "SOCKS5"),
            Self::Http => write!(f, "HTTP"),
        }
    }
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            listen_port: 6881,
            max_active_downloads: 5,
            max_active_seeds: 10,
            max_active_torrents: 15,
            max_connections_global: 500,
            max_connections_per_torrent: 100,
            max_uploads_per_torrent: 8,
            global_download_limit: 0,
            global_upload_limit: 0,
            default_save_path: "/home/user/Downloads".to_string(),
            dht_enabled: true,
            pex_enabled: true,
            lsd_enabled: true,
            encryption_mode: EncryptionMode::Prefer,
            seed_ratio_limit: Some(2.0),
            seed_time_limit: None,
            pre_allocate_storage: false,
            check_hash_on_completion: true,
            enable_utp: true,
            proxy_type: ProxyType::None,
            proxy_host: String::new(),
            proxy_port: 0,
            scheduler_enabled: false,
            schedule_download_rate: 0,
            schedule_upload_rate: 0,
        }
    }
}

// ─── Application ─────────────────────────────────────────────────────

use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

/// Catppuccin Mocha palette
mod colors {
    use guitk::Color;
    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
}

/// Active UI tab
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tab {
    Transfers,
    Details,
    Peers,
    Files,
    Trackers,
    Settings,
}

impl Tab {
    pub const ALL: [Self; 6] = [
        Self::Transfers,
        Self::Details,
        Self::Peers,
        Self::Files,
        Self::Trackers,
        Self::Settings,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Transfers => "Transfers",
            Self::Details => "Details",
            Self::Peers => "Peers",
            Self::Files => "Files",
            Self::Trackers => "Trackers",
            Self::Settings => "Settings",
        }
    }
}

/// Main torrent client application
pub struct TorrentApp {
    pub torrents: Vec<ManagedTorrent>,
    pub settings: ClientSettings,
    pub active_tab: Tab,
    pub selected_torrent: Option<u32>,
    pub next_id: u32,
    pub peer_id: [u8; 20],
    pub search_query: String,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    pub filter: TorrentFilter,
    pub global_download_speed: SpeedTracker,
    pub global_upload_speed: SpeedTracker,
    pub show_add_dialog: bool,
    pub add_url_input: String,
    pub add_save_path: String,
    pub status_message: String,
    pub labels: Vec<String>,
    pub selected_label: Option<String>,
}

/// Column for sorting
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortColumn {
    Name,
    Size,
    Progress,
    Status,
    DownSpeed,
    UpSpeed,
    Ratio,
    Eta,
    Seeds,
    Peers,
    Added,
}

/// Filter for torrent list
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TorrentFilter {
    All,
    Downloading,
    Seeding,
    Completed,
    Paused,
    Active,
    Error,
}

impl TorrentFilter {
    #[must_use]
    pub fn matches(self, state: TorrentState) -> bool {
        match self {
            Self::All => true,
            Self::Downloading => state == TorrentState::Downloading,
            Self::Seeding => state == TorrentState::Seeding,
            Self::Completed => state == TorrentState::Complete || state == TorrentState::Seeding,
            Self::Paused => state == TorrentState::Paused,
            Self::Active => state == TorrentState::Downloading || state == TorrentState::Seeding,
            Self::Error => state == TorrentState::Error,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Downloading => "Downloading",
            Self::Seeding => "Seeding",
            Self::Completed => "Completed",
            Self::Paused => "Paused",
            Self::Active => "Active",
            Self::Error => "Error",
        }
    }
}

impl Default for TorrentApp {
    fn default() -> Self {
        Self::new()
    }
}

impl TorrentApp {
    #[must_use]
    pub fn new() -> Self {
        // Generate peer ID: -OT0100- + 12 random chars (OT = OurTorrent)
        let mut peer_id = [0u8; 20];
        peer_id[..8].copy_from_slice(b"-OT0100-");
        // Fill remainder with deterministic-looking bytes for now
        for i in 8..20 {
            peer_id[i] = ((i as u8).wrapping_mul(37)).wrapping_add(42);
        }

        Self {
            torrents: Vec::new(),
            settings: ClientSettings::default(),
            active_tab: Tab::Transfers,
            selected_torrent: None,
            next_id: 1,
            peer_id,
            search_query: String::new(),
            sort_column: SortColumn::Added,
            sort_ascending: false,
            filter: TorrentFilter::All,
            global_download_speed: SpeedTracker::new(60, 1000),
            global_upload_speed: SpeedTracker::new(60, 1000),
            show_add_dialog: false,
            add_url_input: String::new(),
            add_save_path: ClientSettings::default().default_save_path.clone(),
            status_message: "Ready".to_string(),
            labels: vec![
                "Movies".to_string(),
                "Music".to_string(),
                "Software".to_string(),
                "Games".to_string(),
                "Books".to_string(),
            ],
            selected_label: None,
        }
    }

    /// Add a torrent from parsed metainfo
    pub fn add_torrent(&mut self, meta: TorrentMetainfo, save_path: Option<&str>) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let path = save_path.unwrap_or(&self.settings.default_save_path);
        let torrent = ManagedTorrent::from_metainfo(id, meta, path);
        self.status_message = format!("Added: {}", torrent.name);
        self.torrents.push(torrent);
        id
    }

    /// Add a torrent from a magnet link
    pub fn add_magnet(&mut self, magnet: MagnetLink, save_path: Option<&str>) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let path = save_path.unwrap_or(&self.settings.default_save_path);
        let torrent = ManagedTorrent::from_magnet(id, magnet, path);
        self.status_message = format!("Added magnet: {}", torrent.name);
        self.torrents.push(torrent);
        id
    }

    /// Remove a torrent by ID
    pub fn remove_torrent(&mut self, id: u32, delete_files: bool) {
        if let Some(pos) = self.torrents.iter().position(|t| t.id == id) {
            let name = self
                .torrents
                .get(pos)
                .map_or("Unknown", |t| &t.name)
                .to_string();
            self.torrents.remove(pos);
            if self.selected_torrent == Some(id) {
                self.selected_torrent = None;
            }
            self.status_message = if delete_files {
                format!("Removed and deleted: {name}")
            } else {
                format!("Removed: {name}")
            };
        }
    }

    /// Pause a torrent
    pub fn pause_torrent(&mut self, id: u32) {
        if let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) {
            t.pause();
        }
    }

    /// Resume a torrent
    pub fn resume_torrent(&mut self, id: u32) {
        if let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) {
            t.resume();
        }
    }

    /// Pause all torrents
    pub fn pause_all(&mut self) {
        for t in &mut self.torrents {
            t.pause();
        }
        self.status_message = "All torrents paused".to_string();
    }

    /// Resume all torrents
    pub fn resume_all(&mut self) {
        for t in &mut self.torrents {
            t.resume();
        }
        self.status_message = "All torrents resumed".to_string();
    }

    /// Get filtered and sorted torrent list
    #[must_use]
    pub fn filtered_torrents(&self) -> Vec<&ManagedTorrent> {
        let mut list: Vec<&ManagedTorrent> = self
            .torrents
            .iter()
            .filter(|t| self.filter.matches(t.state))
            .filter(|t| {
                if self.search_query.is_empty() {
                    true
                } else {
                    let q = self.search_query.to_lowercase();
                    t.name.to_lowercase().contains(&q) || t.label.to_lowercase().contains(&q)
                }
            })
            .filter(|t| {
                if let Some(ref label) = self.selected_label {
                    &t.label == label
                } else {
                    true
                }
            })
            .collect();

        list.sort_by(|a, b| {
            let cmp = match self.sort_column {
                SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortColumn::Size => a.total_size.cmp(&b.total_size),
                SortColumn::Progress => a
                    .progress()
                    .partial_cmp(&b.progress())
                    .unwrap_or(std::cmp::Ordering::Equal),
                SortColumn::Status => (a.state as u8).cmp(&(b.state as u8)),
                SortColumn::DownSpeed => a
                    .download_speed
                    .speed_bps()
                    .cmp(&b.download_speed.speed_bps()),
                SortColumn::UpSpeed => a.upload_speed.speed_bps().cmp(&b.upload_speed.speed_bps()),
                SortColumn::Ratio => a
                    .ratio()
                    .partial_cmp(&b.ratio())
                    .unwrap_or(std::cmp::Ordering::Equal),
                SortColumn::Eta => {
                    let ea = a.eta_seconds().unwrap_or(u64::MAX);
                    let eb = b.eta_seconds().unwrap_or(u64::MAX);
                    ea.cmp(&eb)
                }
                SortColumn::Seeds => {
                    let sa: u32 = a.trackers.iter().map(|t| t.seeders).sum();
                    let sb: u32 = b.trackers.iter().map(|t| t.seeders).sum();
                    sa.cmp(&sb)
                }
                SortColumn::Peers => a.peers.len().cmp(&b.peers.len()),
                SortColumn::Added => a.added_time.cmp(&b.added_time),
            };
            if self.sort_ascending {
                cmp
            } else {
                cmp.reverse()
            }
        });

        list
    }

    /// Summary statistics
    #[must_use]
    pub fn stats(&self) -> (usize, usize, usize, u64, u64) {
        let downloading = self
            .torrents
            .iter()
            .filter(|t| t.state == TorrentState::Downloading)
            .count();
        let seeding = self
            .torrents
            .iter()
            .filter(|t| t.state == TorrentState::Seeding)
            .count();
        let total = self.torrents.len();
        let dl_speed = self.global_download_speed.speed_bps();
        let ul_speed = self.global_upload_speed.speed_bps();
        (downloading, seeding, total, dl_speed, ul_speed)
    }

    /// Render the UI
    #[must_use]
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let header_h = 48.0;
        let tab_h = 36.0;
        let status_h = 28.0;
        let sidebar_w = 160.0;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: colors::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header bar
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: header_h,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: 14.0,
            text: "Torrent".to_string(),
            font_size: 18.0,
            color: colors::BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Toolbar buttons
        let buttons = [
            "Add",
            "Remove",
            "Pause",
            "Resume",
            "Pause All",
            "Resume All",
        ];
        let mut bx = 120.0;
        for label in &buttons {
            let bw = label.len() as f32 * 8.0 + 24.0;
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: 8.0,
                width: bw,
                height: 32.0,
                color: colors::SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 12.0,
                y: 16.0,
                text: label.to_string(),
                font_size: 12.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            bx += bw + 8.0;
        }

        // Sidebar
        let sidebar_y = header_h;
        let sidebar_h = height - header_h - status_h;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: sidebar_y,
            width: sidebar_w,
            height: sidebar_h,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Filter items in sidebar
        let filters = [
            TorrentFilter::All,
            TorrentFilter::Downloading,
            TorrentFilter::Seeding,
            TorrentFilter::Completed,
            TorrentFilter::Paused,
            TorrentFilter::Active,
            TorrentFilter::Error,
        ];
        let mut fy = sidebar_y + 8.0;
        for filter in &filters {
            let is_sel = *filter == self.filter;
            if is_sel {
                cmds.push(RenderCommand::FillRect {
                    x: 4.0,
                    y: fy,
                    width: sidebar_w - 8.0,
                    height: 28.0,
                    color: colors::SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            let count = self
                .torrents
                .iter()
                .filter(|t| filter.matches(t.state))
                .count();
            cmds.push(RenderCommand::Text {
                x: 16.0,
                y: fy + 7.0,
                text: format!("{} ({})", filter.label(), count),
                font_size: 12.0,
                color: if is_sel {
                    colors::BLUE
                } else {
                    colors::SUBTEXT1
                },
                font_weight: if is_sel {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(sidebar_w - 24.0),
            });
            fy += 32.0;
        }

        // Labels section
        fy += 16.0;
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: fy,
            text: "Labels".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        fy += 20.0;
        for label in &self.labels {
            let is_sel = self.selected_label.as_ref() == Some(label);
            if is_sel {
                cmds.push(RenderCommand::FillRect {
                    x: 4.0,
                    y: fy,
                    width: sidebar_w - 8.0,
                    height: 24.0,
                    color: colors::SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            cmds.push(RenderCommand::Text {
                x: 16.0,
                y: fy + 5.0,
                text: label.clone(),
                font_size: 12.0,
                color: if is_sel {
                    colors::BLUE
                } else {
                    colors::SUBTEXT0
                },
                font_weight: FontWeightHint::Regular,
                max_width: Some(sidebar_w - 24.0),
            });
            fy += 28.0;
        }

        // Tab bar
        let content_x = sidebar_w;
        let content_w = width - sidebar_w;
        cmds.push(RenderCommand::FillRect {
            x: content_x,
            y: header_h,
            width: content_w,
            height: tab_h,
            color: colors::CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let mut tx = content_x + 8.0;
        for tab in &Tab::ALL {
            let is_active = *tab == self.active_tab;
            let tw = tab.label().len() as f32 * 8.0 + 20.0;
            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tx,
                    y: header_h + 4.0,
                    width: tw,
                    height: tab_h - 4.0,
                    color: colors::BASE,
                    corner_radii: CornerRadii {
                        top_left: 6.0,
                        top_right: 6.0,
                        bottom_left: 0.0,
                        bottom_right: 0.0,
                    },
                });
            }
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: header_h + 12.0,
                text: tab.label().to_string(),
                font_size: 12.0,
                color: if is_active {
                    colors::BLUE
                } else {
                    colors::SUBTEXT0
                },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
            tx += tw + 4.0;
        }

        // Content area
        let content_y = header_h + tab_h;
        let content_h = height - header_h - tab_h - status_h;

        match self.active_tab {
            Tab::Transfers => {
                self.render_transfers(&mut cmds, content_x, content_y, content_w, content_h)
            }
            Tab::Details => {
                self.render_details(&mut cmds, content_x, content_y, content_w, content_h)
            }
            Tab::Peers => self.render_peers(&mut cmds, content_x, content_y, content_w, content_h),
            Tab::Files => self.render_files(&mut cmds, content_x, content_y, content_w, content_h),
            Tab::Trackers => {
                self.render_trackers(&mut cmds, content_x, content_y, content_w, content_h)
            }
            Tab::Settings => {
                self.render_settings(&mut cmds, content_x, content_y, content_w, content_h)
            }
        }

        // Status bar
        let sy = height - status_h;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: sy,
            width,
            height: status_h,
            color: colors::CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let (downloading, seeding, total, dl_speed, ul_speed) = self.stats();
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: sy + 8.0,
            text: format!(
                "↓ {}  ↑ {}  |  {} downloading, {} seeding, {} total  |  {}",
                format_speed(dl_speed),
                format_speed(ul_speed),
                downloading,
                seeding,
                total,
                self.status_message
            ),
            font_size: 11.0,
            color: colors::SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });

        cmds
    }

    fn render_transfers(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        // Column headers
        let cols = [
            ("Name", 250.0),
            ("Size", 80.0),
            ("Progress", 120.0),
            ("Status", 80.0),
            ("↓ Speed", 80.0),
            ("↑ Speed", 80.0),
            ("Ratio", 60.0),
            ("ETA", 80.0),
        ];
        let mut cx = x + 8.0;
        let hy = y + 4.0;
        for (label, cw) in &cols {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: hy,
                text: label.to_string(),
                font_size: 11.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(*cw),
            });
            cx += cw + 8.0;
        }

        // Torrent rows
        let filtered = self.filtered_torrents();
        let row_h = 48.0;
        let mut ry = y + 24.0;

        for torrent in filtered.iter().take(((h - 24.0) / row_h) as usize) {
            if ry + row_h > y + h {
                break;
            }

            let is_sel = self.selected_torrent == Some(torrent.id);
            if is_sel {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: ry,
                    width: w - 8.0,
                    height: row_h - 2.0,
                    color: colors::SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let mut cx = x + 8.0;

            // Name
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: torrent.name.clone(),
                font_size: 12.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(250.0),
            });
            if !torrent.label.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: cx,
                    y: ry + 24.0,
                    text: torrent.label.clone(),
                    font_size: 10.0,
                    color: colors::MAUVE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(250.0),
                });
            }
            cx += 258.0;

            // Size
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: format_size(torrent.total_size),
                font_size: 12.0,
                color: colors::SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 88.0;

            // Progress bar
            let bar_w = 112.0;
            let bar_h = 12.0;
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: ry + 8.0,
                width: bar_w,
                height: bar_h,
                color: colors::SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            let progress = torrent.progress();
            let fill_w = (bar_w * progress as f32 / 100.0).min(bar_w);
            if fill_w > 0.5 {
                let bar_color = match torrent.state {
                    TorrentState::Downloading => colors::BLUE,
                    TorrentState::Seeding => colors::GREEN,
                    TorrentState::Paused => colors::YELLOW,
                    TorrentState::Error => colors::RED,
                    _ => colors::TEAL,
                };
                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: ry + 8.0,
                    width: fill_w,
                    height: bar_h,
                    color: bar_color,
                    corner_radii: CornerRadii::all(3.0),
                });
            }
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 24.0,
                text: format!("{progress:.1}%"),
                font_size: 10.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 128.0;

            // Status
            let status_color = match torrent.state {
                TorrentState::Downloading => colors::BLUE,
                TorrentState::Seeding => colors::GREEN,
                TorrentState::Paused => colors::YELLOW,
                TorrentState::Error => colors::RED,
                TorrentState::Complete => colors::TEAL,
                _ => colors::SUBTEXT0,
            };
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: torrent.state.to_string(),
                font_size: 12.0,
                color: status_color,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 88.0;

            // Down speed
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: format_speed(torrent.download_speed.speed_bps()),
                font_size: 12.0,
                color: colors::TEAL,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 88.0;

            // Up speed
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: format_speed(torrent.upload_speed.speed_bps()),
                font_size: 12.0,
                color: colors::PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 88.0;

            // Ratio
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: format!("{:.2}", torrent.ratio()),
                font_size: 12.0,
                color: colors::SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 68.0;

            // ETA
            let eta_str = torrent
                .eta_seconds()
                .map_or_else(|| "∞".to_string(), format_duration);
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: eta_str,
                font_size: 12.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            ry += row_h;
        }

        if filtered.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 80.0,
                y: y + h / 2.0 - 10.0,
                text: "No torrents".to_string(),
                font_size: 14.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_details(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, _h: f32) {
        let torrent = if let Some(t) = self
            .selected_torrent
            .and_then(|id| self.torrents.iter().find(|t| t.id == id))
        {
            t
        } else {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 20.0,
                text: "Select a torrent to view details".to_string(),
                font_size: 13.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        };

        let mut dy = y + 12.0;
        let label_x = x + 16.0;
        let value_x = x + 160.0;
        let max_val_w = w - 180.0;

        let fields: Vec<(&str, String)> = vec![
            ("Name:", torrent.name.clone()),
            ("Save Path:", torrent.save_path.clone()),
            ("Total Size:", format_size(torrent.total_size)),
            ("Downloaded:", format_size(torrent.downloaded)),
            ("Uploaded:", format_size(torrent.uploaded)),
            ("Ratio:", format!("{:.3}", torrent.ratio())),
            ("Status:", torrent.state.to_string()),
            ("Progress:", format!("{:.1}%", torrent.progress())),
            (
                "Pieces:",
                format!(
                    "{} / {} ({} each)",
                    torrent.pieces.completed_count(),
                    torrent.pieces.total_count(),
                    torrent
                        .metainfo
                        .as_ref()
                        .map_or("?".to_string(), |m| format_size(m.piece_length)),
                ),
            ),
            ("Peers:", format!("{} connected", torrent.peers.len())),
            (
                "Info Hash:",
                torrent.metainfo.as_ref().map_or_else(
                    || {
                        torrent
                            .magnet
                            .as_ref()
                            .map_or("N/A".to_string(), |m| hex_encode(&m.info_hash))
                    },
                    |m| hex_encode(&m.info_hash),
                ),
            ),
            (
                "Comment:",
                torrent
                    .metainfo
                    .as_ref()
                    .and_then(|m| m.comment.clone())
                    .unwrap_or_else(|| "N/A".to_string()),
            ),
            (
                "Created By:",
                torrent
                    .metainfo
                    .as_ref()
                    .and_then(|m| m.created_by.clone())
                    .unwrap_or_else(|| "N/A".to_string()),
            ),
            (
                "Sequential:",
                if torrent.sequential_download {
                    "Yes"
                } else {
                    "No"
                }
                .to_string(),
            ),
            (
                "Label:",
                if torrent.label.is_empty() {
                    "None".to_string()
                } else {
                    torrent.label.clone()
                },
            ),
        ];

        for (label, value) in &fields {
            cmds.push(RenderCommand::Text {
                x: label_x,
                y: dy,
                text: label.to_string(),
                font_size: 12.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: value_x,
                y: dy,
                text: value.clone(),
                font_size: 12.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_val_w),
            });
            dy += 22.0;
        }
    }

    fn render_peers(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        let torrent = if let Some(t) = self
            .selected_torrent
            .and_then(|id| self.torrents.iter().find(|t| t.id == id))
        {
            t
        } else {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 20.0,
                text: "Select a torrent to view peers".to_string(),
                font_size: 13.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        };

        // Headers
        let headers = [
            ("Address", 160.0),
            ("Client", 140.0),
            ("↓ Speed", 80.0),
            ("↑ Speed", 80.0),
            ("Downloaded", 90.0),
            ("Flags", 80.0),
        ];
        let mut cx = x + 8.0;
        for (label, cw) in &headers {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: y + 4.0,
                text: label.to_string(),
                font_size: 11.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(*cw),
            });
            cx += cw + 8.0;
        }

        let mut py = y + 24.0;
        for peer in &torrent.peers {
            if py + 24.0 > y + h {
                break;
            }

            let mut cx = x + 8.0;

            // Address
            cmds.push(RenderCommand::Text {
                x: cx,
                y: py,
                text: format!("{}:{}", peer.address, peer.port),
                font_size: 11.0,
                color: colors::SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(160.0),
            });
            cx += 168.0;

            // Client
            cmds.push(RenderCommand::Text {
                x: cx,
                y: py,
                text: peer.client_name.clone(),
                font_size: 11.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
            });
            cx += 148.0;

            // Down speed
            cmds.push(RenderCommand::Text {
                x: cx,
                y: py,
                text: format_speed(peer.download_rate),
                font_size: 11.0,
                color: colors::TEAL,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 88.0;

            // Up speed
            cmds.push(RenderCommand::Text {
                x: cx,
                y: py,
                text: format_speed(peer.upload_rate),
                font_size: 11.0,
                color: colors::PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 88.0;

            // Downloaded
            cmds.push(RenderCommand::Text {
                x: cx,
                y: py,
                text: format_size(peer.downloaded),
                font_size: 11.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 98.0;

            // Flags
            let mut flags = String::new();
            if !peer.am_choking {
                flags.push('u');
            } // uploading to them
            if peer.am_interested {
                flags.push('I');
            } // interested in them
            if !peer.peer_choking {
                flags.push('d');
            } // downloading from them
            if peer.peer_interested {
                flags.push('i');
            } // they're interested
            if peer.supports_extensions {
                flags.push('e');
            }
            cmds.push(RenderCommand::Text {
                x: cx,
                y: py,
                text: flags,
                font_size: 11.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            py += 24.0;
        }

        if torrent.peers.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 50.0,
                y: y + 40.0,
                text: "No peers".to_string(),
                font_size: 13.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_files(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, _w: f32, h: f32) {
        let torrent = if let Some(t) = self
            .selected_torrent
            .and_then(|id| self.torrents.iter().find(|t| t.id == id))
        {
            t
        } else {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 20.0,
                text: "Select a torrent to view files".to_string(),
                font_size: 13.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        };

        let meta_files = torrent
            .metainfo
            .as_ref()
            .map_or(&[] as &[TorrentFile], |m| &m.files);

        // Headers
        let headers = [("Name", 300.0), ("Size", 80.0), ("Priority", 80.0)];
        let mut cx = x + 8.0;
        for (label, cw) in &headers {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: y + 4.0,
                text: label.to_string(),
                font_size: 11.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(*cw),
            });
            cx += cw + 8.0;
        }

        let mut fy = y + 24.0;
        for (i, file) in meta_files.iter().enumerate() {
            if fy + 22.0 > y + h {
                break;
            }

            let priority = torrent
                .file_priorities
                .get(i)
                .copied()
                .unwrap_or(FilePriority::Normal);
            let mut cx = x + 8.0;

            cmds.push(RenderCommand::Text {
                x: cx,
                y: fy,
                text: file.path.clone(),
                font_size: 11.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });
            cx += 308.0;

            cmds.push(RenderCommand::Text {
                x: cx,
                y: fy,
                text: format_size(file.length),
                font_size: 11.0,
                color: colors::SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 88.0;

            let prio_color = match priority {
                FilePriority::Skip => colors::OVERLAY0,
                FilePriority::Low => colors::YELLOW,
                FilePriority::Normal => colors::TEXT,
                FilePriority::High => colors::GREEN,
            };
            cmds.push(RenderCommand::Text {
                x: cx,
                y: fy,
                text: priority.to_string(),
                font_size: 11.0,
                color: prio_color,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            fy += 22.0;
        }
    }

    fn render_trackers(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, _w: f32, h: f32) {
        let torrent = if let Some(t) = self
            .selected_torrent
            .and_then(|id| self.torrents.iter().find(|t| t.id == id))
        {
            t
        } else {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 20.0,
                text: "Select a torrent to view trackers".to_string(),
                font_size: 13.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        };

        let headers = [
            ("URL", 300.0),
            ("Status", 100.0),
            ("Seeds", 60.0),
            ("Leechers", 60.0),
            ("Tier", 40.0),
        ];
        let mut cx = x + 8.0;
        for (label, cw) in &headers {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: y + 4.0,
                text: label.to_string(),
                font_size: 11.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(*cw),
            });
            cx += cw + 8.0;
        }

        let mut ty = y + 24.0;
        for tracker in &torrent.trackers {
            if ty + 22.0 > y + h {
                break;
            }
            let mut cx = x + 8.0;

            cmds.push(RenderCommand::Text {
                x: cx,
                y: ty,
                text: tracker.url.clone(),
                font_size: 11.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });
            cx += 308.0;

            let status_color = match tracker.status {
                TrackerStatus::Working => colors::GREEN,
                TrackerStatus::Updating => colors::BLUE,
                TrackerStatus::Error => colors::RED,
                _ => colors::SUBTEXT0,
            };
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ty,
                text: tracker.status.to_string(),
                font_size: 11.0,
                color: status_color,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 108.0;

            cmds.push(RenderCommand::Text {
                x: cx,
                y: ty,
                text: tracker.seeders.to_string(),
                font_size: 11.0,
                color: colors::GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 68.0;

            cmds.push(RenderCommand::Text {
                x: cx,
                y: ty,
                text: tracker.leechers.to_string(),
                font_size: 11.0,
                color: colors::PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 68.0;

            cmds.push(RenderCommand::Text {
                x: cx,
                y: ty,
                text: tracker.tier.to_string(),
                font_size: 11.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            ty += 22.0;
        }
    }

    fn render_settings(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, _h: f32) {
        let mut sy = y + 12.0;
        let label_x = x + 16.0;
        let value_x = x + 220.0;
        let max_val_w = w - 240.0;

        let settings: Vec<(&str, String)> = vec![
            ("Listen Port:", self.settings.listen_port.to_string()),
            (
                "Max Active Downloads:",
                self.settings.max_active_downloads.to_string(),
            ),
            (
                "Max Active Seeds:",
                self.settings.max_active_seeds.to_string(),
            ),
            (
                "Max Connections:",
                self.settings.max_connections_global.to_string(),
            ),
            (
                "Connections/Torrent:",
                self.settings.max_connections_per_torrent.to_string(),
            ),
            (
                "Global Download Limit:",
                if self.settings.global_download_limit == 0 {
                    "Unlimited".to_string()
                } else {
                    format_speed(self.settings.global_download_limit)
                },
            ),
            (
                "Global Upload Limit:",
                if self.settings.global_upload_limit == 0 {
                    "Unlimited".to_string()
                } else {
                    format_speed(self.settings.global_upload_limit)
                },
            ),
            (
                "Default Save Path:",
                self.settings.default_save_path.clone(),
            ),
            ("Encryption:", self.settings.encryption_mode.to_string()),
            (
                "DHT:",
                if self.settings.dht_enabled {
                    "Enabled"
                } else {
                    "Disabled"
                }
                .to_string(),
            ),
            (
                "PEX:",
                if self.settings.pex_enabled {
                    "Enabled"
                } else {
                    "Disabled"
                }
                .to_string(),
            ),
            (
                "µTP:",
                if self.settings.enable_utp {
                    "Enabled"
                } else {
                    "Disabled"
                }
                .to_string(),
            ),
            (
                "Seed Ratio Limit:",
                self.settings
                    .seed_ratio_limit
                    .map_or("Unlimited".to_string(), |r| format!("{r:.1}")),
            ),
            (
                "Pre-allocate Storage:",
                if self.settings.pre_allocate_storage {
                    "Yes"
                } else {
                    "No"
                }
                .to_string(),
            ),
            ("Proxy:", self.settings.proxy_type.to_string()),
        ];

        for (label, value) in &settings {
            cmds.push(RenderCommand::Text {
                x: label_x,
                y: sy,
                text: label.to_string(),
                font_size: 12.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: value_x,
                y: sy,
                text: value.clone(),
                font_size: 12.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_val_w),
            });
            sy += 22.0;
        }
    }
}

// ─── Formatting helpers ──────────────────────────────────────────────

/// Format bytes as human-readable size
#[must_use]
pub fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        format!("{:.2} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Format speed in bytes/s as human-readable
#[must_use]
pub fn format_speed(bps: u64) -> String {
    if bps == 0 {
        return "0 B/s".to_string();
    }
    format!("{}/s", format_size(bps))
}

/// Format duration in seconds as human-readable
#[must_use]
pub fn format_duration(seconds: u64) -> String {
    if seconds >= 86400 {
        let d = seconds / 86400;
        let h = (seconds % 86400) / 3600;
        format!("{d}d {h}h")
    } else if seconds >= 3600 {
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        format!("{h}h {m}m")
    } else if seconds >= 60 {
        let m = seconds / 60;
        let s = seconds % 60;
        format!("{m}m {s}s")
    } else {
        format!("{seconds}s")
    }
}

// ─── Main ────────────────────────────────────────────────────────────

fn main() {
    let mut app = TorrentApp::new();

    // Add sample torrents for testing
    let sample_torrent = create_sample_torrent(
        "Ubuntu 24.04 LTS Desktop",
        4_200_000_000,
        262_144,
        "https://torrent.ubuntu.com/announce",
    );
    app.add_torrent(sample_torrent, None);

    let sample2 = create_sample_torrent(
        "LibreOffice 7.6.4",
        350_000_000,
        524_288,
        "udp://tracker.opentrackr.org:1337/announce",
    );
    let id2 = app.add_torrent(sample2, None);
    if let Some(t) = app.torrents.iter_mut().find(|t| t.id == id2) {
        t.state = TorrentState::Downloading;
        t.downloaded = 175_000_000;
        t.label = "Software".to_string();
    }

    let magnet = MagnetLink {
        info_hash: [0xAB; 20],
        display_name: Some("Big Buck Bunny 1080p".to_string()),
        trackers: vec!["udp://tracker.openbittorrent.com:80".to_string()],
        web_seeds: Vec::new(),
        exact_length: Some(276_134_947),
    };
    app.add_magnet(magnet, None);

    let cmds = app.render(1280.0, 800.0);
    // In a real system, these commands would be sent to the compositor
    let _ = cmds;
}

/// Create a sample torrent for testing
fn create_sample_torrent(name: &str, size: u64, piece_len: u64, announce: &str) -> TorrentMetainfo {
    let piece_count = (size.saturating_add(piece_len).saturating_sub(1)) / piece_len;
    let pieces: Vec<[u8; 20]> = (0..piece_count)
        .map(|i| {
            let mut hash = [0u8; 20];
            hash[0] = (i & 0xFF) as u8;
            hash[1] = ((i >> 8) & 0xFF) as u8;
            hash
        })
        .collect();

    TorrentMetainfo {
        info_hash: Sha1::digest(name.as_bytes()),
        name: name.to_string(),
        piece_length: piece_len,
        pieces,
        files: vec![TorrentFile {
            path: format!("{name}.iso"),
            length: size,
            md5sum: None,
        }],
        total_size: size,
        announce: announce.to_string(),
        announce_list: Vec::new(),
        creation_date: Some(1_700_000_000),
        comment: Some(format!("A sample torrent: {name}")),
        created_by: Some("OurTorrent 0.1.0".to_string()),
        is_private: false,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Bencode tests
    #[test]
    fn test_bencode_integer() {
        let (val, len) = BencodeParser::parse(b"i42e").unwrap();
        assert_eq!(val.as_int(), Some(42));
        assert_eq!(len, 4);
    }

    #[test]
    fn test_bencode_negative_integer() {
        let (val, _) = BencodeParser::parse(b"i-7e").unwrap();
        assert_eq!(val.as_int(), Some(-7));
    }

    #[test]
    fn test_bencode_bytes() {
        let (val, _) = BencodeParser::parse(b"5:hello").unwrap();
        assert_eq!(val.as_str(), Some("hello"));
    }

    #[test]
    fn test_bencode_empty_bytes() {
        let (val, _) = BencodeParser::parse(b"0:").unwrap();
        assert_eq!(val.as_bytes(), Some(&[] as &[u8]));
    }

    #[test]
    fn test_bencode_list() {
        let (val, _) = BencodeParser::parse(b"li1ei2ei3ee").unwrap();
        let list = val.as_list().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].as_int(), Some(1));
        assert_eq!(list[2].as_int(), Some(3));
    }

    #[test]
    fn test_bencode_dict() {
        let (val, _) = BencodeParser::parse(b"d3:agei25e4:name3:Bobe").unwrap();
        let dict = val.as_dict().unwrap();
        assert_eq!(dict.get("age").unwrap().as_int(), Some(25));
        assert_eq!(dict.get("name").unwrap().as_str(), Some("Bob"));
    }

    #[test]
    fn test_bencode_roundtrip() {
        let original = BencodeValue::Dict({
            let mut m = BTreeMap::new();
            m.insert("key".to_string(), BencodeValue::Bytes(b"value".to_vec()));
            m.insert("num".to_string(), BencodeValue::Integer(42));
            m.insert(
                "list".to_string(),
                BencodeValue::List(vec![BencodeValue::Integer(1), BencodeValue::Integer(2)]),
            );
            m
        });
        let encoded = bencode_encode(&original);
        let (decoded, _) = BencodeParser::parse(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_bencode_nested_dict() {
        let data = b"d5:innerd3:keyi42eee";
        let (val, _) = BencodeParser::parse(data).unwrap();
        let outer = val.as_dict().unwrap();
        let inner = outer.get("inner").unwrap().as_dict().unwrap();
        assert_eq!(inner.get("key").unwrap().as_int(), Some(42));
    }

    // SHA-1 tests
    #[test]
    fn test_sha1_empty() {
        let hash = Sha1::digest(b"");
        assert_eq!(
            hex_encode(&hash),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn test_sha1_abc() {
        let hash = Sha1::digest(b"abc");
        assert_eq!(
            hex_encode(&hash),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn test_sha1_long() {
        let hash = Sha1::digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        assert_eq!(
            hex_encode(&hash),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    // Hex encoding tests
    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(&[0xDE, 0xAD, 0xBE, 0xEF]), "deadbeef");
    }

    #[test]
    fn test_hex_decode() {
        assert_eq!(hex_decode("deadbeef"), Some(vec![0xDE, 0xAD, 0xBE, 0xEF]));
    }

    #[test]
    fn test_hex_decode_invalid() {
        assert_eq!(hex_decode("xyz"), None);
    }

    // URL encoding tests
    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode_bytes(b"hello world"), "hello%20world");
    }

    #[test]
    fn test_url_decode() {
        assert_eq!(url_decode("hello%20world"), "hello world");
        assert_eq!(url_decode("hello+world"), "hello world");
    }

    // Magnet link tests
    #[test]
    fn test_magnet_parse_hex() {
        let uri = "magnet:?xt=urn:btih:aabbccddee00112233445566778899aabbccddee&dn=Test+File&tr=udp://tracker.example.com:1234";
        let magnet = MagnetLink::parse(uri).unwrap();
        assert_eq!(
            hex_encode(&magnet.info_hash),
            "aabbccddee00112233445566778899aabbccddee"
        );
        assert_eq!(magnet.display_name.as_deref(), Some("Test File"));
        assert_eq!(magnet.trackers.len(), 1);
    }

    #[test]
    fn test_magnet_roundtrip() {
        let original = MagnetLink {
            info_hash: [0x11; 20],
            display_name: Some("TestFile".to_string()),
            trackers: vec!["udp://tracker.example.com:6881".to_string()],
            web_seeds: Vec::new(),
            exact_length: None,
        };
        let uri = original.to_uri();
        assert!(uri.starts_with("magnet:?xt=urn:btih:"));
        assert!(uri.contains("dn=TestFile"));
    }

    // Peer message tests
    #[test]
    fn test_peer_keepalive() {
        let msg = PeerMessage::KeepAlive;
        let encoded = msg.encode();
        assert_eq!(encoded, vec![0, 0, 0, 0]);
    }

    #[test]
    fn test_peer_choke_roundtrip() {
        let msg = PeerMessage::Choke;
        let encoded = msg.encode();
        assert_eq!(encoded, vec![0, 0, 0, 1, 0]);
        let decoded = PeerMessage::decode(&[0]).unwrap();
        assert_eq!(decoded, PeerMessage::Choke);
    }

    #[test]
    fn test_peer_have() {
        let msg = PeerMessage::Have { piece_index: 42 };
        let encoded = msg.encode();
        assert_eq!(encoded.len(), 9);
        let decoded = PeerMessage::decode(&encoded[4..]).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_peer_request_roundtrip() {
        let msg = PeerMessage::Request {
            index: 5,
            begin: 0,
            length: 16384,
        };
        let encoded = msg.encode();
        let decoded = PeerMessage::decode(&encoded[4..]).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_peer_piece_data() {
        let data = vec![1, 2, 3, 4, 5];
        let msg = PeerMessage::Piece {
            index: 0,
            begin: 0,
            data: data.clone(),
        };
        let encoded = msg.encode();
        let decoded = PeerMessage::decode(&encoded[4..]).unwrap();
        if let PeerMessage::Piece {
            index,
            begin,
            data: d,
        } = decoded
        {
            assert_eq!(index, 0);
            assert_eq!(begin, 0);
            assert_eq!(d, data);
        } else {
            panic!("expected Piece");
        }
    }

    // Handshake tests
    #[test]
    fn test_handshake_roundtrip() {
        let info_hash = [0xAA; 20];
        let peer_id = [0xBB; 20];
        let hs = Handshake::new(info_hash, peer_id);
        let encoded = hs.encode();
        assert_eq!(encoded.len(), 68); // 1 + 19 + 8 + 20 + 20
        let decoded = Handshake::decode(&encoded).unwrap();
        assert_eq!(decoded.protocol, Handshake::PROTOCOL);
        assert_eq!(decoded.info_hash, info_hash);
        assert_eq!(decoded.peer_id, peer_id);
    }

    #[test]
    fn test_handshake_extensions() {
        let hs = Handshake::new([0; 20], [0; 20]);
        assert!(hs.supports_extensions());
    }

    // Piece tracker tests
    #[test]
    fn test_piece_tracker_new() {
        let tracker = PieceTracker::new(100);
        assert_eq!(tracker.total_count(), 100);
        assert_eq!(tracker.completed_count(), 0);
        assert!(!tracker.is_complete());
    }

    #[test]
    fn test_piece_tracker_set_has() {
        let mut tracker = PieceTracker::new(16);
        assert!(!tracker.has_piece(5));
        tracker.set_piece(5);
        assert!(tracker.has_piece(5));
        assert_eq!(tracker.completed_count(), 1);
    }

    #[test]
    fn test_piece_tracker_complete() {
        let mut tracker = PieceTracker::new(4);
        for i in 0..4 {
            tracker.set_piece(i);
        }
        assert!(tracker.is_complete());
        assert!((tracker.progress() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_piece_tracker_pick_rarest() {
        let mut tracker = PieceTracker::new(4);
        // Peer has pieces 0, 1, 2
        let peer_bf = vec![0b1110_0000];
        // Piece 1 is rarest
        let availability = vec![5, 1, 3, 0];
        let picked = tracker.pick_piece(&peer_bf, &availability);
        assert_eq!(picked, Some(1));
    }

    #[test]
    fn test_piece_tracker_skip_completed() {
        let mut tracker = PieceTracker::new(4);
        tracker.set_piece(1);
        let peer_bf = vec![0b1110_0000];
        let availability = vec![2, 1, 3, 0];
        let picked = tracker.pick_piece(&peer_bf, &availability);
        // Piece 1 is completed so skip it, pick piece 0 (rarest available)
        assert_eq!(picked, Some(0));
    }

    // Peer info tests
    #[test]
    fn test_peer_identify_qbittorrent() {
        let mut peer_id = [0u8; 20];
        peer_id[..8].copy_from_slice(b"-qB4500-");
        let name = PeerInfo::identify_client(&peer_id);
        assert!(name.contains("qBittorrent"));
    }

    #[test]
    fn test_peer_identify_transmission() {
        let mut peer_id = [0u8; 20];
        peer_id[..8].copy_from_slice(b"-TR3000-");
        let name = PeerInfo::identify_client(&peer_id);
        assert!(name.contains("Transmission"));
    }

    // Speed tracker tests
    #[test]
    fn test_speed_tracker() {
        let mut tracker = SpeedTracker::new(10, 1000);
        tracker.add_sample(1_000_000);
        tracker.add_sample(2_000_000);
        tracker.add_sample(3_000_000);
        // 3 samples × 1000ms = 3s, total 6MB
        assert_eq!(tracker.speed_bps(), 2_000_000);
        assert_eq!(tracker.total(), 6_000_000);
    }

    // Bandwidth limiter tests
    #[test]
    fn test_bandwidth_unlimited() {
        let mut limiter = BandwidthLimiter::new(0);
        assert!(limiter.is_unlimited());
        assert_eq!(limiter.request(1_000_000, 0), 1_000_000);
    }

    #[test]
    fn test_bandwidth_limited() {
        let mut limiter = BandwidthLimiter::new(100_000);
        assert_eq!(limiter.request(50_000, 1), 50_000);
        assert_eq!(limiter.request(80_000, 1), 50_000); // Only 50k remaining
        assert_eq!(limiter.request(80_000, 2), 80_000); // New second
    }

    // Tracker tests
    #[test]
    fn test_announce_url_build() {
        let req = AnnounceRequest {
            info_hash: [0xAA; 20],
            peer_id: [0xBB; 20],
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: 1000,
            compact: true,
            event: TrackerEvent::Started,
            numwant: Some(50),
        };
        let url = req.build_url("http://tracker.example.com/announce");
        assert!(url.contains("port=6881"));
        assert!(url.contains("compact=1"));
        assert!(url.contains("event=started"));
        assert!(url.contains("numwant=50"));
    }

    #[test]
    fn test_announce_response_compact() {
        let mut dict = BTreeMap::new();
        dict.insert("interval".to_string(), BencodeValue::Integer(1800));
        dict.insert("complete".to_string(), BencodeValue::Integer(10));
        dict.insert("incomplete".to_string(), BencodeValue::Integer(5));
        // Compact peers: 192.168.1.1:6881
        dict.insert(
            "peers".to_string(),
            BencodeValue::Bytes(vec![
                192, 168, 1, 1, 0x1A, 0xE1, // 192.168.1.1:6881
            ]),
        );
        let data = bencode_encode(&BencodeValue::Dict(dict));
        let resp = AnnounceResponse::from_bencode(&data).unwrap();
        assert_eq!(resp.interval, 1800);
        assert_eq!(resp.complete, 10);
        assert_eq!(resp.peers.len(), 1);
        assert_eq!(resp.peers[0].0, "192.168.1.1");
        assert_eq!(resp.peers[0].1, 6881);
    }

    #[test]
    fn test_announce_response_failure() {
        let mut dict = BTreeMap::new();
        dict.insert(
            "failure reason".to_string(),
            BencodeValue::Bytes(b"torrent not found".to_vec()),
        );
        let data = bencode_encode(&BencodeValue::Dict(dict));
        let resp = AnnounceResponse::from_bencode(&data).unwrap();
        assert!(resp.failure_reason.is_some());
    }

    // Torrent app tests
    #[test]
    fn test_app_add_remove() {
        let mut app = TorrentApp::new();
        let meta = create_sample_torrent("Test", 1000, 256, "http://t.co/a");
        let id = app.add_torrent(meta, None);
        assert_eq!(app.torrents.len(), 1);
        app.remove_torrent(id, false);
        assert_eq!(app.torrents.len(), 0);
    }

    #[test]
    fn test_app_pause_resume() {
        let mut app = TorrentApp::new();
        let meta = create_sample_torrent("Test", 1000, 256, "http://t.co/a");
        let id = app.add_torrent(meta, None);
        if let Some(t) = app.torrents.iter_mut().find(|t| t.id == id) {
            t.state = TorrentState::Downloading;
        }
        app.pause_torrent(id);
        assert_eq!(app.torrents[0].state, TorrentState::Paused);
        app.resume_torrent(id);
        assert_eq!(app.torrents[0].state, TorrentState::Downloading);
    }

    #[test]
    fn test_app_magnet() {
        let mut app = TorrentApp::new();
        let magnet = MagnetLink {
            info_hash: [0x11; 20],
            display_name: Some("TestFile".to_string()),
            trackers: vec!["udp://tracker.example.com:6881".to_string()],
            web_seeds: Vec::new(),
            exact_length: None,
        };
        let id = app.add_magnet(magnet, None);
        assert_eq!(app.torrents[0].state, TorrentState::Metadata);
        assert_eq!(app.torrents[0].name, "TestFile");
        assert_eq!(app.torrents[0].id, id);
    }

    #[test]
    fn test_app_filter() {
        let mut app = TorrentApp::new();
        let meta1 = create_sample_torrent("T1", 1000, 256, "http://t.co/a");
        let meta2 = create_sample_torrent("T2", 2000, 256, "http://t.co/a");
        let id1 = app.add_torrent(meta1, None);
        let id2 = app.add_torrent(meta2, None);
        if let Some(t) = app.torrents.iter_mut().find(|t| t.id == id1) {
            t.state = TorrentState::Downloading;
        }
        if let Some(t) = app.torrents.iter_mut().find(|t| t.id == id2) {
            t.state = TorrentState::Seeding;
        }
        app.filter = TorrentFilter::Downloading;
        let filtered = app.filtered_torrents();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "T1");
    }

    #[test]
    fn test_app_search() {
        let mut app = TorrentApp::new();
        let meta1 = create_sample_torrent("Ubuntu Desktop", 1000, 256, "http://t.co/a");
        let meta2 = create_sample_torrent("Fedora Server", 2000, 256, "http://t.co/a");
        app.add_torrent(meta1, None);
        app.add_torrent(meta2, None);
        app.search_query = "ubuntu".to_string();
        let filtered = app.filtered_torrents();
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].name.contains("Ubuntu"));
    }

    #[test]
    fn test_torrent_progress() {
        let mut app = TorrentApp::new();
        let meta = create_sample_torrent("Test", 1000, 256, "http://t.co/a");
        let id = app.add_torrent(meta, None);
        if let Some(t) = app.torrents.iter_mut().find(|t| t.id == id) {
            t.downloaded = 500;
        }
        assert!((app.torrents[0].progress() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_torrent_ratio() {
        let mut app = TorrentApp::new();
        let meta = create_sample_torrent("Test", 1000, 256, "http://t.co/a");
        let id = app.add_torrent(meta, None);
        if let Some(t) = app.torrents.iter_mut().find(|t| t.id == id) {
            t.downloaded = 1000;
            t.uploaded = 2000;
        }
        assert!((app.torrents[0].ratio() - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1_048_576), "1.0 MiB");
        assert_eq!(format_size(1_073_741_824), "1.00 GiB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3661), "1h 1m");
        assert_eq!(format_duration(90000), "1d 1h");
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(0), "0 B/s");
        assert_eq!(format_speed(1024), "1.0 KiB/s");
    }

    #[test]
    fn test_base32_decode() {
        // "hello" in base32 is NBSWY3DP
        let decoded = base32_decode("NBSWY3DP").unwrap();
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn test_render_produces_commands() {
        let app = TorrentApp::new();
        let cmds = app.render(1280.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_settings_defaults() {
        let settings = ClientSettings::default();
        assert_eq!(settings.listen_port, 6881);
        assert!(settings.dht_enabled);
        assert_eq!(settings.encryption_mode, EncryptionMode::Prefer);
    }
}
