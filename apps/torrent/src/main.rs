//! `BitTorrent` client application
//!
//! Features:
//! - Bencode parser (encode/decode)
//! - .torrent file parsing (single and multi-file)
//! - SHA-1 info hash computation
//! - Peer wire protocol messages (BEP 3)
//! - Piece management with bitfield tracking
//! - Tracker announce URLs built, and announce responses parsed
//! - Magnet link parsing (BEP 9)
//! - Download/upload speed tracking
//! - Multi-tab UI with transfer list, details, peers, files, trackers
//!
//! What it cannot do is **transfer anything**. The crate depends on `guitk`,
//! `oswindow`, `appearance` and `safeio`, and on no network at all: there is
//! no `std::net` here and no socket of any kind. The announce URL
//! `TrackerRequest::build_url` composes is never fetched, so no peer list
//! comes back, and a torrent with no peers makes no progress. That is
//! deliberate — see the note in the tick loop, which records why the
//! invented peers it used to have were worse than none.
//!
//! Three entries were removed from the list above rather than left to
//! mislead someone planning work from it. *Tracker announce/scrape (HTTP)*
//! became the line above it, since what exists is the URL and the response
//! parser, not the fetch between them. *Bandwidth throttling* is gone:
//! `BandwidthLimiter` is implemented and tested, but the two fields that hold
//! one are read by nothing, so no byte is ever delayed. *Peer discovery* is
//! gone for want of the transport.

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
// torrent parses BEP 3 bencode and renders progress UI — bencode parse
// indices come from explicit length-prefix decoding; UI math is on
// bounded widget dimensions. Allow defensive lints file-wide; allow
// dead_code for the SURFACE2/LAVENDER palette constants kept for theme
// future-proofing.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

use randrange::RandomSource;

// The transport's parts land a stage at a time, each tested; the session that
// drives them comes after the tracker and peer ones. `expect` rather than
// `allow`, so this goes the moment something uses them.
mod peer;
mod session;
mod storage;
mod tracker;

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

/// How deep lists and dictionaries may nest. Nothing in BitTorrent nests more
/// than a handful deep; the bound is what keeps eight megabytes of `l` -- from
/// a file, a tracker or a peer -- from recursing until the stack runs out.
const MAX_BENCODE_DEPTH: usize = 64;

impl BencodeParser {
    /// Parse a bencode value from bytes, returning it and how many bytes it
    /// took.
    pub fn parse(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        Self::parse_at(data, 0)
    }

    fn parse_at(data: &[u8], depth: usize) -> Result<(BencodeValue, usize), String> {
        if data.is_empty() {
            return Err("empty input".to_string());
        }
        if depth > MAX_BENCODE_DEPTH {
            return Err(format!("nested more than {MAX_BENCODE_DEPTH} deep"));
        }

        match data.first() {
            Some(b'i') => Self::parse_integer(data),
            Some(b'l') => Self::parse_list(data, depth),
            Some(b'd') => Self::parse_dict(data, depth),
            Some(b'0'..=b'9') => Self::parse_bytes(data),
            Some(c) => Err(format!("unexpected byte: {c}")),
            None => Err("unexpected end of input".to_string()),
        }
    }

    /// Where the value under `key` in the dictionary that `data` starts with
    /// lies, as a range of `data`: the bytes as they are in the file.
    ///
    /// An info hash is the SHA-1 of the `info` dictionary's own bytes.
    /// Re-encoding the parsed value gives the same bytes only when the file
    /// was written canonically -- keys sorted, integers without leading zeros
    /// -- and a torrent that was not would be given an info hash no tracker
    /// and no peer has heard of.
    pub fn dict_value_span(
        data: &[u8],
        key: &str,
    ) -> Result<Option<std::ops::Range<usize>>, String> {
        if data.first() != Some(&b'd') {
            return Err("not a dictionary".to_string());
        }
        let mut pos = 1_usize;
        loop {
            match data.get(pos) {
                None => return Err("unterminated dict".to_string()),
                Some(b'e') => return Ok(None),
                Some(_) => {}
            }
            let (found, key_len) = Self::parse_at(data.get(pos..).unwrap_or_default(), 1)?;
            let at = pos.saturating_add(key_len);
            let (_, value_len) = Self::parse_at(data.get(at..).unwrap_or_default(), 1)?;
            let end = at.saturating_add(value_len);
            if found.as_bytes() == Some(key.as_bytes()) {
                return Ok(Some(at..end));
            }
            pos = end;
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

    fn parse_list(data: &[u8], depth: usize) -> Result<(BencodeValue, usize), String> {
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
            let (val, consumed) =
                Self::parse_at(data.get(pos..).unwrap_or_default(), depth.saturating_add(1))?;
            items.push(val);
            pos = pos.saturating_add(consumed);
        }
    }

    fn parse_dict(data: &[u8], depth: usize) -> Result<(BencodeValue, usize), String> {
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
            let (key_val, key_consumed) =
                Self::parse_at(data.get(pos..).unwrap_or_default(), depth.saturating_add(1))?;
            let key = match key_val {
                BencodeValue::Bytes(b) => {
                    String::from_utf8(b).map_err(|e| format!("dict key not UTF-8: {e}"))?
                }
                _ => return Err("dict key must be a byte string".to_string()),
            };
            pos = pos.saturating_add(key_consumed);
            let (val, val_consumed) =
                Self::parse_at(data.get(pos..).unwrap_or_default(), depth.saturating_add(1))?;
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
    /// The path as the window shows it: the parts joined by `/`, with any
    /// byte that is not UTF-8 written `\xNN`. Never used to name a file.
    pub path: String,
    /// The path as the torrent gives it, a part at a time, each its own
    /// bytes: what a file on disk is named from. Every part has been checked
    /// by [`checked_part`], so none can climb out of the download's folder.
    pub parts: Vec<Vec<u8>>,
    pub length: u64,
    pub md5sum: Option<String>,
}

impl TorrentFile {
    /// A file at `path` (parts separated by `/`) of `length` bytes.
    #[must_use]
    pub fn named(path: &str, length: u64) -> Self {
        Self {
            path: path.to_string(),
            parts: path.split('/').map(|p| p.as_bytes().to_vec()).collect(),
            length,
            md5sum: None,
        }
    }
}

/// `bytes` as the window shows them: text, with any byte that is not UTF-8
/// written `\xNN`, so two names that differ only there still differ.
#[must_use]
pub fn shown_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for byte in chunk.invalid() {
            // Writing to a String cannot fail.
            let _ = write!(out, "\\x{byte:02X}");
        }
    }
    out
}

/// `part`, if it can name a file or folder inside the download's folder:
/// not empty, not `.` or `..`, and holding no `/` and no NUL.
///
/// A torrent is a stranger's file. One whose path held `..`, or a part with
/// a `/` in it, would have its bytes written wherever it pointed; this is the
/// check that makes the parts safe to join below the save folder.
pub fn checked_part(part: &[u8]) -> Result<&[u8], String> {
    if part.is_empty() || part == b"." || part == b".." || part.contains(&b'/') || part.contains(&0)
    {
        return Err(format!(
            "the torrent names a file outside its folder: \"{}\"",
            shown_bytes(part)
        ));
    }
    Ok(part)
}

/// The most a piece may hold. Pieces are held whole while they are fetched
/// and checked, one a peer; real torrents use 16 KiB to 16 MiB.
pub const MAX_PIECE_LENGTH: u64 = 32 * 1024 * 1024;

/// Torrent metadata parsed from .torrent file
#[derive(Debug, Clone)]
pub struct TorrentMetainfo {
    pub info_hash: [u8; 20],
    /// As the window shows it; see [`TorrentFile::path`].
    pub name: String,
    /// As the torrent gives it: the single file's name, or the folder the
    /// files go in. Checked by [`checked_part`].
    pub name_bytes: Vec<u8>,
    /// Whether the torrent is a folder of files (its info has a `files`
    /// list) rather than one file -- which decides whether `name` is a
    /// folder the files go in or the file itself.
    pub multi_file: bool,
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

        // The info hash, over the info dictionary's bytes as the file has
        // them -- see `BencodeParser::dict_value_span`.
        let span = BencodeParser::dict_value_span(data, "info")?.ok_or("missing 'info' dict")?;
        let info_hash = sha1::sha1(data.get(span).unwrap_or_default());

        let name_bytes = checked_part(
            info_dict
                .get("name")
                .and_then(BencodeValue::as_bytes)
                .ok_or("missing torrent name")?,
        )?
        .to_vec();
        let name = shown_bytes(&name_bytes);

        let piece_length = info_dict
            .get("piece length")
            .and_then(BencodeValue::as_int)
            .ok_or("missing piece length")?;
        let piece_length = u64::try_from(piece_length)
            .ok()
            .filter(|&n| n > 0)
            .ok_or(format!("a piece length of {piece_length} bytes"))?;
        if piece_length > MAX_PIECE_LENGTH {
            return Err(format!(
                "pieces of {piece_length} bytes are more than this client holds ({MAX_PIECE_LENGTH})"
            ));
        }

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

        let length_of = |v: Option<&BencodeValue>| -> Result<u64, String> {
            let n = v
                .and_then(BencodeValue::as_int)
                .ok_or("a file without a length")?;
            u64::try_from(n).map_err(|_| format!("a file of {n} bytes"))
        };
        // Single file or multi-file?
        let listed = info_dict.get("files").and_then(|v| v.as_list());
        let multi_file = listed.is_some();
        let files = if let Some(files_list) = listed {
            // Multi-file torrent: every entry must be a file with a length
            // and a path. One skipped would shift every byte after it into
            // the wrong file.
            files_list
                .iter()
                .map(|f| {
                    let fd = f.as_dict().ok_or("a file entry that is not a dictionary")?;
                    let length = length_of(fd.get("length"))?;
                    let parts = fd
                        .get("path")
                        .and_then(BencodeValue::as_list)
                        .ok_or("a file without a path")?
                        .iter()
                        .map(|p| {
                            p.as_bytes()
                                .ok_or_else(|| "a path part that is not a string".to_string())
                                .and_then(checked_part)
                                .map(<[u8]>::to_vec)
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    if parts.is_empty() {
                        return Err("a file with an empty path".to_string());
                    }
                    let path = parts
                        .iter()
                        .map(|p| shown_bytes(p))
                        .collect::<Vec<_>>()
                        .join("/");
                    let md5sum = fd.get("md5sum").and_then(|v| v.as_str()).map(String::from);
                    Ok(TorrentFile {
                        path,
                        parts,
                        length,
                        md5sum,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?
        } else {
            // Single file torrent
            let length = length_of(info_dict.get("length"))?;
            let md5sum = info_dict
                .get("md5sum")
                .and_then(|v| v.as_str())
                .map(String::from);
            vec![TorrentFile {
                path: name.clone(),
                parts: vec![name_bytes.clone()],
                length,
                md5sum,
            }]
        };

        let total_size = files
            .iter()
            .try_fold(0_u64, |sum, f| sum.checked_add(f.length))
            .ok_or("the files add up to more bytes than can be counted")?;
        // One hash a piece, and exactly as many pieces as the files fill.
        let wanted = total_size.div_ceil(piece_length);
        if u64::try_from(pieces.len()).ok() != Some(wanted) {
            return Err(format!(
                "{} piece hashes for {total_size} bytes in pieces of {piece_length}, which is {wanted}",
                pieces.len()
            ));
        }

        Ok(Self {
            info_hash,
            name,
            name_bytes,
            multi_file,
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

    /// A piece's priority: 0 for never, then 1, 5 and 10.
    #[must_use]
    pub fn priority(&self, index: usize) -> Option<u8> {
        self.priorities.get(index).copied()
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

impl FilePriority {
    /// The priority a piece of a file this important gets: the scale
    /// `PieceTracker` picks by, where 0 is never.
    #[must_use]
    pub fn piece_priority(self) -> u8 {
        match self {
            Self::Skip => 0,
            Self::Low => 1,
            Self::Normal => 5,
            Self::High => 10,
        }
    }
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
    /// Bytes of the pieces on disk that have matched their hashes -- what is
    /// had, not what has arrived (a piece that failed its hash arrived and is
    /// not had).
    pub downloaded: u64,
    pub uploaded: u64,
    pub total_size: u64,
    /// The folder the torrent's file or folder goes in.
    pub save_path: PathBuf,
    pub added_time: u64,
    pub completed_time: Option<u64>,
    pub file_priorities: Vec<FilePriority>,
    pub download_limit: BandwidthLimiter,
    pub upload_limit: BandwidthLimiter,
    pub seed_ratio_limit: Option<f64>,
    pub sequential_download: bool,
    pub error_message: Option<String>,
    pub label: String,
    /// Bytes that have arrived since the speed was last sampled.
    pub unsampled: u64,
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
    pub fn from_metainfo(id: u32, meta: TorrentMetainfo, save_path: &Path) -> Self {
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
            save_path: save_path.to_path_buf(),
            added_time: 0,
            completed_time: None,
            file_priorities: vec![FilePriority::Normal; file_count],
            download_limit: BandwidthLimiter::new(0),
            upload_limit: BandwidthLimiter::new(0),
            seed_ratio_limit: None,
            sequential_download: false,
            error_message: None,
            label: String::new(),
            unsampled: 0,
        }
    }

    /// Create from a magnet link
    #[must_use]
    pub fn from_magnet(id: u32, magnet: MagnetLink, save_path: &Path) -> Self {
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
            save_path: save_path.to_path_buf(),
            added_time: 0,
            completed_time: None,
            file_priorities: Vec::new(),
            download_limit: BandwidthLimiter::new(0),
            upload_limit: BandwidthLimiter::new(0),
            seed_ratio_limit: None,
            sequential_download: false,
            error_message: None,
            label: String::new(),
            unsampled: 0,
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
        if matches!(
            self.state,
            TorrentState::Downloading | TorrentState::Seeding | TorrentState::CheckingFiles
        ) {
            self.state = TorrentState::Paused;
            // Their connections end with the session.
            self.peers.clear();
        }
    }

    /// Whether a download can be started from here: added and not begun,
    /// paused, or stopped by an error.
    #[must_use]
    pub fn can_start(&self) -> bool {
        self.metainfo.is_some()
            && matches!(
                self.state,
                TorrentState::Queued | TorrentState::Paused | TorrentState::Error
            )
    }

    /// Which pieces are wanted: those of the files not set to Skip. A piece
    /// shared by a skipped file and a wanted one is wanted.
    #[must_use]
    pub fn wanted(&self) -> Vec<bool> {
        (0..self.pieces.total_count())
            .map(|i| self.pieces.priority(i).is_some_and(|p| p > 0))
            .collect()
    }

    /// Bytes of the pieces had.
    fn have_bytes(&self) -> u64 {
        let Some(meta) = &self.metainfo else {
            return 0;
        };
        (0..meta.piece_count())
            .filter(|&i| self.pieces.has_piece(i))
            .map(|i| meta.piece_size(i))
            .sum()
    }

    /// Take in what the torrent's download reports. Whether the download has
    /// ended -- finished or failed -- so its session can go.
    fn apply(&mut self, event: session::Event) -> bool {
        use session::Event as E;
        let same = |p: &PeerInfo, addr: &std::net::SocketAddr| {
            p.port == addr.port() && p.address == addr.ip().to_string()
        };
        match event {
            E::Checked(have) => {
                for (i, _) in have.iter().enumerate().filter(|(_, h)| **h) {
                    self.pieces.set_piece(i);
                }
                self.downloaded = self.have_bytes();
                self.state = TorrentState::Downloading;
                false
            }
            E::Tracker { url, result } => {
                if let Some(t) = self.trackers.iter_mut().find(|t| t.url == url) {
                    t.announce_count = t.announce_count.saturating_add(1);
                    match result {
                        Ok(answer) => {
                            t.status = TrackerStatus::Working;
                            t.seeders = answer.seeders.unwrap_or(0);
                            t.leechers = answer.leechers.unwrap_or(0);
                            t.error_message = answer.warning;
                        }
                        Err(why) => {
                            t.status = TrackerStatus::Error;
                            t.error_message = Some(why);
                        }
                    }
                }
                false
            }
            E::PeerUp {
                addr,
                id,
                extensions,
            } => {
                self.peers.retain(|p| !same(p, &addr));
                let mut peer = PeerInfo::new(&addr.ip().to_string(), addr.port());
                peer.supports_extensions = extensions;
                peer.peer_id = Some(id);
                peer.client_name = PeerInfo::identify_client(&id);
                peer.state = PeerState::Connected;
                self.peers.push(peer);
                false
            }
            E::PeerDown { addr, .. } => {
                self.peers.retain(|p| !same(p, &addr));
                false
            }
            E::Received { addr, bytes } => {
                self.unsampled = self.unsampled.saturating_add(bytes);
                if let Some(p) = self.peers.iter_mut().find(|p| same(p, &addr)) {
                    p.downloaded = p.downloaded.saturating_add(bytes);
                }
                false
            }
            E::Piece { index } => {
                self.pieces.set_piece(index);
                self.downloaded = self.have_bytes();
                false
            }
            // Fetched again by the session; nothing to show but the time.
            E::BadPiece { .. } => false,
            E::Finished => {
                // Complete, not Seeding: this client uploads nothing, and
                // "Seeding" would say it was sharing the file.
                self.state = TorrentState::Complete;
                self.completed_time = Some(now_secs());
                self.peers.clear();
                true
            }
            E::Failed(why) => {
                self.state = TorrentState::Error;
                self.error_message = Some(why);
                self.peers.clear();
                true
            }
        }
    }

    /// Toggle sequential download mode
    pub fn toggle_sequential(&mut self) {
        self.sequential_download = !self.sequential_download;
    }

    /// Set file `index`'s priority, and the priority of every piece it
    /// shares -- which the piece picker reads, and which nothing set: a file
    /// marked Skip was still downloaded. A piece takes the highest priority
    /// of the files it holds part of, so a piece shared by a skipped file and
    /// a wanted one is still fetched.
    pub fn set_file_priority(&mut self, index: usize, priority: FilePriority) {
        let Some(slot) = self.file_priorities.get_mut(index) else {
            return;
        };
        *slot = priority;
        let Some(meta) = &self.metainfo else {
            return;
        };
        let piece_length = meta.piece_length.max(1);
        let mut start = 0_u64;
        let mut spans = Vec::with_capacity(meta.files.len());
        for file in &meta.files {
            spans.push((start, start.saturating_add(file.length)));
            start = start.saturating_add(file.length);
        }
        for piece in 0..self.pieces.total_count() {
            let from = (piece as u64).saturating_mul(piece_length);
            let to = from.saturating_add(piece_length);
            let wanted = spans
                .iter()
                .zip(&self.file_priorities)
                .filter(|((a, b), _)| *a < to && from < *b)
                .map(|(_, p)| p.piece_priority())
                .max()
                .unwrap_or(5);
            self.pieces.set_priority(piece, wanted);
        }
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
    pub default_save_path: PathBuf,
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
            default_save_path: downloads_folder(),
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

use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::RenderTree;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::textinput::TextInput;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

/// The window size to ask for.
const WINDOW_WIDTH: f32 = 1300.0;
/// As [`WINDOW_WIDTH`].
const WINDOW_HEIGHT: f32 = 800.0;

/// How often a downloading torrent takes a piece.
///
/// The transfer is simulated -- there is no peer wire behind this -- so it is
/// the pace of the progress bar and not of any network.
const PIECE_STEP: Duration = Duration::from_millis(150);
use guitk::style::CornerRadii;
use guitk::table::{Column, Fit, Table};
use guitk::text;

/// Font size used for every detail-table header and cell.
const TABLE_FONT: f32 = 11.0;

/// The window's bands.
const HEADER_H: f32 = 48.0;
const TAB_H: f32 = 36.0;
const STATUS_H: f32 = 28.0;
const SIDEBAR_W: f32 = 160.0;

/// A transfer row's height.
const ROW_H: f32 = 48.0;

/// The band at the top of the transfer list that says what this client
/// cannot do.
const NOTICE_H: f32 = 48.0;

/// The most characters the magnet dialog takes. A magnet link with a dozen
/// trackers is a few kilobytes; this is past any.
const MAX_MAGNET_CHARS: usize = 8192;

/// The sidebar's filters, in the order the number keys choose them.
const FILTERS: [TorrentFilter; 7] = [
    TorrentFilter::All,
    TorrentFilter::Downloading,
    TorrentFilter::Seeding,
    TorrentFilter::Completed,
    TorrentFilter::Paused,
    TorrentFilter::Active,
    TorrentFilter::Error,
];

/// The transfer list's columns: what each sorts by, its heading, its width.
const TRANSFER_COLUMNS: [(SortColumn, &str, f32); 8] = [
    (SortColumn::Name, "Name", 250.0),
    (SortColumn::Size, "Size", 80.0),
    (SortColumn::Progress, "Progress", 120.0),
    (SortColumn::Status, "Status", 80.0),
    (SortColumn::DownSpeed, "\u{2193} Speed", 80.0),
    (SortColumn::UpSpeed, "\u{2191} Speed", 80.0),
    (SortColumn::Ratio, "Ratio", 60.0),
    (SortColumn::Eta, "ETA", 80.0),
];

/// Everything in the window a pointer can press, as the renderer records it.
///
/// Nothing answered the pointer: six toolbar buttons, seven filters, five
/// labels, six tabs, eight sortable column heads and every row were drawn as
/// controls and were pictures of them (`known-issues.md` ->
/// `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Open,
    AddMagnet,
    Remove,
    Pause,
    Resume,
    PauseAll,
    ResumeAll,
    Help,
    Search,
    Filter(TorrentFilter),
    /// A label in the sidebar, by its place in `TorrentApp::labels`.
    Label(usize),
    Tab(Tab),
    SortBy(SortColumn),
    TransferList,
    Row(u32),
    /// The selected transfer's label, in its details.
    CycleLabel,
    /// Whether the selected transfer downloads in order.
    ToggleSequential,
    /// A file's priority, by its place in the torrent's file list.
    FilePriority(usize),
    MagnetField,
    MagnetAdd,
    MagnetCancel,
    /// Around and behind the magnet dialog: a press does nothing.
    DialogBackdrop,
    HelpCard,
}

/// What the window says instead of a transfer.
///
/// Three lines. The third exists because an empty torrent list, or a torrent
/// sitting at 0%, is read as *the swarm has nobody in it* -- an unpopular
/// torrent, a dead tracker, someone else's problem. It is not: nothing here
/// has contacted a tracker or a peer, and nothing here can write a file even
/// if it had.
/// The most of a `.torrent` one open will read.
///
/// A torrent file describes content; it does not contain it, so this is
/// generous. Reported when it bites, because a cut bencode document does not
/// parse and the parse error alone would blame the file.
pub const MAX_TORRENT_BYTES: usize = 8 * 1024 * 1024;

/// What this client does not do, where the transfers are listed.
const TRANSFER_NOTE_LINES: [&str; 3] = [
    "Downloads only: nothing is uploaded, and no peer can connect to this client.",
    "Trackers are asked over UDP or plain HTTP; one reached only over https:// cannot be, for want of TLS.",
    "A magnet link cannot be fetched yet: its files are learned from peers, which this client does not ask.",
];

/// Where downloads go unless the user says otherwise: `Downloads` in the
/// home folder. The tests' downloads go to a folder of their own in the
/// temporary folder, whatever a test starts -- never the developer's.
fn downloads_folder() -> PathBuf {
    #[cfg(test)]
    {
        std::env::temp_dir().join(format!("slateos-torrent-tests-{}", std::process::id()))
    }
    #[cfg(not(test))]
    {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map_or_else(
                || PathBuf::from("Downloads"),
                |home| PathBuf::from(home).join("Downloads"),
            )
    }
}

/// Delete the files a torrent downloaded, and the folders it made that are
/// left empty. Only the torrent's own files, below its save folder: the same
/// paths `storage` writes, and nothing else.
fn delete_downloaded(torrent: &ManagedTorrent) -> Result<(), String> {
    let Some(meta) = &torrent.metainfo else {
        return Ok(());
    };
    let store = storage::Storage::new(meta, &torrent.save_path)?;
    let mut folders = Vec::new();
    for i in 0..meta.files.len() {
        let Some(path) = store.file_path(i) else {
            continue;
        };
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("could not delete {}: {e}", path.display())),
        }
        let mut up = path.parent();
        while let Some(folder) = up {
            if folder == torrent.save_path || !folder.starts_with(&torrent.save_path) {
                break;
            }
            if !folders.contains(&folder.to_path_buf()) {
                folders.push(folder.to_path_buf());
            }
            up = folder.parent();
        }
    }
    // Deepest first; one that is not empty holds something else, and stays.
    folders.sort_by_key(|f| std::cmp::Reverse(f.components().count()));
    for folder in folders {
        // A folder left non-empty holds the user's own files: keeping it is
        // the point, not a failure.
        let _ = std::fs::remove_dir(&folder);
    }
    Ok(())
}

/// Seconds since the Unix epoch, for the times a torrent keeps.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What the settings panel has to say about itself.
///
/// `TRANSFER_NOTE_LINES` covers the transfers view. This covers the
/// settings panel, which was the half those three lines never reached: a
/// person reading "this client cannot download or upload anything" has been
/// told the *transfers* do not happen, and may still reasonably believe that
/// "Encryption: Prefer" and "DHT: Enabled" describe how this client behaves
/// on a network.
///
/// They describe nothing. There is no network stack here, so none of these
/// values has ever been consulted by anything but the line that draws it --
/// the panel reads the value in order to print the value, which is the most
/// persuasive form of a false claim a program can make, because the evidence
/// is the program's own output at the moment the reader is checking.
///
/// The repair is this line and not a key. A control that moves and changes
/// nothing is a claim; a fixed value beside an honest note is a gap.
const SETTINGS_NOT_APPLIED: &str = "Only the listening port (announced to trackers) and the connections per \
     torrent are used; nothing else here is read yet.";

/// Columns of the Peers detail table.
const PEER_COLUMNS: &[Column] = &[
    Column {
        label: "Address",
        width: 160.0,
    },
    Column {
        label: "Client",
        width: 140.0,
    },
    Column {
        label: "↓ Speed",
        width: 80.0,
    },
    Column {
        label: "↑ Speed",
        width: 80.0,
    },
    Column {
        label: "Downloaded",
        width: 90.0,
    },
    Column {
        label: "Flags",
        width: 80.0,
    },
];

/// Columns of the Files detail table.
const FILE_COLUMNS: &[Column] = &[
    Column {
        label: "Name",
        width: 300.0,
    },
    Column {
        label: "Size",
        width: 80.0,
    },
    Column {
        label: "Priority",
        width: 80.0,
    },
];

/// Columns of the Trackers detail table.
const TRACKER_COLUMNS: &[Column] = &[
    Column {
        label: "URL",
        width: 300.0,
    },
    Column {
        label: "Status",
        width: 100.0,
    },
    Column {
        label: "Seeds",
        width: 60.0,
    },
    Column {
        label: "Leechers",
        width: 60.0,
    },
    Column {
        label: "Tier",
        width: 40.0,
    },
];

/// Active UI tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// The open picker. Holds the dialog and the routing thirteen
    /// applications used to write out by hand.
    pub picker: FilePicker,
    /// What the last open did, for the status line.
    pub last_open: Option<String>,
    /// The size the last frame was drawn at, so a click on the picker is
    /// answered against the window the user is looking at.
    pub win_width: f32,
    /// See `win_width`.
    pub win_height: f32,
    pub torrents: Vec<ManagedTorrent>,
    pub settings: ClientSettings,
    pub active_tab: Tab,
    pub selected_torrent: Option<u32>,
    pub next_id: u32,
    pub peer_id: [u8; 20],
    pub search_query: String,
    pub sort_column: SortColumn,
    /// Whether the shortcut card is up.
    pub show_help: bool,
    pub sort_ascending: bool,
    pub filter: TorrentFilter,
    pub global_download_speed: SpeedTracker,
    pub global_upload_speed: SpeedTracker,
    /// Whether the magnet dialog is up, what is typed in it, why the last
    /// attempt to add it failed, and where the new transfer will save.
    pub show_add_dialog: bool,
    pub magnet_input: TextInput,
    pub magnet_error: Option<String>,
    pub add_save_path: PathBuf,
    /// Each running download, by torrent id. Dropping one stops it.
    sessions: HashMap<u32, session::Session>,
    /// Milliseconds of ticks since the speeds were last sampled.
    since_sample_ms: u64,
    /// Whether the search box has the keys.
    pub search_active: bool,
    /// How far the transfer list is scrolled, in rows. It did not scroll:
    /// rows past the bottom were not drawn.
    pub transfer_scroll: usize,
    /// The text fields' clipboard.
    clipboard: String,
    /// What the pointer is over, so it can be drawn lit.
    hover: Option<Target>,
    /// Every box the last paint recorded, for hover and the wheel.
    last_hits: Vec<(Target, Rect)>,
    /// The wheel's remainder.
    wheel: wheel::Accumulator,
    pub status_message: String,
    pub labels: Vec<String>,
    pub selected_label: Option<String>,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

/// Stepping and naming for [`SortColumn`].
impl SortColumn {
    /// Every column, in the order `C` steps through them.
    ///
    /// Written in the same order as the comparator so the two cannot drift:
    /// a variant added to the enum and forgotten here would be unreachable
    /// again, which is the defect this list exists to end.
    pub const ALL: [Self; 11] = [
        Self::Name,
        Self::Size,
        Self::Progress,
        Self::Status,
        Self::DownSpeed,
        Self::UpSpeed,
        Self::Ratio,
        Self::Eta,
        Self::Seeds,
        Self::Peers,
        Self::Added,
    ];

    /// What the status bar calls this column.
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Progress => "progress",
            Self::Status => "status",
            Self::DownSpeed => "download speed",
            Self::UpSpeed => "upload speed",
            Self::Ratio => "ratio",
            Self::Eta => "time left",
            Self::Seeds => "seeds",
            Self::Peers => "peers",
            Self::Added => "date added",
        }
    }

    /// The next column round the ring.
    #[must_use]
    pub fn next(self) -> Self {
        let at = Self::ALL.iter().position(|c| *c == self).unwrap_or(0);
        let wrapped = at.saturating_add(1) % Self::ALL.len();
        Self::ALL.get(wrapped).copied().unwrap_or(Self::Added)
    }
}

/// Column for sorting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// The keys this window answers, as a reader sees them.
///
/// Before this the window named two of them -- `(C, R)` in the status bar
/// beside the sort -- and the seven filter digits, the streaming toggle and
/// all three Ctrl chords were written down nowhere. The survey read `1`, `2`
/// and `3` as named because those characters appear in drawn text like
/// "1 downloading"; a lone digit is cheap to match by accident, which is why
/// the count it reported was seven and the true number was ten.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F1 / ?", "This list"),
    ("Up / Down", "Move the selection"),
    ("Space", "Pause or resume the selected transfer"),
    ("S", "Download this one in order, for streaming"),
    ("Delete", "Remove it from the list; the files stay"),
    (
        "1-7",
        "Filter: all, downloading, seeding, done, paused, active, error",
    ),
    ("C", "Change the sort column"),
    ("R", "Reverse the sort"),
    ("Tab", "Next tab"),
    ("Enter", "The selected transfer's details"),
    ("L", "Change the selected transfer's label"),
    ("PgUp / PgDn", "Scroll the list a page"),
    ("/", "Search names and labels; Esc clears it"),
    ("Ctrl+O", "Open a .torrent file"),
    ("Ctrl+U", "Add a magnet link"),
    ("Ctrl+P", "Pause every transfer"),
    ("Ctrl+R", "Resume every transfer"),
];

impl Default for TorrentApp {
    fn default() -> Self {
        Self::new()
    }
}

impl TorrentApp {
    #[must_use]
    pub fn new() -> Self {
        // An Azureus-style id (BEP 20): this client and version, then twelve
        // characters drawn afresh each run, so no two clients share one.
        let mut peer_id = *b"-SL0001-000000000000";
        let mut rng = randrange::seeded_from_system(0x0070_6565_7269_6464);
        const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
        for b in peer_id.iter_mut().skip(8) {
            let pick = usize::try_from(rng.next_u32()).unwrap_or(0) % DIGITS.len();
            *b = DIGITS.get(pick).copied().unwrap_or(b'0');
        }

        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            picker: FilePicker::new(),
            last_open: None,
            win_width: WINDOW_WIDTH,
            win_height: WINDOW_HEIGHT,
            torrents: Vec::new(),
            settings: ClientSettings::default(),
            active_tab: Tab::Transfers,
            selected_torrent: None,
            next_id: 1,
            peer_id,
            search_query: String::new(),
            sort_column: SortColumn::Added,
            show_help: false,
            sort_ascending: false,
            filter: TorrentFilter::All,
            global_download_speed: SpeedTracker::new(60, 1000),
            global_upload_speed: SpeedTracker::new(60, 1000),
            show_add_dialog: false,
            magnet_input: TextInput::new(),
            magnet_error: None,
            add_save_path: ClientSettings::default().default_save_path.clone(),
            sessions: HashMap::new(),
            since_sample_ms: 0,
            search_active: false,
            transfer_scroll: 0,
            clipboard: String::new(),
            hover: None,
            last_hits: Vec::new(),
            wheel: wheel::Accumulator::default(),
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
    pub fn add_torrent(&mut self, meta: TorrentMetainfo, save_path: Option<&Path>) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let path = save_path.map_or_else(
            || self.settings.default_save_path.clone(),
            Path::to_path_buf,
        );
        let torrent = ManagedTorrent::from_metainfo(id, meta, &path);
        self.status_message = format!("Added: {}", torrent.name);
        self.torrents.push(torrent);
        id
    }

    /// Add a torrent from a magnet link
    pub fn add_magnet(&mut self, magnet: MagnetLink, save_path: Option<&Path>) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let path = save_path.map_or_else(
            || self.settings.default_save_path.clone(),
            Path::to_path_buf,
        );
        let torrent = ManagedTorrent::from_magnet(id, magnet, &path);
        self.status_message = format!("Added magnet: {}", torrent.name);
        self.torrents.push(torrent);
        id
    }

    /// Remove a torrent by ID
    pub fn remove_torrent(&mut self, id: u32, delete_files: bool) {
        // Its download ends first -- and when its files are to go, nothing may
        // be writing them. Neither waits on the trackers being told.
        if let Some(session) = self.sessions.remove(&id) {
            if delete_files {
                session.stop_until_quiet();
            } else {
                session.stop();
            }
        }
        if let Some(pos) = self.torrents.iter().position(|t| t.id == id) {
            let name = self
                .torrents
                .get(pos)
                .map_or("Unknown", |t| &t.name)
                .to_string();
            let removed = self.torrents.remove(pos);
            if delete_files && let Err(why) = delete_downloaded(&removed) {
                self.status_message = format!("Removed {name}, but {why}");
                if self.selected_torrent == Some(id) {
                    self.selected_torrent = None;
                }
                return;
            }
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

    // ====================================================================
    // Input
    //
    // This program had none, and twenty functions had no caller outside the
    // tests: the transfer controls (`pause_torrent`, `resume_torrent`,
    // `pause_all`, `resume_all`, `remove_torrent`), the settings
    // (`set_priority`, `toggle_sequential`, `set_limit`), and the piece
    // picker -- `pick_piece`, `set_piece`, `set_in_progress` -- which is the
    // whole of a `BitTorrent` client's download loop.
    // ====================================================================

    /// Handle one event from the window.
    /// Read `path` as a `.torrent` and list what is in it.
    ///
    /// `TorrentMetainfo::from_bencode` was written, tested and unreachable:
    /// this program had no way to obtain a byte. **A torrent file is the one
    /// thing here that does not need the network** -- it is a description of
    /// content, so reading it tells the user the name, the size, the piece
    /// count, the file list and which trackers it names, none of which
    /// requires contacting anything.
    ///
    /// Goes through `add_torrent` rather than building a `ManagedTorrent`
    /// here: that function owns the id counter, the default save path and the
    /// status line, and a second copy of it would drift.
    /// `ManagedTorrent::from_metainfo` marks every tracker `NotContacted`,
    /// which is the truth and stays the truth -- opening a file adds a
    /// description, not a download.
    ///
    /// Read as BYTES. `.torrent` is bencode, and the text reader refuses at
    /// the first byte that is not UTF-8, reporting "stream did not contain
    /// valid UTF-8" about a file that is perfectly valid and simply not text.
    pub fn open_torrent_file(&mut self, path: &std::path::Path) -> String {
        let read = match safeio::read_capped(path, MAX_TORRENT_BYTES) {
            Ok(read) => read,
            Err(err) => return format!("Could not read {}: {err}", path.display()),
        };
        // Front-loaded: a cut bencode document fails to parse, so without this
        // the user is told their file is malformed when it is merely long.
        let note = read.note(MAX_TORRENT_BYTES);
        match TorrentMetainfo::from_bencode(&read.bytes) {
            Ok(meta) => {
                let name = meta.name.clone();
                let files = meta.files.len().max(1);
                let id = self.add_torrent(meta, None);
                self.start_torrent(id);
                let into = self.settings.default_save_path.display();
                format!("{note}Opened {name}: {files} file(s), fetching into {into}")
            }
            // The parser's own reason, not one invented here: "missing 'info'
            // dict" and "missing torrent name" say which part is absent, and
            // replacing them with "not a torrent" would throw that away.
            Err(why) => format!("{note}Could not read {}: {why}", path.display()),
        }
    }

    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The picker takes input first while it is up, or a keystroke meant
        // for a filename reaches the list behind it -- where Delete removes
        // the selected torrent.
        //
        // A tick comes back as `Ignored` and falls through, and here that is
        // load-bearing rather than incidental: `tick_interval` asks for ticks
        // while anything is downloading, and the arm below advances the
        // pieces. Swallowing it would stall a transfer because somebody
        // opened a file dialog -- which is what apps/podcast and
        // apps/photomanager each did before this type existed.
        match self.picker.handle(event, self.win_width, self.win_height) {
            Picked::Chose(path) => {
                self.last_open = Some(self.open_torrent_file(&path));
                return EventResult::Consumed;
            }
            // Cancelled grouped with Handled: this caller keeps no dialog
            // state of its own that could go stale.
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key) if key.pressed => {
                let result = self.handle_key(key);
                self.keep_selection_visible();
                result
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Tick { elapsed_ms } => self.handle_tick(*elapsed_ms),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.win_width = *width as f32;
                    self.win_height = *height as f32;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Take in what each download has reported, and sample the speeds.
    ///
    /// This used to simulate a download: it picked a piece from invented
    /// peers each tick and marked it had, so a torrent ran to 100% with
    /// nothing fetched and nothing written. The sessions do the real thing
    /// now (`session.rs`), and a tick is when the window hears of it.
    fn handle_tick(&mut self, elapsed_ms: u64) -> EventResult {
        let mut moved = false;
        let ids: Vec<u32> = self.sessions.keys().copied().collect();
        for id in ids {
            let events: Vec<session::Event> = self
                .sessions
                .get(&id)
                .map(|s| s.events.try_iter().collect())
                .unwrap_or_default();
            let mut ended = false;
            match self.torrents.iter_mut().find(|t| t.id == id) {
                Some(t) => {
                    for event in events {
                        moved = true;
                        ended |= t.apply(event);
                    }
                }
                None => ended = true,
            }
            if ended {
                // Dropping it stops whatever of it is still running.
                self.sessions.remove(&id);
            }
        }
        // A second's worth of arrivals becomes one sample of the speed.
        self.since_sample_ms = self.since_sample_ms.saturating_add(elapsed_ms);
        if self.since_sample_ms >= 1000 {
            self.since_sample_ms = 0;
            let mut total = 0_u64;
            for t in &mut self.torrents {
                if self.sessions.contains_key(&t.id) || t.unsampled > 0 {
                    t.download_speed.add_sample(t.unsampled);
                    total = total.saturating_add(t.unsampled);
                    t.unsampled = 0;
                    moved = true;
                }
            }
            self.global_download_speed.add_sample(total);
        }
        if moved {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    /// Handle a key press.
    /// Choose a filter, reporting whether the list actually changed.
    ///
    /// `Ignored` when the filter is already the one asked for: pressing `1`
    /// twice does nothing the second time, and saying so is how every other
    /// key in this app behaves.
    fn set_filter(&mut self, filter: TorrentFilter) -> EventResult {
        // `Ignored` when the filter does not change, and that is not a key
        // being given away. I removed this early return on 2026-09-21 arguing
        // that `Ignored` means "propagate to parent" and so handed 3 to
        // whatever sits above this window. There is nothing above it:
        // `handle_event` maps `Ignored` to `Response::Idle`, so in a
        // top-level app the word means "nothing changed, do not redraw".
        // Putting it back, because a redundant repaint of every row on a key
        // that did nothing is a real if small cost, and the reason I took it
        // out was simply wrong.
        if self.filter == filter {
            return EventResult::Ignored;
        }
        self.filter = filter;
        EventResult::Consumed
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // Above the Ctrl branch, which returns for every Ctrl chord: placed
        // after it, Ctrl+P would pause every transfer from behind the card.
        if key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift) {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            // Modal. Delete removes the selected transfer, and doing that
            // from behind a list the reader is consulting is the reason this
            // does not let keys through.
            if matches!(key.key, Key::Escape | Key::Enter | Key::F1) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }
        // The magnet dialog and the search box take the keys while they have
        // them: a `1` in a magnet link would change the filter, and Delete in
        // the search would remove the selected transfer.
        if self.show_add_dialog {
            return self.handle_dialog_key(key);
        }
        if self.search_active {
            return self.handle_search_key(key);
        }
        if key.modifiers.ctrl {
            return match key.key {
                Key::U => {
                    self.open_magnet_dialog();
                    EventResult::Consumed
                }
                // Everything at once, which is what the toolbar buttons are
                // for and what nothing called.
                Key::P => {
                    self.pause_all();
                    EventResult::Consumed
                }
                Key::R => {
                    self.resume_all();
                    EventResult::Consumed
                }
                // **This arm used to be in the match below**, which is only
                // reached when Ctrl is NOT held -- the block it is in now
                // returns for every Ctrl chord, so `Key::O if
                // key.modifiers.ctrl` down there could never fire and Ctrl+O
                // did nothing at all.
                //
                // The door was shut and the test suite was green, because the
                // open test called `picker.open_to_read()` directly rather
                // than pressing the key. A guard narrows only the arm it is
                // on; an early `return match` narrows everything after it.
                Key::O => {
                    self.picker.open_to_read();
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            };
        }

        match key.key {
            Key::Up | Key::Down => {
                self.move_selection(if key.key == Key::Down { 1 } else { -1 });
                EventResult::Consumed
            }
            Key::PageUp | Key::PageDown => {
                let (_, rows) = Self::transfer_pane(self.content_rect());
                let page = isize::try_from(rows).unwrap_or(1);
                self.move_selection(if key.key == Key::PageDown {
                    page
                } else {
                    page.saturating_neg()
                });
                EventResult::Consumed
            }
            Key::Enter => {
                if self.selected_torrent.is_none() || self.active_tab == Tab::Details {
                    return EventResult::Ignored;
                }
                self.active_tab = Tab::Details;
                EventResult::Consumed
            }
            Key::L => self.cycle_label(),
            Key::Slash => {
                self.search_active = true;
                EventResult::Consumed
            }
            // Pause and resume the selected transfer. Both existed, both were
            // tested, and neither had a key -- so a download could be started
            // and not stopped.
            Key::Space => {
                if let Some(id) = self.selected_torrent {
                    let downloading = self.torrents.iter().find(|t| t.id == id).is_some_and(|t| {
                        matches!(
                            t.state,
                            TorrentState::Downloading | TorrentState::CheckingFiles
                        )
                    });
                    if downloading {
                        self.pause_torrent(id);
                    } else {
                        self.resume_torrent(id);
                    }
                }
                EventResult::Consumed
            }
            Key::Delete => {
                if let Some(id) = self.selected_torrent {
                    // The files stay: deleting someone's download because
                    // they pressed Delete on the list entry is the one
                    // irreversible thing this program can do, and it is not
                    // what a bare Delete should mean.
                    self.remove_torrent(id, false);
                    self.reanchor_selection();
                }
                EventResult::Consumed
            }
            // Download the pieces in order rather than rarest-first, which is
            // what someone streaming a file wants. `toggle_sequential` had no
            // caller.
            Key::S => {
                if let Some(id) = self.selected_torrent
                    && let Some(t) = self.torrents.iter_mut().find(|t| t.id == id)
                {
                    t.toggle_sequential();
                }
                EventResult::Consumed
            }
            // The filter sidebar, the sort column and its direction: three
            // controls this window draws and nothing could operate. The
            // sidebar highlighted whichever filter was selected and none could
            // be chosen; the comparator had eleven arms and ten were
            // unreachable. Found by `scripts/frozen-flag-survey.py` after it
            // learned to look at enums as well as booleans.
            Key::Num1 => self.set_filter(TorrentFilter::All),
            Key::Num2 => self.set_filter(TorrentFilter::Downloading),
            Key::Num3 => self.set_filter(TorrentFilter::Seeding),
            Key::Num4 => self.set_filter(TorrentFilter::Completed),
            Key::Num5 => self.set_filter(TorrentFilter::Paused),
            Key::Num6 => self.set_filter(TorrentFilter::Active),
            Key::Num7 => self.set_filter(TorrentFilter::Error),
            Key::C => {
                self.sort_column = self.sort_column.next();
                EventResult::Consumed
            }
            Key::R => {
                self.sort_ascending = !self.sort_ascending;
                EventResult::Consumed
            }
            Key::Tab => {
                self.active_tab = match self.active_tab {
                    Tab::Transfers => Tab::Details,
                    Tab::Details => Tab::Peers,
                    Tab::Peers => Tab::Files,
                    Tab::Files => Tab::Trackers,
                    Tab::Trackers => Tab::Settings,
                    Tab::Settings => Tab::Transfers,
                };
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Move the selection by `delta` rows through the torrent list.
    ///
    /// Held as an id rather than an index, so removing a torrent above the
    /// selected one does not silently select its neighbour. Stops at the ends.
    fn move_selection(&mut self, delta: isize) {
        // The list on screen, in its order: it walked `torrents` in the order
        // they were added, so with a sort or a filter on, Down jumped about
        // the list and onto rows the filter hid.
        let ids: Vec<u32> = self.filtered_torrents().iter().map(|t| t.id).collect();
        if ids.is_empty() {
            self.selected_torrent = None;
            return;
        }
        let last = (ids.len() as isize).saturating_sub(1);
        let current = self
            .selected_torrent
            .and_then(|id| ids.iter().position(|other| *other == id));
        let next = match current {
            Some(index) => (index as isize).saturating_add(delta).clamp(0, last),
            None if delta < 0 => last,
            None => 0,
        };
        self.selected_torrent = ids.get(next.unsigned_abs()).copied();
    }

    /// Keep the selection on a torrent that still exists.
    fn reanchor_selection(&mut self) {
        let ids: Vec<u32> = self.torrents.iter().map(|t| t.id).collect();
        if self.selected_torrent.is_some_and(|id| ids.contains(&id)) {
            return;
        }
        self.selected_torrent = ids.first().copied();
    }

    /// Start downloading torrent `id`: a session of its own checks what is
    /// already on disk, asks the trackers for peers and fetches the rest.
    pub fn start_torrent(&mut self, id: u32) {
        if self.sessions.contains_key(&id) {
            return;
        }
        let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if !t.can_start() {
            return;
        }
        let Some(meta) = t.metainfo.clone() else {
            return;
        };
        let plan = session::Plan {
            wanted: t.wanted(),
            meta,
            save_dir: t.save_path.clone(),
            peer_id: self.peer_id,
            port: self.settings.listen_port,
            max_peers: usize::try_from(self.settings.max_connections_per_torrent)
                .unwrap_or(usize::MAX)
                .max(1),
        };
        t.state = TorrentState::CheckingFiles;
        t.error_message = None;
        self.sessions.insert(id, session::Session::start(plan));
    }

    /// Pause a torrent: its download stops, and what it has fetched stays.
    pub fn pause_torrent(&mut self, id: u32) {
        if let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) {
            t.pause();
        }
        if let Some(session) = self.sessions.remove(&id) {
            session.stop();
        }
    }

    /// Resume a torrent: its download starts again from what is on disk.
    pub fn resume_torrent(&mut self, id: u32) {
        self.start_torrent(id);
    }

    /// Pause all torrents
    pub fn pause_all(&mut self) {
        let ids: Vec<u32> = self.torrents.iter().map(|t| t.id).collect();
        for id in ids {
            self.pause_torrent(id);
        }
        self.status_message = "All torrents paused".to_string();
    }

    /// Resume all torrents
    pub fn resume_all(&mut self) {
        let ids: Vec<u32> = self.torrents.iter().map(|t| t.id).collect();
        for id in ids {
            self.resume_torrent(id);
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

    /// For the tests: the window draws `frame`, whose boxes it keeps.
    ///
    /// Not `render`: [`App::render`] is the one the window calls, and this one
    /// takes the same two arguments -- so at equal arity the inherent method
    /// wins method lookup outright and the trait's is never called, silently.
    #[cfg(test)]
    #[must_use]
    pub fn render_commands(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        self.frame(width, height).into_tree().commands
    }

    /// Draw the window, recording every control where it is drawn: both the
    /// picture and the hit test.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        self.render_header(&mut f, width);
        self.render_sidebar(&mut f, height);
        self.render_tabs(&mut f, width);

        let content = Rect::new(
            SIDEBAR_W,
            HEADER_H + TAB_H,
            (width - SIDEBAR_W).max(0.0),
            (height - HEADER_H - TAB_H - STATUS_H).max(0.0),
        );
        f.clip(content);
        match self.active_tab {
            Tab::Transfers => self.render_transfers(&mut f, content),
            Tab::Details => self.render_details(&mut f, content),
            Tab::Peers => self.render_peers(&mut f, content),
            Tab::Files => self.render_files(&mut f, content),
            Tab::Trackers => self.render_trackers(&mut f, content),
            Tab::Settings => self.render_settings(&mut f, content),
        }
        f.unclip();
        self.render_status(&mut f, width, height);

        if self.show_add_dialog {
            self.render_magnet_dialog(&mut f, width, height);
        }
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 or ? closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));
        }
        // Last, so it is above everything. Without this the picker
        // takes every keystroke with nothing on screen to say why --
        // the defect apps/flashcards shipped.
        f.extend(self.picker.render(&self.palette, width, height));
        f
    }

    /// A button, lit while the pointer is on it; one with nothing to do is
    /// drawn dim and records no box.
    fn button(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        label: &str,
        target: Target,
        enabled: bool,
    ) {
        let lit = enabled && self.hover == Some(target);
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if lit {
                self.palette.surface2
            } else {
                self.palette.surface1
            },
            corner_radii: CornerRadii::all(4.0),
        });
        f.push(RenderCommand::Text {
            x: rect.x + 12.0,
            y: rect.y + (rect.h - 12.0) / 2.0,
            text: label.to_string(),
            font_size: 12.0,
            color: if enabled {
                self.palette.text
            } else {
                self.palette.overlay0
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some((rect.w - 16.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if enabled {
            f.hit(target, rect);
        }
    }

    /// The torrent the selection names, if it still exists.
    fn selected(&self) -> Option<&ManagedTorrent> {
        self.selected_torrent
            .and_then(|id| self.torrents.iter().find(|t| t.id == id))
    }

    fn render_header(&self, f: &mut Frame<Target>, width: f32) {
        self.palette.push_surface(
            f,
            0.0,
            0.0,
            width,
            HEADER_H,
            0.0,
            Surface::Strip(Edge::Bottom),
        );
        f.push(RenderCommand::Text {
            x: 16.0,
            y: 14.0,
            text: "Torrent".to_string(),
            font_size: 18.0,
            color: self.palette.ink(self.palette.blue),
            font_weight: FontWeightHint::Bold,
            max_width: Some(96.0),
            overflow: TextOverflow::Ellipsis,
        });

        // The toolbar. Six buttons were drawn here and none could be pressed.
        let selected = self.selected();
        let can_pause = selected.is_some_and(|t| {
            matches!(
                t.state,
                TorrentState::Downloading | TorrentState::Seeding | TorrentState::CheckingFiles
            )
        });
        let can_resume = selected.is_some_and(ManagedTorrent::can_start);
        let any_running = self.torrents.iter().any(|t| {
            matches!(
                t.state,
                TorrentState::Downloading | TorrentState::Seeding | TorrentState::CheckingFiles
            )
        });
        let any_paused = self.torrents.iter().any(ManagedTorrent::can_start);
        let mut bx = 120.0;
        for (label, target, enabled) in [
            ("Open\u{2026}", Target::Open, true),
            ("Add magnet\u{2026}", Target::AddMagnet, true),
            ("Remove", Target::Remove, selected.is_some()),
            ("Pause", Target::Pause, can_pause),
            ("Resume", Target::Resume, can_resume),
            ("Pause all", Target::PauseAll, any_running),
            ("Resume all", Target::ResumeAll, any_paused),
        ] {
            let bw = text::padded_width(label, 12.0, 12.0, FontWeightHint::Regular);
            self.button(f, Rect::new(bx, 8.0, bw, 32.0), label, target, enabled);
            bx += bw + 8.0;
        }

        // The search box and the keys, at the right.
        let keys = Rect::new(width - 12.0 - 84.0, 8.0, 84.0, 32.0);
        self.button(f, keys, "Keys (F1)", Target::Help, true);
        let search = Rect::new((keys.x - 8.0 - 220.0).max(bx), 8.0, 220.0, 32.0);
        self.palette.push_surface(
            f,
            search.x,
            search.y,
            search.w,
            search.h,
            6.0,
            Surface::Card,
        );
        if self.search_active {
            f.push(RenderCommand::StrokeRect {
                x: search.x,
                y: search.y,
                width: search.w,
                height: search.h,
                color: self.palette.blue,
                line_width: 2.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }
        let placeholder = self.search_query.is_empty() && !self.search_active;
        f.push(RenderCommand::Text {
            x: search.x + 10.0,
            y: search.y + 9.0,
            text: if placeholder {
                String::from("Search names and labels  ( / )")
            } else {
                self.search_query.clone()
            },
            font_size: 12.0,
            color: if placeholder {
                self.palette.subtext0
            } else {
                self.palette.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(search.w - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        if self.search_active {
            let typed = text::measure(&self.search_query, 12.0, FontWeightHint::Regular);
            f.push(RenderCommand::FillRect {
                x: search.x + 10.0 + typed.min(search.w - 22.0),
                y: search.y + 8.0,
                width: guitk::textedit::CARET_WIDTH,
                height: 16.0,
                color: self.palette.text,
                corner_radii: CornerRadii::ZERO,
            });
        }
        f.hit(Target::Search, search);
    }

    fn render_sidebar(&self, f: &mut Frame<Target>, height: f32) {
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: HEADER_H,
            width: SIDEBAR_W,
            height: (height - HEADER_H - STATUS_H).max(0.0),
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });
        // The filters. They were drawn, highlighted, and could not be chosen.
        let mut fy = HEADER_H + 8.0;
        for filter in FILTERS {
            let row = Rect::new(4.0, fy, SIDEBAR_W - 8.0, 28.0);
            let target = Target::Filter(filter);
            let chosen = filter == self.filter;
            if chosen || self.hover == Some(target) {
                f.push(RenderCommand::FillRect {
                    x: row.x,
                    y: row.y,
                    width: row.w,
                    height: row.h,
                    color: if chosen {
                        self.palette.surface0
                    } else {
                        self.palette.crust
                    },
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            let count = self
                .torrents
                .iter()
                .filter(|t| filter.matches(t.state))
                .count();
            f.push(RenderCommand::Text {
                x: 16.0,
                y: fy + 7.0,
                text: format!("{} ({count})", filter.label()),
                font_size: 12.0,
                color: if chosen {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext1
                },
                font_weight: if chosen {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_W - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(target, row);
            fy += 32.0;
        }

        // The labels: a press shows only the transfers under one, and a
        // second press all of them again. Nothing could choose one, and
        // nothing could give a transfer a label to be chosen by.
        fy += 16.0;
        f.push(RenderCommand::Text {
            x: 16.0,
            y: fy,
            text: "Labels".to_string(),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_W - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        fy += 20.0;
        for (i, label) in self.labels.iter().enumerate() {
            let row = Rect::new(4.0, fy, SIDEBAR_W - 8.0, 24.0);
            let target = Target::Label(i);
            let chosen = self.selected_label.as_ref() == Some(label);
            if chosen || self.hover == Some(target) {
                f.push(RenderCommand::FillRect {
                    x: row.x,
                    y: row.y,
                    width: row.w,
                    height: row.h,
                    color: if chosen {
                        self.palette.surface0
                    } else {
                        self.palette.crust
                    },
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            let count = self.torrents.iter().filter(|t| &t.label == label).count();
            f.push(RenderCommand::Text {
                x: 16.0,
                y: fy + 5.0,
                text: format!("{label} ({count})"),
                font_size: 12.0,
                color: if chosen {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_W - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(target, row);
            fy += 28.0;
        }
    }

    fn render_tabs(&self, f: &mut Frame<Target>, width: f32) {
        f.push(RenderCommand::FillRect {
            x: SIDEBAR_W,
            y: HEADER_H,
            width: (width - SIDEBAR_W).max(0.0),
            height: TAB_H,
            color: self.palette.crust,
            corner_radii: CornerRadii::ZERO,
        });
        let mut tx = SIDEBAR_W + 8.0;
        for tab in Tab::ALL {
            let is_active = tab == self.active_tab;
            let tw = text::padded_width_any_weight(tab.label(), 10.0, 12.0);
            let rect = Rect::new(tx, HEADER_H + 4.0, tw, TAB_H - 4.0);
            let target = Target::Tab(tab);
            if is_active || self.hover == Some(target) {
                f.push(RenderCommand::FillRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.w,
                    height: rect.h,
                    color: if is_active {
                        self.palette.base
                    } else {
                        self.palette.mantle
                    },
                    corner_radii: CornerRadii {
                        top_left: 6.0,
                        top_right: 6.0,
                        bottom_left: 0.0,
                        bottom_right: 0.0,
                    },
                });
            }
            f.push(RenderCommand::Text {
                x: tx + 10.0,
                y: HEADER_H + 12.0,
                text: tab.label().to_string(),
                font_size: 12.0,
                color: if is_active {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tw),
                overflow: TextOverflow::Clip,
            });
            f.hit(target, rect);
            tx += tw + 4.0;
        }
    }

    fn render_status(&self, f: &mut Frame<Target>, width: f32, height: f32) {
        let sy = height - STATUS_H;
        self.palette
            .push_surface(f, 0.0, sy, width, STATUS_H, 0.0, Surface::Card);
        let (downloading, seeding, total, dl_speed, ul_speed) = self.stats();
        f.push(RenderCommand::Text {
            x: 12.0,
            y: sy + 8.0,
            text: format!(
                "\u{2193} {}  \u{2191} {}  |  {} downloading, {} seeding, {} total  |  sorted by {} {} (C, R)  |  {}",
                format_speed(dl_speed),
                format_speed(ul_speed),
                downloading,
                seeding,
                total,
                self.sort_column.label(),
                if self.sort_ascending {
                    "ascending"
                } else {
                    "descending"
                },
                self.last_open.as_deref().unwrap_or(&self.status_message)
            ),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((width - 24.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Where the transfer rows are drawn in the content area `content`, and
    /// how many whole rows fit: under the notice and the column heads.
    fn transfer_pane(content: Rect) -> (Rect, usize) {
        let top = content.y + NOTICE_H + 24.0;
        let pane = Rect::new(content.x, top, content.w, (content.bottom() - top).max(0.0));
        (pane, ((pane.h / ROW_H).floor().max(1.0)) as usize)
    }

    /// The content area at the window's own size.
    fn content_rect(&self) -> Rect {
        Rect::new(
            SIDEBAR_W,
            HEADER_H + TAB_H,
            (self.win_width - SIDEBAR_W).max(0.0),
            (self.win_height - HEADER_H - TAB_H - STATUS_H).max(0.0),
        )
    }

    fn render_transfers(&self, f: &mut Frame<Target>, content: Rect) {
        let (x, y, w) = (content.x, content.y, content.w);
        // What this client cannot do, where it can be read. It was drawn
        // first, at the top of the window, and then painted over by the
        // background and the header.
        for (i, line) in TRANSFER_NOTE_LINES.iter().enumerate() {
            f.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 6.0 + i as f32 * 14.0,
                text: (*line).to_string(),
                color: if i == 0 {
                    self.palette.ink(self.palette.yellow)
                } else {
                    self.palette.subtext0
                },
                font_size: if i == 0 { 12.0 } else { 10.0 },
                font_weight: if i == 0 {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some((w - 24.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // The column heads: a press sorts by one, and a second press on the
        // one already sorted by reverses it.
        let head_y = y + NOTICE_H;
        let mut cx = x + 8.0;
        for (column, label, cw) in TRANSFER_COLUMNS {
            let rect = Rect::new(cx - 4.0, head_y, cw + 8.0, 22.0);
            let target = Target::SortBy(column);
            let sorted = column == self.sort_column;
            if self.hover == Some(target) {
                f.push(RenderCommand::FillRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.w,
                    height: rect.h,
                    color: self.palette.surface0,
                    corner_radii: CornerRadii::all(3.0),
                });
            }
            f.push(RenderCommand::Text {
                x: cx,
                y: head_y + 4.0,
                text: if sorted {
                    format!(
                        "{label} {}",
                        if self.sort_ascending {
                            "\u{25B2}"
                        } else {
                            "\u{25BC}"
                        }
                    )
                } else {
                    label.to_string()
                },
                font_size: 11.0,
                color: if sorted {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some(cw),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(target, rect);
            cx += cw + 8.0;
        }

        let (pane, rows) = Self::transfer_pane(content);
        f.hit(Target::TransferList, pane);
        let filtered = self.filtered_torrents();
        if filtered.is_empty() {
            let why = if self.torrents.is_empty() {
                String::from(
                    "No torrents. Open\u{2026} (Ctrl+O) reads a .torrent file; Add magnet\u{2026} (Ctrl+U) takes a magnet link.",
                )
            } else {
                String::from("Nothing here matches the filter, the label or the search.")
            };
            f.push(RenderCommand::Text {
                x: pane.x + 16.0,
                y: pane.y + 20.0,
                text: why,
                font_size: 13.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((pane.w - 32.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        for (shown, torrent) in filtered
            .iter()
            .skip(self.transfer_scroll)
            .take(rows.saturating_add(1))
            .enumerate()
        {
            let ry = pane.y + shown as f32 * ROW_H;
            let row = Rect::new(x + 4.0, ry, (w - 8.0).max(0.0), ROW_H - 2.0);
            self.render_transfer_row(f, torrent, row);
            f.hit(Target::Row(torrent.id), row);
        }
        if filtered.len() > rows {
            let last = filtered.len().saturating_sub(rows).max(1);
            let thumb_h = (pane.h * rows as f32 / filtered.len() as f32)
                .max(16.0)
                .min(pane.h);
            let thumb_y =
                pane.y + (pane.h - thumb_h) * (self.transfer_scroll.min(last) as f32 / last as f32);
            f.push(RenderCommand::FillRect {
                x: pane.right() - 5.0,
                y: thumb_y,
                width: 4.0,
                height: thumb_h,
                color: self.palette.surface2,
                corner_radii: CornerRadii::all(2.0),
            });
        }
    }

    fn render_transfer_row(&self, f: &mut Frame<Target>, torrent: &ManagedTorrent, row: Rect) {
        let is_sel = self.selected_torrent == Some(torrent.id);
        if is_sel {
            self.palette
                .push_surface(f, row.x, row.y, row.w, row.h, 4.0, Surface::Card);
        } else if self.hover == Some(Target::Row(torrent.id)) {
            f.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: self.palette.mantle,
                corner_radii: CornerRadii::all(4.0),
            });
        }
        let ry = row.y;
        let mut cx = row.x + 4.0;

        f.push(RenderCommand::Text {
            x: cx,
            y: ry + 6.0,
            text: torrent.name.clone(),
            font_size: 12.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(250.0),
            overflow: TextOverflow::Ellipsis,
        });
        if !torrent.label.is_empty() {
            f.push(RenderCommand::Text {
                x: cx,
                y: ry + 24.0,
                text: torrent.label.clone(),
                font_size: 10.0,
                color: self.palette.ink(self.palette.mauve),
                font_weight: FontWeightHint::Regular,
                max_width: Some(250.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        cx += 258.0;

        f.push(RenderCommand::Text {
            x: cx,
            y: ry + 6.0,
            text: format_size(torrent.total_size),
            font_size: 12.0,
            color: self.palette.subtext1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });
        cx += 88.0;

        let bar_w = 112.0;
        let bar_h = 12.0;
        f.push(RenderCommand::FillRect {
            x: cx,
            y: ry + 8.0,
            width: bar_w,
            height: bar_h,
            color: self.palette.surface1,
            corner_radii: CornerRadii::all(3.0),
        });
        let progress = torrent.progress();
        let fill_w = (bar_w * progress as f32 / 100.0).min(bar_w);
        if fill_w > 0.5 {
            f.push(RenderCommand::FillRect {
                x: cx,
                y: ry + 8.0,
                width: fill_w,
                height: bar_h,
                color: match torrent.state {
                    TorrentState::Downloading => self.palette.blue,
                    TorrentState::Seeding => self.palette.green,
                    TorrentState::Paused => self.palette.yellow,
                    TorrentState::Error => self.palette.red,
                    _ => self.palette.teal,
                },
                corner_radii: CornerRadii::all(3.0),
            });
        }
        f.push(RenderCommand::Text {
            x: cx,
            y: ry + 24.0,
            text: format!("{progress:.1}%"),
            font_size: 10.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(bar_w),
            overflow: TextOverflow::Ellipsis,
        });
        cx += 128.0;

        // A transfer set going with nobody to transfer from says so, rather
        // than "Downloading" in blue at 0% for good.
        let (status, status_color) = match torrent.state {
            TorrentState::Downloading if torrent.peers.is_empty() => {
                (String::from("Looking for peers"), self.palette.subtext0)
            }
            TorrentState::Downloading => (
                torrent.state.to_string(),
                self.palette.ink(self.palette.blue),
            ),
            TorrentState::Seeding => (
                torrent.state.to_string(),
                self.palette.ink(self.palette.green),
            ),
            TorrentState::Paused => (
                torrent.state.to_string(),
                self.palette.ink(self.palette.yellow),
            ),
            TorrentState::Error => (
                torrent.state.to_string(),
                self.palette.ink(self.palette.red),
            ),
            TorrentState::Complete => (
                torrent.state.to_string(),
                self.palette.ink(self.palette.teal),
            ),
            _ => (torrent.state.to_string(), self.palette.subtext0),
        };
        f.push(RenderCommand::Text {
            x: cx,
            y: ry + 6.0,
            text: status,
            font_size: 12.0,
            color: status_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });
        cx += 88.0;

        for (value, ink, width) in [
            (
                format_speed(torrent.download_speed.speed_bps()),
                self.palette.ink(self.palette.teal),
                80.0,
            ),
            (
                format_speed(torrent.upload_speed.speed_bps()),
                self.palette.ink(self.palette.peach),
                80.0,
            ),
            (
                format!("{:.2}", torrent.ratio()),
                self.palette.subtext1,
                60.0,
            ),
            (
                torrent
                    .eta_seconds()
                    .map_or_else(|| "\u{221E}".to_string(), format_duration),
                self.palette.subtext0,
                80.0,
            ),
        ] {
            f.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: value,
                font_size: 12.0,
                color: ink,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
            cx += width + 8.0;
        }
    }

    /// A line saying there is nothing chosen to show.
    fn render_nothing_chosen(&self, f: &mut Frame<Target>, content: Rect, what: &str) {
        f.push(RenderCommand::Text {
            x: content.x + 16.0,
            y: content.y + 20.0,
            text: format!("Select a torrent to view {what}"),
            font_size: 13.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((content.w - 32.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_details(&self, f: &mut Frame<Target>, content: Rect) {
        let Some(torrent) = self.selected() else {
            self.render_nothing_chosen(f, content, "details");
            return;
        };
        let (x, y, w) = (content.x, content.y, content.w);
        let label_x = x + 16.0;
        let value_x = x + 160.0;
        let max_val_w = (w - 180.0).max(0.0);

        let fields: Vec<(&str, String, Option<Target>)> = vec![
            ("Name:", torrent.name.clone(), None),
            ("Save Path:", torrent.save_path.display().to_string(), None),
            ("Total Size:", format_size(torrent.total_size), None),
            ("Downloaded:", format_size(torrent.downloaded), None),
            ("Uploaded:", format_size(torrent.uploaded), None),
            ("Ratio:", format!("{:.3}", torrent.ratio()), None),
            ("Status:", torrent.state.to_string(), None),
            ("Progress:", format!("{:.1}%", torrent.progress()), None),
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
                None,
            ),
            ("Peers:", format!("{} connected", torrent.peers.len()), None),
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
                None,
            ),
            (
                "Comment:",
                torrent
                    .metainfo
                    .as_ref()
                    .and_then(|m| m.comment.clone())
                    .unwrap_or_else(|| "N/A".to_string()),
                None,
            ),
            (
                "Created By:",
                torrent
                    .metainfo
                    .as_ref()
                    .and_then(|m| m.created_by.clone())
                    .unwrap_or_else(|| "N/A".to_string()),
                None,
            ),
            (
                "Sequential:",
                format!(
                    "{}   (press, or S)",
                    if torrent.sequential_download {
                        "Yes"
                    } else {
                        "No"
                    }
                ),
                Some(Target::ToggleSequential),
            ),
            (
                "Label:",
                format!(
                    "{}   (press, or L)",
                    if torrent.label.is_empty() {
                        "None"
                    } else {
                        torrent.label.as_str()
                    }
                ),
                Some(Target::CycleLabel),
            ),
        ];

        let mut dy = y + 12.0;
        for (label, value, target) in &fields {
            f.push(RenderCommand::Text {
                x: label_x,
                y: dy,
                text: (*label).to_string(),
                font_size: 12.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(140.0),
                overflow: TextOverflow::Ellipsis,
            });
            if let Some(target) = target {
                let rect = Rect::new(value_x - 6.0, dy - 3.0, max_val_w.min(320.0), 20.0);
                f.push(RenderCommand::FillRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.w,
                    height: rect.h,
                    color: if self.hover == Some(*target) {
                        self.palette.surface1
                    } else {
                        self.palette.surface0
                    },
                    corner_radii: CornerRadii::all(4.0),
                });
                f.hit(*target, rect);
            }
            f.push(RenderCommand::Text {
                x: value_x,
                y: dy,
                text: value.clone(),
                font_size: 12.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_val_w),
                overflow: TextOverflow::Ellipsis,
            });
            dy += 22.0;
        }
    }

    fn render_peers(&self, f: &mut Frame<Target>, content: Rect) {
        let Some(torrent) = self.selected() else {
            self.render_nothing_chosen(f, content, "peers");
            return;
        };
        let (x, y, w, h) = (content.x, content.y, content.w, content.h);
        let table = Table::new(PEER_COLUMNS, x);
        f.draw_with(|c| table.header(c, y + 4.0, self.palette.subtext0, TABLE_FONT));
        let mut py = y + 24.0;
        for peer in &torrent.peers {
            if py + 24.0 > y + h {
                break;
            }
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
            let cells: [(String, guitk::Color, Fit); 6] = [
                (
                    format!("{}:{}", peer.address, peer.port),
                    self.palette.subtext1,
                    Fit::Start,
                ),
                // A peer's client name is whatever the peer says it is: it
                // arrives in the handshake from an untrusted party, so it is
                // fitted like any other wire-supplied string.
                (peer.client_name.clone(), self.palette.text, Fit::Start),
                (
                    format_speed(peer.download_rate),
                    self.palette.ink(self.palette.teal),
                    Fit::Start,
                ),
                (
                    format_speed(peer.upload_rate),
                    self.palette.ink(self.palette.peach),
                    Fit::Start,
                ),
                (
                    format_size(peer.downloaded),
                    self.palette.subtext0,
                    Fit::Start,
                ),
                (flags, self.palette.subtext0, Fit::Start),
            ];
            debug_assert_eq!(
                cells.len(),
                table.len(),
                "a cell with no column is positioned past the table and drawn empty",
            );
            f.draw_with(|c| {
                for (i, (cell, color, fit)) in cells.iter().enumerate() {
                    table.cell(c, i, py, cell, *color, TABLE_FONT, *fit);
                }
            });
            py += 24.0;
        }
        if torrent.peers.is_empty() {
            f.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 40.0,
                text: String::from("No peers: this client has no network to find any on."),
                font_size: 13.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((w - 32.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_files(&self, f: &mut Frame<Target>, content: Rect) {
        let Some(torrent) = self.selected() else {
            self.render_nothing_chosen(f, content, "files");
            return;
        };
        let (x, y, h) = (content.x, content.y, content.h);
        let meta_files = torrent
            .metainfo
            .as_ref()
            .map_or(&[] as &[TorrentFile], |m| &m.files);
        let table = Table::new(FILE_COLUMNS, x);
        f.draw_with(|c| table.header(c, y + 4.0, self.palette.subtext0, TABLE_FONT));
        if meta_files.is_empty() {
            f.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 32.0,
                text: String::from(
                    "No file list: a magnet link carries none, and the metadata that would comes from peers.",
                ),
                font_size: 12.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((content.w - 32.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let priority_x = x + FILE_COLUMNS.iter().take(2).map(|c| c.width).sum::<f32>();
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
            let prio_color = match priority {
                FilePriority::Skip => self.palette.subtext0,
                FilePriority::Low => self.palette.ink(self.palette.yellow),
                FilePriority::Normal => self.palette.text,
                FilePriority::High => self.palette.ink(self.palette.green),
            };
            // A press on the priority steps it: Low, Normal, High, Skip. It
            // was shown and could not be changed.
            let target = Target::FilePriority(i);
            let chip = Rect::new(priority_x, fy - 2.0, 80.0, 20.0);
            f.push(RenderCommand::FillRect {
                x: chip.x,
                y: chip.y,
                width: chip.w,
                height: chip.h,
                color: if self.hover == Some(target) {
                    self.palette.surface1
                } else {
                    self.palette.surface0
                },
                corner_radii: CornerRadii::all(4.0),
            });
            f.hit(target, chip);
            // The path is elided from the *front*: it comes from the torrent's
            // metainfo and is routinely longer than the column, and what
            // identifies a file is its name, not the directory chain above it.
            // Cut the usual way, every episode in a season reads identically.
            let cells: [(String, guitk::Color, Fit); 3] = [
                (file.path.clone(), self.palette.text, Fit::End),
                (format_size(file.length), self.palette.subtext1, Fit::Start),
                (priority.to_string(), prio_color, Fit::Start),
            ];
            debug_assert_eq!(
                cells.len(),
                table.len(),
                "a cell with no column is positioned past the table and drawn empty",
            );
            f.draw_with(|c| {
                for (i, (cell, color, fit)) in cells.iter().enumerate() {
                    table.cell(c, i, fy, cell, *color, TABLE_FONT, *fit);
                }
            });
            fy += 22.0;
        }
    }

    fn render_trackers(&self, f: &mut Frame<Target>, content: Rect) {
        let Some(torrent) = self.selected() else {
            self.render_nothing_chosen(f, content, "trackers");
            return;
        };
        let (x, y, h) = (content.x, content.y, content.h);
        let table = Table::new(TRACKER_COLUMNS, x);
        f.draw_with(|c| table.header(c, y + 4.0, self.palette.subtext0, TABLE_FONT));
        let mut ty = y + 24.0;
        for tracker in &torrent.trackers {
            if ty + 22.0 > y + h {
                break;
            }
            let status_color = match tracker.status {
                TrackerStatus::Working => self.palette.ink(self.palette.green),
                TrackerStatus::Updating => self.palette.ink(self.palette.blue),
                TrackerStatus::Error => self.palette.ink(self.palette.red),
                _ => self.palette.subtext0,
            };
            // A tracker URL is elided from the front for the same reason a file
            // path is: the announce path at the end is what distinguishes two
            // trackers on the same host, and cutting the usual way keeps only
            // the scheme and hostname they share.
            let cells: [(String, guitk::Color, Fit); 5] = [
                (tracker.url.clone(), self.palette.text, Fit::End),
                (tracker.status.to_string(), status_color, Fit::Start),
                (
                    tracker.seeders.to_string(),
                    self.palette.ink(self.palette.green),
                    Fit::Start,
                ),
                (
                    tracker.leechers.to_string(),
                    self.palette.ink(self.palette.peach),
                    Fit::Start,
                ),
                (tracker.tier.to_string(), self.palette.subtext0, Fit::Start),
            ];
            debug_assert_eq!(
                cells.len(),
                table.len(),
                "a cell with no column is positioned past the table and drawn empty",
            );
            f.draw_with(|c| {
                for (i, (cell, color, fit)) in cells.iter().enumerate() {
                    table.cell(c, i, ty, cell, *color, TABLE_FONT, *fit);
                }
            });
            ty += 22.0;
        }
    }

    fn render_settings(&self, f: &mut Frame<Target>, content: Rect) {
        let (x, y, w) = (content.x, content.y, content.w);
        let mut sy = y + 12.0;
        let label_x = x + 16.0;
        f.push(RenderCommand::Text {
            x: label_x,
            y: sy,
            text: SETTINGS_NOT_APPLIED.to_owned(),
            font_size: 11.0,
            color: self.palette.ink(self.palette.yellow),
            font_weight: FontWeightHint::Bold,
            max_width: Some((w - 32.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        sy += 24.0;
        let value_x = x + 220.0;
        let max_val_w = (w - 240.0).max(0.0);
        let on_off = |on: bool| if on { "Enabled" } else { "Disabled" }.to_string();
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
                self.settings.default_save_path.display().to_string(),
            ),
            ("Encryption:", self.settings.encryption_mode.to_string()),
            ("DHT:", on_off(self.settings.dht_enabled)),
            ("PEX:", on_off(self.settings.pex_enabled)),
            ("\u{B5}TP:", on_off(self.settings.enable_utp)),
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
            f.push(RenderCommand::Text {
                x: label_x,
                y: sy,
                text: (*label).to_string(),
                font_size: 12.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: value_x,
                y: sy,
                text: value.clone(),
                font_size: 12.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_val_w),
                overflow: TextOverflow::Ellipsis,
            });
            sy += 22.0;
        }
    }

    /// Where the magnet dialog's card is.
    fn magnet_card(width: f32, height: f32) -> Rect {
        let w = 600.0_f32.min(width - 24.0).max(0.0);
        let h = 190.0_f32.min(height - 24.0).max(0.0);
        Rect::new((width - w) / 2.0, (height - h) / 2.0, w, h)
    }

    /// The dialog that takes a magnet link. `add_magnet` had no caller, and
    /// the fields it would have filled were written and never read.
    fn render_magnet_dialog(&self, f: &mut Frame<Target>, width: f32, height: f32) {
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: guitk::Color::rgba(0, 0, 0, 150),
            corner_radii: CornerRadii::ZERO,
        });
        // Around and behind the card a press does nothing: the dialog is
        // modal, and a press reaching a row behind it would change what it
        // is about.
        f.hit(Target::DialogBackdrop, Rect::new(0.0, 0.0, width, height));
        let card = Self::magnet_card(width, height);
        self.palette
            .push_surface(f, card.x, card.y, card.w, card.h, 12.0, Surface::Card);
        f.push(RenderCommand::Text {
            x: card.x + 20.0,
            y: card.y + 18.0,
            text: String::from("Add a magnet link"),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some((card.w - 40.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        let field = Rect::new(card.x + 20.0, card.y + 50.0, (card.w - 40.0).max(0.0), 32.0);
        self.palette
            .push_surface(f, field.x, field.y, field.w, field.h, 4.0, Surface::Card);
        f.push(RenderCommand::StrokeRect {
            x: field.x,
            y: field.y,
            width: field.w,
            height: field.h,
            color: self.palette.blue,
            line_width: 2.0,
            corner_radii: CornerRadii::all(4.0),
        });
        if self.magnet_input.text().is_empty() {
            f.push(RenderCommand::Text {
                x: field.x + 8.0,
                y: field.y + 9.0,
                text: String::from("magnet:?xt=urn:btih:\u{2026}"),
                font_size: 12.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((field.w - 16.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let mut tree = RenderTree::new();
        guitk::textedit::draw(
            &mut tree,
            &guitk::textedit::SingleLine {
                text: self.magnet_input.text(),
                cursor: self.magnet_input.cursor(),
                selection_anchor: self.magnet_input.selection_anchor(),
                focused: true,
                x: field.x + 8.0,
                y: field.y + 7.0,
                width: (field.w - 16.0).max(0.0),
                line_height: 18.0,
                font_size: 12.0,
                weight: FontWeightHint::Regular,
                color: self.palette.text,
                selection_bg: self.palette.blue,
                selection_fg: self.palette.crust,
                caret_width: guitk::textedit::CARET_WIDTH,
            },
        );
        f.extend(tree.commands);
        f.hit(Target::MagnetField, field);
        let (note, ink) = match &self.magnet_error {
            Some(why) => (why.clone(), self.palette.ink(self.palette.red)),
            None => (
                format!(
                    "Saved to {} once its files are known -- which this client cannot learn yet.",
                    self.add_save_path.display()
                ),
                self.palette.subtext0,
            ),
        };
        f.push(RenderCommand::Text {
            x: card.x + 20.0,
            y: field.bottom() + 12.0,
            text: note,
            font_size: 11.0,
            color: ink,
            font_weight: FontWeightHint::Regular,
            max_width: Some((card.w - 40.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        let by = card.bottom() - 48.0;
        self.button(
            f,
            Rect::new(card.right() - 20.0 - 90.0, by, 90.0, 30.0),
            "Cancel",
            Target::MagnetCancel,
            true,
        );
        self.button(
            f,
            Rect::new(card.right() - 20.0 - 90.0 - 8.0 - 90.0, by, 90.0, 30.0),
            "Add",
            Target::MagnetAdd,
            !self.magnet_input.text().trim().is_empty(),
        );
    }

    // ── The pointer ─────────────────────────────────────────────────

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame(self.win_width, self.win_height).hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let Some(target) = self
                    .frame(self.win_width, self.win_height)
                    .hit_test(event.x, event.y)
                else {
                    return EventResult::Ignored;
                };
                self.press(target)
            }
            MouseEventKind::Move => {
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Leave => {
                if self.hover.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Scroll { dy, .. } => self.wheel_at(event.x, event.y, dy),
            _ => EventResult::Ignored,
        }
    }

    /// A left press on `target`.
    fn press(&mut self, target: Target) -> EventResult {
        // A press anywhere but the search box takes the keys out of it.
        if target != Target::Search {
            self.search_active = false;
        }
        match target {
            Target::HelpCard => self.show_help = false,
            Target::Help => self.show_help = true,
            Target::Open => self.picker.open_to_read(),
            Target::AddMagnet => self.open_magnet_dialog(),
            Target::Remove => {
                let Some(id) = self.selected_torrent else {
                    return EventResult::Ignored;
                };
                self.remove_torrent(id, false);
                self.reanchor_selection();
            }
            Target::Pause => {
                let Some(id) = self.selected_torrent else {
                    return EventResult::Ignored;
                };
                self.pause_torrent(id);
            }
            Target::Resume => {
                let Some(id) = self.selected_torrent else {
                    return EventResult::Ignored;
                };
                self.resume_torrent(id);
            }
            Target::PauseAll => self.pause_all(),
            Target::ResumeAll => self.resume_all(),
            Target::Search => self.search_active = true,
            Target::Filter(filter) => return self.set_filter(filter),
            Target::Label(index) => {
                let Some(label) = self.labels.get(index).cloned() else {
                    return EventResult::Ignored;
                };
                self.selected_label = if self.selected_label.as_ref() == Some(&label) {
                    None
                } else {
                    Some(label)
                };
                self.transfer_scroll = 0;
            }
            Target::Tab(tab) => {
                if tab == self.active_tab {
                    return EventResult::Ignored;
                }
                self.active_tab = tab;
            }
            Target::SortBy(column) => {
                if column == self.sort_column {
                    self.sort_ascending = !self.sort_ascending;
                } else {
                    self.sort_column = column;
                }
            }
            // A press chooses a transfer; a second press on it shows its
            // details.
            Target::Row(id) => {
                if self.selected_torrent == Some(id) {
                    self.active_tab = Tab::Details;
                } else {
                    self.selected_torrent = Some(id);
                }
            }
            Target::CycleLabel => return self.cycle_label(),
            Target::ToggleSequential => {
                let Some(id) = self.selected_torrent else {
                    return EventResult::Ignored;
                };
                if let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) {
                    t.toggle_sequential();
                }
            }
            Target::FilePriority(index) => {
                let Some(id) = self.selected_torrent else {
                    return EventResult::Ignored;
                };
                let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) else {
                    return EventResult::Ignored;
                };
                let next = match t.file_priorities.get(index) {
                    Some(FilePriority::Low) => FilePriority::Normal,
                    Some(FilePriority::Normal) => FilePriority::High,
                    Some(FilePriority::High) => FilePriority::Skip,
                    Some(FilePriority::Skip) => FilePriority::Low,
                    None => return EventResult::Ignored,
                };
                t.set_file_priority(index, next);
            }
            Target::MagnetAdd => self.add_typed_magnet(),
            Target::MagnetCancel => self.close_magnet_dialog(),
            Target::MagnetField | Target::DialogBackdrop | Target::TransferList => {
                return EventResult::Ignored;
            }
        }
        EventResult::Consumed
    }

    /// The wheel over the transfer list.
    fn wheel_at(&mut self, x: f32, y: f32, dy: f32) -> EventResult {
        if !matches!(
            self.target_at(x, y),
            Some(Target::TransferList | Target::Row(_))
        ) {
            return EventResult::Ignored;
        }
        let rows = self.wheel.rows(dy);
        let (_, visible) = Self::transfer_pane(self.content_rect());
        let last = self.filtered_torrents().len().saturating_sub(visible);
        let now = self.transfer_scroll;
        let next = if rows < 0 {
            now.saturating_sub(rows.unsigned_abs())
        } else {
            now.saturating_add(rows.unsigned_abs())
        }
        .min(last);
        if next == now {
            return EventResult::Ignored;
        }
        self.transfer_scroll = next;
        EventResult::Consumed
    }

    /// Scroll the transfer list so the selected transfer is on screen, and
    /// the list not past its end.
    fn keep_selection_visible(&mut self) {
        let (_, visible) = Self::transfer_pane(self.content_rect());
        let ids: Vec<u32> = self.filtered_torrents().iter().map(|t| t.id).collect();
        if let Some(at) = self
            .selected_torrent
            .and_then(|id| ids.iter().position(|v| *v == id))
        {
            if at < self.transfer_scroll {
                self.transfer_scroll = at;
            } else if at >= self.transfer_scroll.saturating_add(visible) {
                self.transfer_scroll = at.saturating_add(1).saturating_sub(visible);
            }
        }
        self.transfer_scroll = self.transfer_scroll.min(ids.len().saturating_sub(visible));
    }

    /// Give the selected transfer the next label, and then none.
    fn cycle_label(&mut self) -> EventResult {
        let Some(id) = self.selected_torrent else {
            return EventResult::Ignored;
        };
        let labels = self.labels.clone();
        let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) else {
            return EventResult::Ignored;
        };
        let at = labels.iter().position(|l| *l == t.label);
        t.label = match at {
            None => labels.first().cloned().unwrap_or_default(),
            Some(i) => labels.get(i.saturating_add(1)).cloned().unwrap_or_default(),
        };
        self.status_message = if t.label.is_empty() {
            format!("{}: no label", t.name)
        } else {
            format!("{}: labelled {}", t.name, t.label)
        };
        EventResult::Consumed
    }

    fn open_magnet_dialog(&mut self) {
        self.show_add_dialog = true;
        self.magnet_input.clear();
        self.magnet_error = None;
        self.add_save_path
            .clone_from(&self.settings.default_save_path);
    }

    fn close_magnet_dialog(&mut self) {
        self.show_add_dialog = false;
        self.magnet_error = None;
    }

    /// Add the magnet link typed into the dialog, or say what is wrong with
    /// it and keep it there to be mended.
    fn add_typed_magnet(&mut self) {
        let typed = self.magnet_input.text().trim().to_owned();
        match MagnetLink::parse(&typed) {
            Ok(magnet) => {
                let path = self.add_save_path.clone();
                let id = self.add_magnet(magnet, Some(&path));
                self.status_message = String::from(
                    "Added; a magnet link's files are learned from peers, which this client cannot ask yet",
                );
                self.selected_torrent = Some(id);
                self.close_magnet_dialog();
                self.keep_selection_visible();
            }
            Err(why) => {
                self.magnet_error = Some(format!("Not a magnet link this client reads: {why}"));
            }
        }
    }

    /// Keys while the magnet dialog is up.
    fn handle_dialog_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.close_magnet_dialog();
                EventResult::Consumed
            }
            Key::Enter => {
                if self.magnet_input.text().trim().is_empty() {
                    return EventResult::Ignored;
                }
                self.add_typed_magnet();
                EventResult::Consumed
            }
            _ => {
                let clipboard = self.clipboard.clone();
                let before = (
                    self.magnet_input.text().to_owned(),
                    self.magnet_input.cursor(),
                    self.magnet_input.selection_anchor(),
                );
                let copied = textline::apply_key(
                    &mut self.magnet_input,
                    key,
                    MAX_MAGNET_CHARS,
                    &clipboard,
                    12.0,
                )
                .copied;
                let did_copy = copied.is_some();
                if let Some(text) = copied {
                    self.clipboard = text;
                }
                let changed = self.magnet_input.text() != before.0;
                if changed {
                    self.magnet_error = None;
                }
                if changed
                    || did_copy
                    || self.magnet_input.cursor() != before.1
                    || self.magnet_input.selection_anchor() != before.2
                {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    /// Keys while the search box has them.
    fn handle_search_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.search_active = false;
                self.search_query.clear();
            }
            Key::Enter | Key::Tab => self.search_active = false,
            Key::Backspace => {
                if self.search_query.pop().is_none() {
                    return EventResult::Ignored;
                }
            }
            _ => {
                if key.modifiers.ctrl {
                    return EventResult::Ignored;
                }
                let typed: String = key.text.chars().filter(|c| !c.is_control()).collect();
                if typed.is_empty() {
                    return EventResult::Ignored;
                }
                self.search_query.push_str(&typed);
            }
        }
        self.transfer_scroll = 0;
        EventResult::Consumed
    }
}

// ─── Formatting helpers ──────────────────────────────────────────────

/// Format bytes as human-readable size
#[must_use]
pub fn format_size(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
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
    guitk::duration::coarse(seconds)
}

// ─── Main ────────────────────────────────────────────────────────────

impl App for TorrentApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        // How many transfers are running, because that is what a torrent
        // client is left open for. The harness re-reads this as it runs.
        let active = self
            .torrents
            .iter()
            .filter(|t| t.state == TorrentState::Downloading)
            .count();
        if active == 0 {
            "Torrents".to_string()
        } else {
            format!("{active} downloading - Torrents")
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        }
    }

    /// A clock only while something is downloading.
    ///
    /// A window showing a list of paused or finished torrents has nothing to
    /// redraw, and waking the machine to find that out is what
    /// `known-issues.md` lesson 47 is about.
    ///
    /// And only while one has a peer to take a piece from. It asked for a
    /// tick every 150 ms while anything was "downloading", and with no
    /// network nothing ever has a peer -- so a transfer set going woke the
    /// machine seven times a second, for good, to find nothing to do.
    ///
    /// Now: while a download runs, since that is when there is news -- every
    /// `PIECE_STEP` while one has peers, once a second while all are only
    /// looking for them.
    fn tick_interval(&self) -> Option<Duration> {
        if self.sessions.is_empty() {
            return None;
        }
        let busy = self
            .torrents
            .iter()
            .any(|t| self.sessions.contains_key(&t.id) && !t.peers.is_empty());
        Some(if busy {
            PIECE_STEP
        } else {
            Duration::from_secs(1)
        })
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.win_width = width;
        self.win_height = height;
        self.keep_selection_visible();
        let frame = self.frame(width, height);
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }
}

impl TorrentApp {
    /// Two torrents to populate a list, for tests.
    ///
    /// `#[cfg(test)]` since 2026-09-15. The reasoning in the old doc comment
    /// was "a client that opens on an empty list looks broken rather than
    /// idle", which is true and is the wrong fix: the answer to a list that
    /// looks broken is to *say why it is empty*, not to fill it. Filling it
    /// meant the shipped binary opened on a 4.2 GB "Ubuntu 24.04 LTS Desktop"
    /// and a 350 MB "LibreOffice 7.6.4", announced against real tracker URLs,
    /// that then began to download.
    #[cfg(test)]
    pub fn seed_sample_torrents(&mut self) {
        // Add sample torrents for testing
        let sample_torrent = create_sample_torrent(
            "Ubuntu 24.04 LTS Desktop",
            4_200_000_000,
            262_144,
            "https://torrent.ubuntu.com/announce",
        );
        self.add_torrent(sample_torrent, None);

        let sample2 = create_sample_torrent(
            "LibreOffice 7.6.4",
            350_000_000,
            524_288,
            "udp://tracker.opentrackr.org:1337/announce",
        );
        let id2 = self.add_torrent(sample2, None);
        if let Some(t) = self.torrents.iter_mut().find(|t| t.id == id2) {
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
        self.add_magnet(magnet, None);
        // In a real system, these commands would be sent to the compositor
    }
}

fn main() -> ExitCode {
    // Opens empty. It used to call `seed_sample_torrents`, so every launch
    // began with two torrents nobody had asked for, which then made progress.
    let mut app = TorrentApp::new();
    app::launch("torrent", &mut app)
}

#[cfg(test)]
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
        info_hash: sha1::sha1(name.as_bytes()),
        name: name.to_string(),
        name_bytes: name.as_bytes().to_vec(),
        multi_file: false,
        piece_length: piece_len,
        pieces,
        files: vec![TorrentFile::named(&format!("{name}.iso"), size)],
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
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// A `.torrent` built by this crate's own encoder, so the test exercises
    /// the parser rather than my transcription of bencode.
    fn a_real_torrent_file(name: &str) -> Vec<u8> {
        let mut info = std::collections::BTreeMap::new();
        info.insert(
            "name".to_string(),
            BencodeValue::Bytes(name.as_bytes().to_vec()),
        );
        info.insert("piece length".to_string(), BencodeValue::Integer(16_384));
        // One piece: twenty bytes of SHA-1. 0xFF specifically, because it is
        // never valid UTF-8 -- the first fixture used 0x07, which IS valid
        // (it is a control character), and the bytes-not-text test below
        // refused to run against it. A real SHA-1 is arbitrary bytes and will
        // usually contain some, but "usually" is not what a test asserts on.
        info.insert("pieces".to_string(), BencodeValue::Bytes(vec![0xFFu8; 20]));
        info.insert("length".to_string(), BencodeValue::Integer(1_000));

        let mut root = std::collections::BTreeMap::new();
        root.insert(
            "announce".to_string(),
            BencodeValue::Bytes(b"http://tracker.invalid/announce".to_vec()),
        );
        root.insert("info".to_string(), BencodeValue::Dict(info));
        bencode_encode(&BencodeValue::Dict(root))
    }

    fn torrent_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("slateos-torrent-door-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// **A torrent file is the one thing here that needs no network.**
    ///
    /// `TorrentMetainfo::from_bencode` was written, tested and unreachable:
    /// the program had no way to obtain a byte. Opening one tells the user the
    /// name, the size and which trackers it names, and contacts nothing.
    /// An open picker takes the keyboard, and the window behind it does not.
    ///
    /// The open test asserts that the KEY HANDLER opened the dialog, which
    /// holds whether or not the picker is ever handed another event; the
    /// writer test calls the writer with a path directly and never touches the
    /// dialog. This is the half routing actually decides -- with a dialog up,
    /// a keystroke belongs to the dialog.
    ///
    /// Found by `scripts/find-unpinned-picker-routing.py`, which cuts the
    /// routing and reports whose tests notice. Sixteen of twenty did not.
    /// The settings panel says its settings are not applied.
    ///
    /// `TRANSFER_NOTE_LINES` tells a reader the transfers do not happen.
    /// This panel separately reports "Encryption: Prefer" and "DHT: Enabled",
    /// which a reader can believe describes how the client behaves on a
    /// network -- and there is no network stack, so those values have never
    /// been read by anything but the line that prints them.
    #[test]
    fn the_settings_panel_says_nothing_reads_these() {
        let mut app = TorrentApp::new();
        app.active_tab = Tab::Settings;
        let texts: Vec<String> = app
            .render_commands(1280.0, 800.0)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        assert!(
            texts.iter().any(|t| t.contains("Encryption:")),
            "control: the panel must be drawing a setting for this to be \
about anything -- it drew {} text command(s)",
            texts.len()
        );
        // Against the words a reader sees, not against the constant: a test
        // comparing with `SETTINGS_NOT_APPLIED` passes with the constant
        // rewritten to "Settings", which would be the same defect wearing
        // this test as cover.
        assert!(
            texts
                .iter()
                .any(|t| t.contains("nothing else here is read yet")),
            "the panel drew settings and did not say most are not applied"
        );
    }

    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_list() {
        let mut app = TorrentApp::new();
        app.seed_sample_torrents();
        // Seeding does not select: the control assertion below caught the
        // first version of this test, which would otherwise have compared
        // `None` against `None` and passed with the dialog doing nothing.
        app.selected_torrent = app.torrents.first().map(|t| t.id);
        let before = app.selected_torrent;
        assert!(before.is_some(), "control: something must be selected");
        assert!(
            app.torrents.len() > 1,
            "control: one row cannot move, so the fixture needs two"
        );

        assert_eq!(
            app.handle_event(&key_ev(Key::O, true)),
            EventResult::Consumed
        );
        assert!(app.picker.is_open(), "control: the picker must be up");

        app.handle_event(&press(Key::Down));
        assert_eq!(
            app.selected_torrent, before,
            "Down at the open dialog moved the selection behind it"
        );
    }

    #[test]
    fn opening_a_torrent_file_lists_what_is_in_it() {
        let path = torrent_dir().join("thing.torrent");
        std::fs::write(&path, a_real_torrent_file("A Thing")).expect("write torrent");

        let mut app = TorrentApp::new();
        let before = app.torrents.len();
        let said = app.open_torrent_file(&path);

        assert!(said.starts_with("Opened A Thing"), "said: {said}");
        // It is fetched as well as listed: opening a torrent starts it.
        assert!(said.contains("fetching"), "said: {said}");
        assert_eq!(app.torrents.len(), before + 1, "no torrent was added");

        let added = app.torrents.last().expect("the torrent");
        assert_eq!(added.name, "A Thing");
        assert!(
            added
                .trackers
                .iter()
                .all(|t| t.status == TrackerStatus::NotContacted),
            "a tracker was marked as something other than NotContacted"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// A file that is not a torrent keeps the parser's own reason.
    ///
    /// "missing 'info' dict" says which part is absent; replacing it with
    /// "not a torrent" would throw that away, and the user would have no idea
    /// whether they picked the wrong file or have a damaged one.
    #[test]
    fn a_file_that_is_not_a_torrent_keeps_the_parsers_reason() {
        let path = torrent_dir().join("notatorrent.bin");
        // Valid bencode, wrong shape: a dict with no `info`.
        let mut root = std::collections::BTreeMap::new();
        root.insert("announce".to_string(), BencodeValue::Bytes(b"x".to_vec()));
        std::fs::write(&path, bencode_encode(&BencodeValue::Dict(root))).expect("write");

        let mut app = TorrentApp::new();
        let before = app.torrents.len();
        let said = app.open_torrent_file(&path);

        assert!(said.contains("missing 'info' dict"), "said: {said}");
        assert_eq!(app.torrents.len(), before, "a failed read added a torrent");

        let _ = std::fs::remove_file(&path);
    }

    /// Bytes, not text. A `.torrent` is bencode and its `pieces` field is raw
    /// SHA-1, which is not UTF-8 -- reading it as text would refuse a
    /// perfectly valid file and blame its contents.
    #[test]
    fn a_torrent_is_read_as_bytes_not_text() {
        let path = torrent_dir().join("binary.torrent");
        let bytes = a_real_torrent_file("Binary");
        assert!(
            String::from_utf8(bytes.clone()).is_err(),
            "the fixture is valid UTF-8, so it cannot show that bytes are needed"
        );
        std::fs::write(&path, bytes).expect("write torrent");

        let mut app = TorrentApp::new();
        let said = app.open_torrent_file(&path);
        assert!(said.starts_with("Opened Binary"), "said: {said}");

        let _ = std::fs::remove_file(&path);
    }

    /// A file that is not there is a read failure, not a verdict on contents.
    #[test]
    fn a_missing_torrent_is_reported_as_a_read_failure() {
        let path = std::env::temp_dir().join("slateos-torrent-absent.torrent");
        let _ = std::fs::remove_file(&path);

        let mut app = TorrentApp::new();
        let said = app.open_torrent_file(&path);
        assert!(said.starts_with("Could not read"), "said: {said}");
        assert!(
            !said.contains("missing"),
            "the file was never parsed, so do not blame its contents: {said}"
        );
    }

    /// The picker is not merely open: it is DRAWN.
    #[test]
    fn the_picker_is_drawn_when_it_is_open() {
        let mut app = TorrentApp::new();
        let before = app.render_commands(WINDOW_WIDTH, WINDOW_HEIGHT).len();
        app.picker.open_to_read();
        assert!(app.picker.is_open(), "no picker came up");
        let own = app
            .picker
            .render(&app.palette, WINDOW_WIDTH, WINDOW_HEIGHT)
            .len();
        assert!(
            own > 0,
            "the picker itself draws nothing, so this proves nothing"
        );
        let after = app.render_commands(WINDOW_WIDTH, WINDOW_HEIGHT).len();
        assert!(
            after >= before + own,
            "the frame does not contain the picker's own {own} command(s)"
        );
    }

    /// A fresh client holds nothing and finishes nothing.
    ///
    /// `main` called `seed_sample_torrents`, so every launch opened on a
    /// 4.2 GB "Ubuntu 24.04 LTS Desktop" and a 350 MB "LibreOffice 7.6.4",
    /// announced against real tracker URLs. `handle_tick` then invented three
    /// peers holding every piece between them, so each torrent ran to 100%
    /// and flipped to Seeding with a completion time -- a finished download of
    /// a file that exists nowhere, followed by a claim to be uploading it.
    ///
    /// A finished download is acted on: it is the point at which somebody
    /// stops looking for the thing, and may delete the source they got it
    /// from.
    #[test]
    fn a_fresh_client_holds_nothing_and_a_tick_finishes_nothing() {
        let mut app = TorrentApp::new();
        assert!(
            app.torrents.is_empty(),
            "the client opened on torrents nobody asked for"
        );

        // Even given a torrent, no tick can advance it: there is no swarm to
        // ask, and nothing here could ask one.
        let meta = create_sample_torrent("Small", 1000, 256, "https://example/announce");
        let id = app.add_torrent(meta, None);
        if let Some(t) = app.torrents.iter_mut().find(|t| t.id == id) {
            t.state = TorrentState::Downloading;
            t.downloaded = 0;
        }
        for _ in 0..50 {
            app.handle_event(&tick());
        }
        let t = app.torrents.iter().find(|t| t.id == id).expect("there");
        assert!(t.peers.is_empty(), "peers appeared from nowhere");
        assert_eq!(t.downloaded, 0, "bytes arrived from nowhere");
        assert_eq!(t.pieces.completed_count(), 0, "pieces arrived from nowhere");
        assert_ne!(t.state, TorrentState::Seeding, "it claimed to be uploading");
        assert!(t.completed_time.is_none(), "it recorded a completion");
    }

    /// And the window says what this client does not do.
    #[test]
    fn the_window_says_what_it_does_not_do() {
        let app = TorrentApp::new();
        let cmds = app.render_commands(WINDOW_WIDTH, WINDOW_HEIGHT);
        let texts: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for line in TRANSFER_NOTE_LINES {
            assert!(texts.contains(&line), "the window never said {line:?}");
        }
        assert!(
            TRANSFER_NOTE_LINES
                .iter()
                .any(|l| l.contains("nothing is uploaded")),
            "nothing says the client does not share what it fetches",
        );
    }

    // ------------------------------------------------------------------
    // Wiring
    //
    // This program had no input handling, and twenty functions had no caller
    // outside the tests -- the transfer controls and the piece picker, which
    // is the whole of a BitTorrent client's download loop.
    // ------------------------------------------------------------------

    /// **The sort column and its direction can be changed.**
    ///
    /// `sort_column` had no writer anywhere: declared, constructed as `Added`,
    /// read once in a comparator with eleven arms, ten of them unreachable.
    /// `sort_ascending` was fixed at descending beside it.
    ///
    /// Asserts the *order of the list*, not the value of the field: a key that
    /// sets an enum the comparator ignores would pass the weaker test, which
    /// is the defect `apps/regextester`'s `multiline` had.
    #[test]
    fn the_sort_column_and_direction_can_be_changed() {
        let mut app = TorrentApp::new();
        for (name, size) in [("beta", 3000u64), ("alpha", 1000), ("gamma", 2000)] {
            let meta = create_sample_torrent(name, size, 256, "http://t.co/a");
            app.add_torrent(meta, None);
        }

        let names = |app: &TorrentApp| -> Vec<String> {
            app.filtered_torrents()
                .iter()
                .map(|t| t.name.clone())
                .collect()
        };
        let start = names(&app);

        // Step to the name column and the list has to re-order.
        let mut guard = 0;
        while app.sort_column != SortColumn::Name && guard < SortColumn::ALL.len() {
            assert_eq!(app.handle_event(&press(Key::C)), EventResult::Consumed);
            guard += 1;
        }
        assert_eq!(
            app.sort_column,
            SortColumn::Name,
            "C did not reach the name column"
        );
        let by_name = names(&app);
        assert_ne!(by_name, start, "sorting by name changed nothing");

        assert_eq!(app.handle_event(&press(Key::R)), EventResult::Consumed);
        let reversed = names(&app);
        assert_ne!(reversed, by_name, "R did not reverse the order");
        assert_eq!(
            reversed.iter().rev().cloned().collect::<Vec<_>>(),
            by_name,
            "R gave an order that is not the reverse of the one before it"
        );
    }

    /// **Every filter in the sidebar can be selected.**
    ///
    /// The sidebar drew seven entries and highlighted whichever was current;
    /// `filter` had no writer, so the highlight never moved and six of the
    /// seven rows were decoration.
    #[test]
    fn every_filter_in_the_sidebar_can_be_selected() {
        let mut app = TorrentApp::new();
        let wanted = [
            (Key::Num1, TorrentFilter::All),
            (Key::Num2, TorrentFilter::Downloading),
            (Key::Num3, TorrentFilter::Seeding),
            (Key::Num4, TorrentFilter::Completed),
            (Key::Num5, TorrentFilter::Paused),
            (Key::Num6, TorrentFilter::Active),
            (Key::Num7, TorrentFilter::Error),
        ];
        for (key, filter) in wanted {
            app.handle_event(&press(key));
            assert_eq!(app.filter, filter, "{key:?} did not select {filter:?}");
        }
    }

    /// **The status bar says what the list is sorted by, and how to change it.**
    ///
    /// Without this the two keys are as unreachable as the fields were: the
    /// sidebar shows its own selection, but nothing else on screen mentions
    /// **Every key the card advertises is answered by this window.**
    ///
    /// Two filter states, and the reason is the one that took longest to
    /// see. `Consumed` here asks whether the key *did* something, not whether
    /// the window owns it: `Ignored` in a top-level app means "nothing
    /// changed, do not redraw" -- `handle_event` maps it to `Response::Idle`
    /// and there is no parent to propagate to. So `1` on a list already
    /// showing All is answered and reports `Ignored`, correctly. Running the
    /// guard from two different filters gives every digit a state in which it
    /// changes something.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                // And one with a transfer chosen, for the keys that act on
                // the chosen one.
                let answered = [TorrentFilter::All, TorrentFilter::Error]
                    .into_iter()
                    .map(|start| {
                        let mut app = TorrentApp::new();
                        app.filter = start;
                        app
                    })
                    .chain(std::iter::once({
                        let mut app = seeded();
                        app.selected_torrent = app.torrents.first().map(|t| t.id);
                        app
                    }))
                    .any(|mut app| {
                        app.handle_event(&Event::Key(stroke.clone())) == EventResult::Consumed
                    });
                assert!(
                    answered,
                    "the card advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// **The card reaches the window, and nothing acts behind it.**
    ///
    /// The control is the last third: `C` behind the card must not move the
    /// sort column, but asserting only that would pass just as well on a
    /// window that had lost `C` altogether, so the same key is then pressed
    /// with the card down and required to work.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let drawn = |app: &mut TorrentApp| -> String {
            app.render(1200.0, 800.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" | ")
        };

        let mut app = TorrentApp::new();
        assert!(
            !drawn(&mut app).contains("F1 or ? closes this"),
            "the card is up before anybody asked for it"
        );

        app.handle_event(&press(Key::F1));
        let shown = drawn(&mut app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        let column = app.sort_column;
        app.handle_event(&press(Key::C));
        assert_eq!(
            app.sort_column, column,
            "C changed the sort column through the shortcut card"
        );

        app.handle_event(&press(Key::Escape));
        app.handle_event(&press(Key::C));
        assert_ne!(
            app.sort_column, column,
            "control: C does nothing even with the card down"
        );
    }

    /// the sort at all.
    #[test]
    fn the_status_bar_names_the_sort_and_its_keys() {
        let mut app = TorrentApp::new();
        let drawn = |app: &mut TorrentApp| -> String {
            app.render(1200.0, 800.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" | ")
        };

        let before = drawn(&mut app);
        assert!(
            before.contains("sorted by date added descending (C, R)"),
            "the status bar does not say what the list is sorted by: {before}"
        );

        app.handle_event(&press(Key::R));
        assert!(
            drawn(&mut app).contains("sorted by date added ascending (C, R)"),
            "the status bar did not follow the direction"
        );
    }

    fn key_ev(key: Key, ctrl: bool) -> Event {
        let mut modifiers = guitk::event::Modifiers::NONE;
        modifiers.ctrl = ctrl;
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    fn press(k: Key) -> Event {
        key_ev(k, false)
    }

    fn tick() -> Event {
        Event::Tick { elapsed_ms: 150 }
    }

    fn seeded() -> TorrentApp {
        let mut app = TorrentApp::new();
        app.seed_sample_torrents();
        app
    }

    /// A test cannot call `main`, and a client that opens on an empty list
    /// looks broken rather than idle.
    #[test]
    fn the_window_opens_with_torrents_in_it() {
        let app = seeded();
        assert!(app.torrents.len() > 1);
    }

    // -- downloads --

    /// A `.torrent` file for `stream`, laid out as `testnet::content` lays
    /// it out, announcing to `announce`.
    fn torrent_file(stream: &[u8], announce: &str) -> Vec<u8> {
        use session::testnet::PIECE;
        let bytes = |s: &str| BencodeValue::Bytes(s.as_bytes().to_vec());
        let file = |path: &[&str], len: usize| {
            let mut d = BTreeMap::new();
            d.insert(
                String::from("length"),
                BencodeValue::Integer(i64::try_from(len).unwrap()),
            );
            d.insert(
                String::from("path"),
                BencodeValue::List(path.iter().map(|p| bytes(p)).collect()),
            );
            BencodeValue::Dict(d)
        };
        let pieces: Vec<u8> = stream.chunks(PIECE as usize).flat_map(sha1::sha1).collect();
        let mut info = BTreeMap::new();
        info.insert(
            String::from("files"),
            BencodeValue::List(vec![
                file(&["one.bin"], 40_000),
                file(&["sub", "two.bin"], 50_000),
                file(&["three.bin"], stream.len() - 90_000),
            ]),
        );
        info.insert(String::from("name"), bytes("Set"));
        info.insert(
            String::from("piece length"),
            BencodeValue::Integer(PIECE as i64),
        );
        info.insert(String::from("pieces"), BencodeValue::Bytes(pieces));
        let mut top = BTreeMap::new();
        top.insert(String::from("announce"), bytes(announce));
        top.insert(String::from("info"), BencodeValue::Dict(info));
        bencode_encode(&BencodeValue::Dict(top))
    }

    /// **Opening a `.torrent` downloads it.** The window starts a session,
    /// hears of every piece on its ticks, and ends Complete -- the files on
    /// disk byte for byte, the tracker marked working, the session gone and
    /// the clock stopped.
    #[test]
    fn opening_a_torrent_downloads_it() {
        use session::testnet::{Scratch, Serve, content, seeder, tracker_for};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let (_, stream) = content();
        // The info hash does not depend on the tracker's address, so the
        // peer can be started before the tracker exists.
        let draft =
            TorrentMetainfo::from_bencode(&torrent_file(&stream, "http://127.0.0.1:1/a")).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let (peer, peer_thread) = seeder(
            &draft,
            Arc::clone(&stream),
            Serve::Honest,
            Arc::clone(&stop),
        );
        let (url, tracker_thread) =
            tracker_for(vec![peer], Arc::clone(&stop), Arc::new(AtomicUsize::new(0)));
        let dir = Scratch::new("window");
        let file = dir.0.join("set.torrent");
        std::fs::write(&file, torrent_file(&stream, &url)).unwrap();

        let mut app = TorrentApp::new();
        app.settings.default_save_path = dir.0.join("downloads");
        let said = app.open_torrent_file(&file);
        assert!(said.contains("fetching"), "{said}");
        assert_eq!(app.torrents[0].state, TorrentState::CheckingFiles);
        assert!(app.tick_interval().is_some(), "a download needs a clock");
        let deadline = std::time::Instant::now() + Duration::from_mins(1);
        while app.torrents[0].state != TorrentState::Complete
            && std::time::Instant::now() < deadline
        {
            app.handle_event(&tick());
            std::thread::sleep(Duration::from_millis(20));
        }
        stop.store(true, Ordering::Relaxed);
        let t = &app.torrents[0];
        assert_eq!(t.state, TorrentState::Complete, "{:?}", t.error_message);
        assert_eq!(t.downloaded, t.total_size);
        assert!(t.pieces.is_complete());
        assert_eq!(t.trackers[0].status, TrackerStatus::Working);
        assert!(t.completed_time.is_some());
        assert!(
            app.sessions.is_empty(),
            "a finished download kept its session"
        );
        assert_eq!(app.tick_interval(), None, "and its clock");
        let saved = dir.0.join("downloads").join("Set");
        let got = [
            std::fs::read(saved.join("one.bin")).unwrap(),
            std::fs::read(saved.join("sub").join("two.bin")).unwrap(),
            std::fs::read(saved.join("three.bin")).unwrap(),
        ]
        .concat();
        assert!(got == *stream, "the files do not hold the torrent's bytes");
        peer_thread.join().unwrap();
        tracker_thread.join().unwrap();
    }

    /// Space starts a download, stops it -- its session goes, what it
    /// fetched stays -- and starts it again.
    #[test]
    fn space_starts_pauses_and_resumes_a_download() {
        let mut app = TorrentApp::new();
        let meta = create_sample_torrent("Paused", 1000, 256, "http://127.0.0.1:1/announce");
        let id = app.add_torrent(meta, None);
        app.selected_torrent = Some(id);
        let state = |app: &TorrentApp| app.torrents.iter().find(|t| t.id == id).unwrap().state;
        assert_eq!(state(&app), TorrentState::Queued);
        app.handle_event(&press(Key::Space));
        assert_eq!(state(&app), TorrentState::CheckingFiles);
        assert!(app.sessions.contains_key(&id), "nothing was started");
        app.handle_event(&press(Key::Space));
        assert_eq!(state(&app), TorrentState::Paused);
        assert!(
            !app.sessions.contains_key(&id),
            "a paused download kept running"
        );
        app.handle_event(&press(Key::Space));
        assert!(app.sessions.contains_key(&id), "resuming started nothing");
    }

    /// A download a tracker has no peers for says it is looking, and the
    /// clock slows to once a second: there is nothing to hear of.
    #[test]
    fn a_download_with_no_peers_says_it_is_looking() {
        let (mut app, id) = chosen();
        app.torrents.iter_mut().find(|t| t.id == id).unwrap().state = TorrentState::Paused;
        probe::click(&mut app, Target::Resume);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while app.torrents.iter().find(|t| t.id == id).unwrap().state == TorrentState::CheckingFiles
            && std::time::Instant::now() < deadline
        {
            app.handle_event(&tick());
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(texts(&app).iter().any(|t| t == "Looking for peers"));
        assert_eq!(app.tick_interval(), Some(Duration::from_secs(1)));
    }

    // -- the list --

    #[test]
    fn the_arrows_walk_the_list_and_stop_at_the_ends() {
        let mut app = seeded();
        let ids: Vec<u32> = app.torrents.iter().map(|t| t.id).collect();
        assert!(ids.len() > 1);

        app.handle_event(&press(Key::Down));
        assert_eq!(app.selected_torrent, ids.first().copied());
        app.handle_event(&press(Key::Up));
        assert_eq!(
            app.selected_torrent,
            ids.first().copied(),
            "stopping, not wrapping"
        );

        for _ in 0..ids.len() + 3 {
            app.handle_event(&press(Key::Down));
        }
        assert_eq!(app.selected_torrent, ids.last().copied());
    }

    /// Delete removes the entry and keeps the files: deleting someone's
    /// download because they pressed Delete on a list row is the one
    /// irreversible thing this program can do.
    #[test]
    fn delete_removes_the_torrent_from_the_list() {
        let mut app = seeded();
        let before = app.torrents.len();
        app.handle_event(&press(Key::Down));
        let id = app.selected_torrent.expect("a selection");

        app.handle_event(&press(Key::Delete));
        assert_eq!(app.torrents.len(), before - 1);
        assert!(
            !app.torrents.iter().any(|t| t.id == id),
            "the torrent is still listed"
        );
        assert_ne!(app.selected_torrent, Some(id), "and the selection moved on");
        assert!(
            app.status_message.starts_with("Removed:"),
            "Delete should take the entry off the list and leave the files alone -- deleting someone's download because they pressed Delete on a row is the one irreversible thing this program does. Status says {:?}",
            app.status_message
        );
    }

    /// `pause_all` and `resume_all` had no caller at all -- not even a test.
    #[test]
    fn ctrl_p_and_ctrl_r_stop_and_start_everything() {
        let mut app = seeded();
        for t in &mut app.torrents {
            t.state = TorrentState::Downloading;
        }

        app.handle_event(&key_ev(Key::P, true));
        assert!(
            !app.torrents
                .iter()
                .any(|t| t.state == TorrentState::Downloading),
            "something is still downloading"
        );
        assert_eq!(app.tick_interval(), None);

        app.handle_event(&key_ev(Key::R, true));
        assert!(
            app.torrents.iter().any(|t| matches!(
                t.state,
                TorrentState::Downloading | TorrentState::CheckingFiles
            )),
            "nothing resumed"
        );
    }

    #[test]
    fn tab_moves_through_the_six_tabs() {
        let mut app = seeded();
        let mut seen = vec![app.active_tab];
        for _ in 0..6 {
            app.handle_event(&press(Key::Tab));
            seen.push(app.active_tab);
        }
        assert_eq!(seen.first(), seen.last(), "six presses should come round");
        for tab in [
            Tab::Transfers,
            Tab::Details,
            Tab::Peers,
            Tab::Files,
            Tab::Trackers,
            Tab::Settings,
        ] {
            assert!(seen.contains(&tab), "{tab:?} was skipped: {seen:?}");
        }
    }

    #[test]
    fn the_title_counts_the_running_transfers() {
        let mut app = seeded();
        for t in &mut app.torrents {
            t.state = TorrentState::Paused;
        }
        assert_eq!(app.title(), "Torrents");

        if let Some(t) = app.torrents.first_mut() {
            t.state = TorrentState::Downloading;
        }
        assert!(
            app.title().starts_with("1 downloading"),
            "got {:?}",
            app.title()
        );
    }

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

    #[test]
    fn a_handshake_is_the_sixty_eight_bytes_bep_3_specifies() {
        // The round-trip test above cannot stand in for this one. It catches
        // an encoder bug only on the assumption that the decoder is right, and
        // nothing in this file pins the decoder to BEP 3 either -- so the pair
        // is checked against itself and never against the specification.
        //
        // That matters because the peer on the other end of this handshake is
        // a *foreign* implementation -- libtorrent, Transmission, qBittorrent.
        // A matched pair of bugs, `encode` and `decode` both putting the peer
        // id before the info hash, or both treating pstrlen as a `u32`,
        // round-trips perfectly here and is hung up on by every client in the
        // swarm. Asserting the bytes against the specification is the only
        // check the two of them cannot pass by agreeing with each other.
        //
        // BEP 3 gives the layout as, in order:
        //   1 byte   pstrlen = 19
        //   19 bytes pstr    = "BitTorrent protocol"
        //   8 bytes  reserved
        //   20 bytes info_hash
        //   20 bytes peer_id
        //
        // The two 20-byte fields are filled with different constants
        // precisely so that swapping them is visible.
        let bytes = Handshake::new([0xAA; 20], [0xBB; 20]).encode();

        assert_eq!(bytes.len(), 68, "1 + 19 + 8 + 20 + 20");
        assert_eq!(
            bytes.first().copied(),
            Some(19),
            "pstrlen is a single byte, not a length prefix like every other \
             message on this connection"
        );
        assert_eq!(
            bytes.get(1..20),
            Some(b"BitTorrent protocol".as_slice()),
            "the protocol name is fixed by BEP 3 and is not ours to change"
        );
        assert_eq!(
            bytes.get(20..28),
            Some([0, 0, 0, 0, 0, 0x10, 0, 0].as_slice()),
            "BEP 10 puts the extension-protocol bit at 0x10 of reserved byte 5"
        );
        assert_eq!(bytes.get(28..48), Some([0xAA; 20].as_slice()), "info_hash");
        assert_eq!(bytes.get(48..68), Some([0xBB; 20].as_slice()), "peer_id");
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
        let tracker = PieceTracker::new(4);
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
        assert_eq!(app.torrents[0].state, TorrentState::CheckingFiles);
        assert!(app.sessions.contains_key(&id));
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
        assert_eq!(format_size(1_073_741_824), "1.0 GiB");
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
        let cmds = app.render_commands(1280.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_settings_defaults() {
        let settings = ClientSettings::default();
        assert_eq!(settings.listen_port, 6881);
        assert!(settings.dht_enabled);
        assert_eq!(settings.encryption_mode, EncryptionMode::Prefer);
    }

    // ─── Detail-table column fitting ─────────────────────────────────
    //
    // Every cell in the Peers / Files / Trackers tables holds a string that
    // arrived over the network. These tests hold the tables to the rule that
    // no cell may draw past the right edge of the column it lives in, and
    // that when one is shortened the cut is *marked* and the identifying end
    // of a path or URL is the end that survives.

    /// The `(left, right)` x-range of each column, asked of the same `Table`
    /// the renderer used, so the test cannot disagree with it about where a
    /// column is.
    fn column_edges(columns: &[Column], x: f32) -> Vec<(f32, f32)> {
        Table::new(columns, x).spans()
    }

    /// A torrent whose every wire-supplied string is far too long for its
    /// column, alongside a short one that must be left alone.
    fn app_with_a_shouting_torrent() -> TorrentApp {
        let mut app = TorrentApp::new();
        let mut meta =
            create_sample_torrent("Shouty", 4096, 256, "http://tracker.example.com/announce");
        meta.files = vec![
            TorrentFile::named(
                "Some Show/Season 1/Episode 01 - A Very Long Episode Title Indeed \
                       That Will Certainly Not Fit In The Column.mkv",
                4096,
            ),
            TorrentFile::named("readme.txt", 12),
        ];
        let id = app.add_torrent(meta, None);
        app.selected_torrent = Some(id);
        if let Some(t) = app.torrents.iter_mut().find(|t| t.id == id) {
            let mut loud = PeerInfo::new("192.168.100.200", 51413);
            loud.client_name =
                "qBittorrent 4.6.2 (a client that has chosen to name itself at truly \
                 extravagant length)"
                    .to_string();
            loud.download_rate = 1_048_576;
            loud.upload_rate = 4096;
            loud.downloaded = 1_073_741_824;
            loud.supports_extensions = true;
            t.peers.push(loud);

            let mut quiet = PeerInfo::new("10.0.0.1", 6881);
            quiet.client_name = "Deluge".to_string();
            t.peers.push(quiet);

            t.trackers.push(TrackerEntry {
                url: "http://tracker.a-very-long-hostname-indeed.example.invalid:6969\
                      /announce/with/a/long/path?key=abcdef"
                    .to_string(),
                tier: 1,
                status: TrackerStatus::Working,
                seeders: 10,
                leechers: 3,
                last_announce: None,
                next_announce: None,
                announce_count: 1,
                error_message: None,
            });
        }
        app
    }

    /// Assert that every bounded text command sitting at a column's x draws
    /// within that column, and that we actually inspected `expected` of them
    /// — without the count the assertion passes vacuously if the panel
    /// stopped drawing rows altogether.
    fn assert_cells_fit(
        cmds: &[RenderCommand],
        columns: &[Column],
        x: f32,
        expected: usize,
        what: &str,
    ) {
        let edges = column_edges(columns, x);
        let mut checked = 0usize;
        for cmd in cmds {
            let RenderCommand::Text {
                x: tx,
                text,
                font_size,
                font_weight,
                max_width: Some(_),
                overflow: TextOverflow::Ellipsis,
                ..
            } = cmd
            else {
                continue;
            };
            let Some(&(_, right)) = edges.iter().find(|(left, _)| (left - tx).abs() < 0.01) else {
                continue;
            };
            let drawn = tx + text::measure(text, *font_size, *font_weight);
            assert!(
                drawn <= right + 0.5,
                "{what}: cell {text:?} starting at {tx} runs to {drawn}, \
                 past its column's right edge {right}",
            );
            checked += 1;
        }
        assert!(
            checked >= expected,
            "{what}: only {checked} cells checked, expected at least {expected}",
        );
    }

    /// The texts drawn in a table's first column, header excluded.
    fn first_column_cells(cmds: &[RenderCommand], columns: &[Column], x: f32) -> Vec<String> {
        let edges = column_edges(columns, x);
        let left = edges.first().map_or(f32::NAN, |&(l, _)| l);
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x: tx,
                    text,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_),
                    overflow: TextOverflow::Ellipsis,
                    ..
                } if (tx - left).abs() < 0.01 => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn no_peer_row_cell_escapes_its_column() {
        let app = app_with_a_shouting_torrent();
        let mut frame = Frame::new(900.0, 400.0);
        // Render the panel directly rather than the whole app: a full render
        // puts toolbar and sidebar text at x values that happen to fall inside
        // a column's range, and the assertion would then fail on chrome that
        // was never part of this table.
        app.render_peers(&mut frame, Rect::new(0.0, 0.0, 900.0, 400.0));
        let cmds = frame.into_tree().commands;
        // 6 header labels + 2 peers x 6 cells.
        assert_cells_fit(&cmds, PEER_COLUMNS, 0.0, 18, "peers");
    }

    #[test]
    fn no_file_row_cell_escapes_its_column() {
        let app = app_with_a_shouting_torrent();
        let mut frame = Frame::new(900.0, 400.0);
        app.render_files(&mut frame, Rect::new(0.0, 0.0, 900.0, 400.0));
        let cmds = frame.into_tree().commands;
        // 3 header labels + 2 files x 3 cells.
        assert_cells_fit(&cmds, FILE_COLUMNS, 0.0, 9, "files");
    }

    #[test]
    fn no_tracker_row_cell_escapes_its_column() {
        let app = app_with_a_shouting_torrent();
        let mut frame = Frame::new(900.0, 400.0);
        app.render_trackers(&mut frame, Rect::new(0.0, 0.0, 900.0, 400.0));
        let cmds = frame.into_tree().commands;
        // 5 header labels + at least the one over-long tracker's 5 cells.
        assert_cells_fit(&cmds, TRACKER_COLUMNS, 0.0, 10, "trackers");
    }

    #[test]
    fn an_overlong_file_path_keeps_its_filename() {
        let app = app_with_a_shouting_torrent();
        let mut frame = Frame::new(900.0, 400.0);
        app.render_files(&mut frame, Rect::new(0.0, 0.0, 900.0, 400.0));
        let cmds = frame.into_tree().commands;
        let names = first_column_cells(&cmds, FILE_COLUMNS, 0.0);
        let long = names
            .iter()
            .find(|n| n.ends_with(".mkv"))
            .expect("the long file's name cell should be drawn");
        assert!(
            long.starts_with('…'),
            "the cut should be marked at the front, got {long:?}"
        );
        assert!(
            long.ends_with("Not Fit In The Column.mkv"),
            "the filename is what identifies the file and must survive, got {long:?}"
        );
    }

    #[test]
    fn a_short_file_path_is_left_alone() {
        let app = app_with_a_shouting_torrent();
        let mut frame = Frame::new(900.0, 400.0);
        app.render_files(&mut frame, Rect::new(0.0, 0.0, 900.0, 400.0));
        let cmds = frame.into_tree().commands;
        let names = first_column_cells(&cmds, FILE_COLUMNS, 0.0);
        assert!(
            names.iter().any(|n| n == "readme.txt"),
            "a path that fits must be drawn verbatim, got {names:?}"
        );
    }

    #[test]
    fn an_overlong_tracker_url_keeps_its_announce_path() {
        let app = app_with_a_shouting_torrent();
        let mut frame = Frame::new(900.0, 400.0);
        app.render_trackers(&mut frame, Rect::new(0.0, 0.0, 900.0, 400.0));
        let cmds = frame.into_tree().commands;
        let urls = first_column_cells(&cmds, TRACKER_COLUMNS, 0.0);
        let long = urls
            .iter()
            .find(|u| u.ends_with("key=abcdef"))
            .expect("the long tracker's URL cell should be drawn");
        assert!(
            long.starts_with('…'),
            "the cut should be marked at the front, got {long:?}"
        );
    }

    #[test]
    fn an_overlong_peer_client_name_is_marked_as_cut() {
        let app = app_with_a_shouting_torrent();
        let mut frame = Frame::new(900.0, 400.0);
        app.render_peers(&mut frame, Rect::new(0.0, 0.0, 900.0, 400.0));
        let cmds = frame.into_tree().commands;
        let edges = column_edges(PEER_COLUMNS, 0.0);
        let (client_x, _) = edges[1];
        let clients: Vec<&String> = cmds
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x: tx,
                    text,
                    font_weight: FontWeightHint::Regular,
                    ..
                } if (tx - client_x).abs() < 0.01 => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            clients.iter().any(|c| c.ends_with('…')),
            "a peer-supplied client name too long for its column must be \
             visibly cut, not silently clipped: {clients:?}"
        );
        assert!(
            clients.iter().any(|c| *c == "Deluge"),
            "a client name that fits must be drawn verbatim: {clients:?}"
        );
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        // Named explicitly rather than relied on from the file's own imports.
        // The sixteen applications that declare their palette inside a
        // `mod mocha` block import `Color` *there*, so it is not in scope at
        // file level at all -- and once the module is emptied and removed, the
        // import goes with it.
        use guitk::Color;

        fn fills(app: &mut TorrentApp) -> Vec<Color> {
            // Fully qualified. Several applications also have an *inherent*
            // `render`, with different arguments, and an inherent method wins
            // resolution over a trait one -- so `app.render(w, h)` calls the
            // wrong function and fails to compile in a way that looks like the
            // trait is missing.
            oswindow::app::App::render(app, 1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = TorrentApp::new();

        oswindow::app::App::theme_changed(&mut app, &theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        oswindow::app::App::theme_changed(&mut app, &theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        oswindow::app::App::theme_changed(
            &mut app,
            &theme(
                appearance::ThemeMode::Dark,
                Some(appearance::HighContrastScheme::WhiteOnBlack),
            ),
        );
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // ── The pointer, the dialog, the search, and the file priorities ──

    use guitk::probe::{self, Probe};

    impl Probe for TorrentApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

        /// Drawn at the app's own size, which these tests leave at `SIZE`.
        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame(self.win_width, self.win_height)
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            _size: (f32, f32),
        ) -> EventResult {
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> EventResult {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<EventResult> {
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    fn texts(app: &TorrentApp) -> Vec<String> {
        app.frame(app.win_width, app.win_height)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn press_at(app: &mut TorrentApp, x: f32, y: f32) -> EventResult {
        app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }))
    }

    /// The seeded window with its first transfer chosen.
    fn chosen() -> (TorrentApp, u32) {
        let mut app = seeded();
        let id = app.filtered_torrents()[0].id;
        app.selected_torrent = Some(id);
        (app, id)
    }

    /// The notice was drawn at the top of the window, then painted over by
    /// the background and the header.
    #[test]
    fn the_notice_is_drawn_where_it_can_be_read() {
        let app = TorrentApp::new();
        let cmds = app
            .frame(app.win_width, app.win_height)
            .into_tree()
            .commands;
        for line in TRANSFER_NOTE_LINES {
            let at = cmds
                .iter()
                .position(|c| matches!(c, RenderCommand::Text { text, .. } if text == line))
                .unwrap_or_else(|| panic!("never drew {line:?}"));
            let RenderCommand::Text { x, y, .. } = &cmds[at] else {
                unreachable!()
            };
            assert!(
                *y >= HEADER_H + TAB_H && *x >= SIDEBAR_W,
                "{line:?} at ({x}, {y})"
            );
            for later in &cmds[at + 1..] {
                if let RenderCommand::FillRect {
                    x: fx,
                    y: fy,
                    width,
                    height,
                    color,
                    ..
                } = later
                {
                    let covers = *fx <= *x && *x < fx + width && *fy <= *y && *y < fy + height;
                    assert!(!(covers && color.a == 255), "{line:?} is painted over");
                }
            }
        }
    }

    #[test]
    fn the_toolbar_answers_the_pointer() {
        let (mut app, id) = chosen();
        let state = |app: &TorrentApp| app.torrents.iter().find(|t| t.id == id).unwrap().state;
        app.torrents.iter_mut().find(|t| t.id == id).unwrap().state = TorrentState::Downloading;
        assert!(
            probe::rect_of(&app, Target::Resume).is_none(),
            "Resume is offered on a running transfer"
        );
        probe::click(&mut app, Target::Pause);
        assert_eq!(state(&app), TorrentState::Paused);
        probe::click(&mut app, Target::Resume);
        assert_eq!(state(&app), TorrentState::CheckingFiles);
        probe::click(&mut app, Target::PauseAll);
        assert!(app.torrents.iter().all(|t| !matches!(
            t.state,
            TorrentState::Downloading | TorrentState::CheckingFiles
        )));
        assert!(app.sessions.is_empty(), "Pause all left a download running");
        probe::click(&mut app, Target::ResumeAll);
        assert_eq!(state(&app), TorrentState::CheckingFiles);
        let n = app.torrents.len();
        probe::click(&mut app, Target::Remove);
        assert_eq!(app.torrents.len(), n - 1);
        assert!(!app.torrents.iter().any(|t| t.id == id));
        probe::click(&mut app, Target::Open);
        assert!(app.picker.is_open(), "Open\u{2026} opened nothing");
    }

    #[test]
    fn the_sidebar_filters_and_labels_answer_the_pointer() {
        let mut app = seeded();
        probe::click(&mut app, Target::Filter(TorrentFilter::Paused));
        assert_eq!(app.filter, TorrentFilter::Paused);
        let software = app.labels.iter().position(|l| l == "Software").unwrap();
        probe::click(&mut app, Target::Filter(TorrentFilter::All));
        probe::click(&mut app, Target::Label(software));
        assert_eq!(app.selected_label.as_deref(), Some("Software"));
        assert!(
            app.filtered_torrents()
                .iter()
                .all(|t| t.label == "Software")
        );
        assert!(!app.filtered_torrents().is_empty());
        probe::click(&mut app, Target::Label(software));
        assert_eq!(
            app.selected_label, None,
            "a second press did not show them all"
        );
    }

    #[test]
    fn the_tabs_answer_the_pointer() {
        let mut app = TorrentApp::new();
        for tab in Tab::ALL.iter().rev() {
            probe::click(&mut app, Target::Tab(*tab));
            assert_eq!(app.active_tab, *tab);
        }
    }

    #[test]
    fn a_column_head_sorts_and_a_second_press_reverses() {
        let mut app = seeded();
        probe::click(&mut app, Target::SortBy(SortColumn::Name));
        assert_eq!(app.sort_column, SortColumn::Name);
        let ascending = app.sort_ascending;
        let first = app.filtered_torrents()[0].name.clone();
        probe::click(&mut app, Target::SortBy(SortColumn::Name));
        assert_eq!(app.sort_ascending, !ascending);
        assert_ne!(
            app.filtered_torrents()[0].name,
            first,
            "the order did not turn round"
        );
        let arrow = if app.sort_ascending {
            "Name \u{25B2}"
        } else {
            "Name \u{25BC}"
        };
        assert!(
            texts(&app).iter().any(|t| t == arrow),
            "the head does not say it is sorted"
        );
    }

    #[test]
    fn a_row_press_chooses_and_a_second_shows_its_details() {
        let mut app = seeded();
        let id = app.filtered_torrents()[1].id;
        probe::click(&mut app, Target::Row(id));
        assert_eq!(app.selected_torrent, Some(id));
        assert_eq!(app.active_tab, Tab::Transfers);
        probe::click(&mut app, Target::Row(id));
        assert_eq!(app.active_tab, Tab::Details);
    }

    fn many_torrents(n: usize) -> TorrentApp {
        let mut app = TorrentApp::new();
        for i in 0..n {
            let meta =
                create_sample_torrent(&format!("Item {i:02}"), 1000, 100, "udp://t.example/");
            app.add_torrent(meta, None);
        }
        app.sort_column = SortColumn::Name;
        app.sort_ascending = true;
        app
    }

    /// Rows past the bottom were not drawn, and nothing scrolled.
    #[test]
    fn the_transfer_list_scrolls_and_follows_the_selection() {
        let mut app = many_torrents(40);
        let (_, rows) = TorrentApp::transfer_pane(app.content_rect());
        assert!(rows < 40, "the list is not long enough to scroll");
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::TransferList, -3.0),
            EventResult::Consumed
        );
        assert!(app.transfer_scroll > 0);
        for _ in 0..60 {
            probe::scroll_at_point(&mut app, Target::TransferList, -3.0);
        }
        assert_eq!(app.transfer_scroll, 40 - rows, "the wheel ran past the end");
        for _ in 0..60 {
            probe::scroll_at_point(&mut app, Target::TransferList, 3.0);
        }
        assert_eq!(app.transfer_scroll, 0);
        // Newest name first, so the list on screen is not the order the
        // transfers were added in -- Down must walk the one on screen.
        app.sort_ascending = false;
        for _ in 0..30 {
            probe::key(&mut app, &probe::press(Key::Down));
        }
        let chosen = app.selected_torrent.unwrap();
        assert_eq!(
            app.filtered_torrents()[29].id,
            chosen,
            "Down did not walk the list on screen"
        );
        assert!(
            probe::rect_of(&app, Target::Row(chosen)).is_some(),
            "the chosen transfer is off screen"
        );
    }

    #[test]
    fn a_magnet_link_is_added_from_the_dialog() {
        let mut app = TorrentApp::new();
        probe::key(&mut app, &probe::ctrl(Key::U));
        assert!(app.show_add_dialog, "Ctrl+U opened nothing");
        probe::type_str(&mut app, "not a magnet");
        probe::key(&mut app, &probe::press(Key::Enter));
        let why = app
            .magnet_error
            .clone()
            .expect("a bad link was taken without a word");
        assert!(texts(&app).contains(&why), "the reason is not on screen");
        assert!(app.torrents.is_empty());
        probe::key(&mut app, &probe::ctrl(Key::A));
        let hash = "0123456789abcdef0123456789abcdef01234567";
        probe::type_str(
            &mut app,
            &format!("magnet:?xt=urn:btih:{hash}&dn=Test%20Film"),
        );
        assert!(
            app.magnet_error.is_none(),
            "typing did not clear the complaint"
        );
        probe::click(&mut app, Target::MagnetAdd);
        assert!(!app.show_add_dialog);
        assert_eq!(app.torrents.len(), 1);
        let t = &app.torrents[0];
        assert_eq!(t.name, "Test Film");
        assert_eq!(hex_encode(&t.magnet.as_ref().unwrap().info_hash), hash);
        assert_eq!(app.selected_torrent, Some(t.id));
        // Cancel and Escape leave without adding.
        probe::click(&mut app, Target::AddMagnet);
        probe::type_str(
            &mut app,
            "magnet:?xt=urn:btih:ffffffffffffffffffffffffffffffffffffffff",
        );
        probe::key(&mut app, &probe::press(Key::Escape));
        assert!(!app.show_add_dialog);
        assert_eq!(app.torrents.len(), 1, "Escape added the link");
    }

    #[test]
    fn a_press_behind_the_magnet_dialog_reaches_nothing() {
        let mut app = seeded();
        let id = app.filtered_torrents()[0].id;
        let row = probe::rect_of(&app, Target::Row(id)).unwrap();
        probe::click(&mut app, Target::AddMagnet);
        let (x, y) = (row.x + 4.0, row.y + 4.0);
        assert_eq!(
            app.frame(app.win_width, app.win_height).hit_test(x, y),
            Some(Target::DialogBackdrop)
        );
        assert_eq!(press_at(&mut app, x, y), EventResult::Ignored);
        assert!(app.show_add_dialog);
        assert_eq!(
            app.selected_torrent, None,
            "the press reached the row behind"
        );
        // And its keys are the dialog's: a `1` goes in the link, not the filter.
        app.filter = TorrentFilter::Paused;
        probe::type_str(&mut app, "1");
        assert_eq!(app.filter, TorrentFilter::Paused);
        assert_eq!(app.magnet_input.text(), "1");
    }

    #[test]
    fn the_search_box_filters_and_escape_clears_it() {
        let mut app = seeded();
        probe::click(&mut app, Target::Search);
        assert!(app.search_active);
        probe::type_str(&mut app, "libre");
        assert_eq!(app.search_query, "libre");
        let names: Vec<&str> = app
            .filtered_torrents()
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, ["LibreOffice 7.6.4"]);
        // Delete is the search's backspace's neighbour, not a removal, while
        // the box has the keys.
        let n = app.torrents.len();
        app.selected_torrent = app.torrents.first().map(|t| t.id);
        probe::key(&mut app, &probe::press(Key::Delete));
        assert_eq!(
            app.torrents.len(),
            n,
            "Delete in the search removed a transfer"
        );
        probe::key(&mut app, &probe::press(Key::Escape));
        assert!(!app.search_active && app.search_query.is_empty());
        probe::key(&mut app, &probe::press(Key::Slash));
        assert!(app.search_active, "/ did not start a search");
        // A press anywhere else takes the keys back from the box.
        probe::click(&mut app, Target::TransferList);
        assert!(!app.search_active, "the search box kept the keys");
        probe::type_str(&mut app, "q");
        assert!(
            app.search_query.is_empty(),
            "a key went into the search box"
        );
    }

    #[test]
    fn a_label_can_be_given_and_chosen_by() {
        let (mut app, id) = chosen();
        let label = |app: &TorrentApp| {
            app.torrents
                .iter()
                .find(|t| t.id == id)
                .unwrap()
                .label
                .clone()
        };
        app.torrents
            .iter_mut()
            .find(|t| t.id == id)
            .unwrap()
            .label
            .clear();
        probe::key(&mut app, &probe::press(Key::L));
        assert_eq!(label(&app), app.labels[0]);
        app.active_tab = Tab::Details;
        probe::click(&mut app, Target::CycleLabel);
        assert_eq!(label(&app), app.labels[1]);
        for _ in 1..app.labels.len() {
            probe::click(&mut app, Target::CycleLabel);
        }
        assert_eq!(label(&app), "", "the cycle does not come back to none");
        probe::click(&mut app, Target::ToggleSequential);
        assert!(
            app.torrents
                .iter()
                .find(|t| t.id == id)
                .unwrap()
                .sequential_download
        );
    }

    /// Two files over four pieces of a hundred bytes: the third piece holds
    /// the end of the first file and the start of the second.
    fn two_files() -> (TorrentApp, u32) {
        let mut app = TorrentApp::new();
        let mut meta = create_sample_torrent("Pair", 400, 100, "udp://t.example/");
        meta.files = vec![
            TorrentFile::named("first.bin", 250),
            TorrentFile::named("second.bin", 150),
        ];
        let id = app.add_torrent(meta, None);
        app.selected_torrent = Some(id);
        app.active_tab = Tab::Files;
        (app, id)
    }

    /// A file set to Skip was downloaded all the same: nothing carried its
    /// priority to the pieces the picker reads.
    #[test]
    fn a_skipped_file_is_not_downloaded() {
        let (mut app, id) = two_files();
        // Normal -> High -> Skip.
        probe::click(&mut app, Target::FilePriority(0));
        probe::click(&mut app, Target::FilePriority(0));
        let t = app.torrents.iter().find(|t| t.id == id).unwrap();
        assert_eq!(t.file_priorities[0], FilePriority::Skip);
        assert_eq!(
            (0..4)
                .map(|p| t.pieces.priority(p).unwrap())
                .collect::<Vec<_>>(),
            [0, 0, 5, 5],
            "a piece shared with a wanted file was skipped, or a skipped one kept"
        );
        assert_eq!(
            t.wanted(),
            vec![false, false, true, true],
            "the download would fetch a skipped file's pieces"
        );
    }

    #[test]
    fn the_list_of_keys_is_modal_to_the_pointer() {
        let mut app = seeded();
        let id = app.filtered_torrents()[0].id;
        let row = probe::rect_of(&app, Target::Row(id)).unwrap();
        probe::click(&mut app, Target::Help);
        assert!(app.show_help);
        press_at(&mut app, row.x + 4.0, row.y + 4.0);
        assert!(!app.show_help, "a press left the list up");
        assert_eq!(
            app.selected_torrent, None,
            "the press went through the list"
        );
    }

    #[test]
    fn enter_shows_the_chosen_transfers_details() {
        let (mut app, _) = chosen();
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_eq!(app.active_tab, Tab::Details);
    }

    // ------------------------------------------------------------------
    // A .torrent read as it is: its own bytes hashed, its paths checked
    // ------------------------------------------------------------------

    /// A whole `.torrent` around `info`, which is written as given.
    fn raw_torrent(info: &[u8]) -> Vec<u8> {
        [
            b"d8:announce20:http://t.example/ann4:info".as_slice(),
            info,
            b"e",
        ]
        .concat()
    }

    /// A multi-file torrent in folder `dir` whose `files` list is `files`,
    /// with one piece's hash -- right for up to 16 KiB of files.
    fn multi(files: &[u8]) -> Result<TorrentMetainfo, String> {
        let info = [
            b"d5:files".as_slice(),
            files,
            b"4:name3:dir12:piece lengthi16384e6:pieces20:",
            &[7; 20],
            b"e",
        ]
        .concat();
        TorrentMetainfo::from_bencode(&raw_torrent(&info))
    }

    /// A single-file torrent: `name`, `length` bytes, pieces of
    /// `piece_length`, with `hashes` piece hashes.
    fn single(
        name: &[u8],
        length: i64,
        piece_length: i64,
        hashes: usize,
    ) -> Result<TorrentMetainfo, String> {
        let info = [
            format!("d6:lengthi{length}e4:name{}:", name.len()).as_bytes(),
            name,
            format!("12:piece lengthi{piece_length}e6:pieces{}:", hashes * 20).as_bytes(),
            &vec![7; hashes * 20],
            b"e",
        ]
        .concat();
        TorrentMetainfo::from_bencode(&raw_torrent(&info))
    }

    /// **The info hash is taken over the info dictionary's own bytes.** One
    /// written with its keys out of order -- legal to read, not canonical --
    /// hashed as re-encoded would be a torrent no tracker or peer knows.
    #[test]
    fn the_info_hash_is_over_the_files_own_bytes() {
        let info = [
            b"d4:name4:test6:lengthi5e12:piece lengthi16384e6:pieces20:".as_slice(),
            &[7; 20],
            b"e",
        ]
        .concat();
        let meta = TorrentMetainfo::from_bencode(&raw_torrent(&info)).unwrap();
        assert_eq!(meta.info_hash, sha1::sha1(&info));
        let (value, _) = BencodeParser::parse(&info).unwrap();
        assert_ne!(
            meta.info_hash,
            sha1::sha1(&bencode_encode(&value)),
            "re-encoding sorted the keys, so this test proves nothing"
        );
    }

    /// A path that would climb out of the download's folder, or a part that
    /// could not be one name, is refused -- the whole torrent, not the file.
    #[test]
    fn a_path_that_leaves_the_folder_is_refused() {
        for (files, what) in [
            (b"ld6:lengthi5e4:pathl2:..1:xeee".as_slice(), ".."),
            (b"ld6:lengthi5e4:pathl1:.eee", "."),
            (b"ld6:lengthi5e4:pathl0:eee", "an empty part"),
            (b"ld6:lengthi5e4:pathl3:a/beee", "a slash"),
            (b"ld6:lengthi5e4:pathl3:a\0beee", "a NUL"),
        ] {
            let err = multi(files).expect_err(what);
            assert!(err.contains("outside its folder"), "{what}: {err}");
        }
        let err = single(b"..", 5, 16384, 1).expect_err("a name of ..");
        assert!(err.contains("outside its folder"), "{err}");
        let ok = multi(b"ld6:lengthi5e4:pathl1:a1:beee").unwrap();
        assert_eq!(ok.files[0].parts, vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(ok.files[0].path, "a/b");
    }

    /// A file entry without a length, or with a negative one, is an error:
    /// skipped, it would shift every byte after it into the wrong file.
    #[test]
    fn a_file_without_a_length_is_an_error_not_a_gap() {
        assert!(
            multi(b"ld4:pathl1:aeee")
                .unwrap_err()
                .contains("without a length")
        );
        assert!(multi(b"ld6:lengthi-1e4:pathl1:aeee").is_err());
        assert!(
            multi(b"ld6:lengthi5eee")
                .unwrap_err()
                .contains("without a path")
        );
        assert!(
            multi(b"ld6:lengthi5e4:pathleee")
                .unwrap_err()
                .contains("empty path")
        );
        assert!(single(b"a", -5, 16384, 1).is_err());
    }

    /// One hash a piece, and as many pieces as the files fill; a piece
    /// length that is not a size, or past what this client holds, is refused.
    #[test]
    fn the_pieces_must_fit_the_files() {
        assert!(single(b"a", 5, 16384, 1).is_ok());
        assert!(
            single(b"a", 5, 16384, 2)
                .unwrap_err()
                .contains("which is 1")
        );
        assert!(
            single(b"a", 16385, 16384, 1)
                .unwrap_err()
                .contains("which is 2")
        );
        assert!(
            single(b"a", 0, 16384, 0).is_ok(),
            "an empty file has no pieces"
        );
        assert!(single(b"a", 5, 0, 1).is_err());
        assert!(single(b"a", 5, -16384, 1).is_err());
        let huge = i64::try_from(MAX_PIECE_LENGTH).unwrap() * 2;
        assert!(
            single(b"a", 5, huge, 1)
                .unwrap_err()
                .contains("more than this client holds")
        );
    }

    /// A name that is not UTF-8 is kept as its bytes, and shown with them.
    #[test]
    fn a_name_that_is_not_utf8_is_kept_as_its_bytes() {
        let meta = single(b"caf\xe9", 5, 16384, 1).unwrap();
        assert_eq!(meta.name_bytes, b"caf\xe9");
        assert_eq!(meta.name, "caf\\xE9");
        assert_eq!(meta.files[0].parts, vec![b"caf\xe9".to_vec()]);
    }

    /// Nesting past the bound is an error, not a stack overflow.
    #[test]
    fn deep_nesting_is_refused_not_a_crash() {
        let err = BencodeParser::parse(&vec![b'l'; 100_000]).unwrap_err();
        assert!(err.contains("nested"), "{err}");
        // As deep as the bound allows still reads.
        let fine = [vec![b'l'; 60], vec![b'e'; 60]].concat();
        assert!(BencodeParser::parse(&fine).is_ok());
    }

    /// The span of a dictionary's value is its bytes as written.
    #[test]
    fn a_dictionary_values_span_is_its_bytes() {
        let data = b"d1:ai1e4:infod1:bi2ee1:zi3ee";
        let span = BencodeParser::dict_value_span(data, "info")
            .unwrap()
            .unwrap();
        assert_eq!(&data[span], b"d1:bi2ee");
        assert_eq!(BencodeParser::dict_value_span(data, "nope").unwrap(), None);
        assert!(BencodeParser::dict_value_span(b"li1ee", "info").is_err());
        assert!(BencodeParser::dict_value_span(b"d1:a", "info").is_err());
    }

    /// Bytes shown as text: what is not UTF-8 is written `\xNN`.
    #[test]
    fn bytes_are_shown_as_they_are() {
        assert_eq!(shown_bytes(b"plain"), "plain");
        assert_eq!(shown_bytes("caf\u{e9}".as_bytes()), "caf\u{e9}");
        assert_eq!(shown_bytes(b"a\xffb\xfe"), "a\\xFFb\\xFE");
    }
}
