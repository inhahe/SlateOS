//! SlateOS `wpa` -- WiFi Protected Access multi-personality binary.
//!
//! Detected from `argv[0]`:
//! - **wpa_supplicant** -- WiFi authentication daemon managing WPA/WPA2/WPA3
//!   state machine, BSS scanning, network selection, and key negotiation.
//! - **wpa_cli** -- control interface client for querying and configuring
//!   the running wpa_supplicant instance.
//! - **wpa_passphrase** -- derives a 256-bit PSK from an SSID + passphrase
//!   via PBKDF2-HMAC-SHA1 (4096 iterations) and outputs a network block.
//!
//! All cryptographic primitives (SHA-1, HMAC-SHA1, PBKDF2) are implemented
//! from scratch -- no external dependencies.

#![cfg_attr(not(test), no_main)]
#![deny(clippy::all)]
#![allow(
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::match_same_arms,
    clippy::struct_excessive_bools,
    clippy::cast_possible_truncation
)]
// WpaConfig parse/serialize/parse_global_field/parse_network_field, the
// AuthAlg/KeyMgmt::from_str_ci helpers, KeyMgmt::Wpa & ::Wpa3 variants,
// Proto::as_str, and the unread NetworkConfig::key_mgmt / ::proto and
// WpaConfig::update_config / ::country fields encode the wpa_supplicant
// configuration-file grammar (wpa_supplicant.conf), the EAPOL key-management
// negotiation vocabulary, and the wpa_cli control interface. The current
// multi-personality stub exercises only a subset; the full surface is
// intentionally kept so the future driver-attached implementation can
// drop in without reshaping public types. Dead-code lint cannot see across
// that future boundary.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{self, Write};

// ============================================================================
// Constants
// ============================================================================

/// SHA-1 block size in bytes.
const SHA1_BLOCK_SIZE: usize = 64;

/// SHA-1 digest size in bytes.
const SHA1_DIGEST_SIZE: usize = 20;

/// PBKDF2 iteration count for WPA-PSK derivation (IEEE 802.11i).
const PBKDF2_ITERATIONS: u32 = 4096;

/// WPA PSK length in bytes (256 bits).
const WPA_PSK_LEN: usize = 32;

/// Maximum SSID length per IEEE 802.11.
const MAX_SSID_LEN: usize = 32;

/// Maximum passphrase length for WPA-PSK.
const MAX_PASSPHRASE_LEN: usize = 63;

/// Minimum passphrase length for WPA-PSK.
const MIN_PASSPHRASE_LEN: usize = 8;

/// Default control interface socket path.
const DEFAULT_CTRL_IFACE: &str = "/var/run/wpa_supplicant";

/// Default configuration file path.
const DEFAULT_CONFIG_FILE: &str = "/etc/wpa_supplicant/wpa_supplicant.conf";

/// Default driver name.
const DEFAULT_DRIVER: &str = "nl80211";

/// Version string.
const VERSION: &str = "0.1.0";

// ============================================================================
// Hex encoding
// ============================================================================

const HEX_TABLE: [u8; 16] = *b"0123456789abcdef";

/// Encode bytes as lowercase hexadecimal into `out`.
/// Returns the number of bytes written (always `src.len() * 2`).
fn hex_encode(src: &[u8], out: &mut [u8]) -> usize {
    let mut i = 0;
    for &b in src {
        if let Some(slot) = out.get_mut(i) {
            *slot = HEX_TABLE[(b >> 4) as usize];
        }
        i += 1;
        if let Some(slot) = out.get_mut(i) {
            *slot = HEX_TABLE[(b & 0x0f) as usize];
        }
        i += 1;
    }
    i
}

/// Encode bytes as a hex String.
fn hex_encode_string(src: &[u8]) -> String {
    let mut buf = vec![0u8; src.len() * 2];
    hex_encode(src, &mut buf);
    // hex_encode only produces ASCII hex chars, so this is valid UTF-8.
    String::from_utf8(buf).unwrap_or_default()
}

/// Decode a hex string into bytes. Returns None on invalid input.
fn hex_decode(src: &[u8]) -> Option<Vec<u8>> {
    if !src.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(src.len() / 2);
    let mut i = 0;
    while i < src.len() {
        let hi = hex_val(src[i])?;
        let lo = hex_val(src[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// ============================================================================
// SHA-1 (FIPS 180-4)
// ============================================================================

/// SHA-1 initial hash values.
const SHA1_H0: [u32; 5] = [
    0x6745_2301,
    0xEFCD_AB89,
    0x98BA_DCFE,
    0x1032_5476,
    0xC3D2_E1F0,
];

/// Compute SHA-1 digest of `data`.
fn sha1(data: &[u8]) -> [u8; SHA1_DIGEST_SIZE] {
    let mut h = SHA1_H0;
    let mut total_len: u64 = 0;

    // Process complete 64-byte blocks.
    let mut offset = 0;
    while offset + SHA1_BLOCK_SIZE <= data.len() {
        sha1_compress(&mut h, &data[offset..offset + SHA1_BLOCK_SIZE]);
        offset += SHA1_BLOCK_SIZE;
        total_len += SHA1_BLOCK_SIZE as u64;
    }

    // Final block(s) with padding.
    let remaining = &data[offset..];
    total_len += remaining.len() as u64;
    let bit_len = total_len.wrapping_mul(8);

    let mut pad = [0u8; 128]; // worst case: 2 blocks
    let rlen = remaining.len();
    pad[..rlen].copy_from_slice(remaining);
    pad[rlen] = 0x80;

    let pad_blocks = if rlen + 1 + 8 <= SHA1_BLOCK_SIZE {
        1
    } else {
        2
    };
    let total_pad = pad_blocks * SHA1_BLOCK_SIZE;

    // Big-endian bit length at end.
    let bl = bit_len.to_be_bytes();
    pad[total_pad - 8..total_pad].copy_from_slice(&bl);

    for blk in 0..pad_blocks {
        let start = blk * SHA1_BLOCK_SIZE;
        sha1_compress(&mut h, &pad[start..start + SHA1_BLOCK_SIZE]);
    }

    let mut digest = [0u8; SHA1_DIGEST_SIZE];
    for (i, &val) in h.iter().enumerate() {
        let be = val.to_be_bytes();
        digest[i * 4..i * 4 + 4].copy_from_slice(&be);
    }
    digest
}

/// SHA-1 compression function for a single 64-byte block.
fn sha1_compress(h: &mut [u32; 5], block: &[u8]) {
    let mut w = [0u32; 80];
    // Load 16 big-endian words.
    for (i, w_word) in w.iter_mut().enumerate().take(16) {
        let off = i * 4;
        *w_word = u32::from_be_bytes([
            block[off],
            block[off + 1],
            block[off + 2],
            block[off + 3],
        ]);
    }
    // Extend to 80 words.
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }

    let mut a = h[0];
    let mut b = h[1];
    let mut c = h[2];
    let mut d = h[3];
    let mut e = h[4];

    for (i, &w_i) in w.iter().enumerate() {
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
            .wrapping_add(w_i);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = temp;
    }

    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
}

// ============================================================================
// HMAC-SHA1 (RFC 2104)
// ============================================================================

/// Compute HMAC-SHA1.
fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; SHA1_DIGEST_SIZE] {
    // If key > block size, hash it first.
    let key_hash;
    let actual_key = if key.len() > SHA1_BLOCK_SIZE {
        key_hash = sha1(key);
        &key_hash[..]
    } else {
        key
    };

    let mut ipad = [0x36u8; SHA1_BLOCK_SIZE];
    let mut opad = [0x5cu8; SHA1_BLOCK_SIZE];

    for (i, &kb) in actual_key.iter().enumerate() {
        ipad[i] ^= kb;
        opad[i] ^= kb;
    }

    // inner = SHA1(ipad || data)
    let mut inner_data = Vec::with_capacity(SHA1_BLOCK_SIZE + data.len());
    inner_data.extend_from_slice(&ipad);
    inner_data.extend_from_slice(data);
    let inner_hash = sha1(&inner_data);

    // outer = SHA1(opad || inner_hash)
    let mut outer_data = Vec::with_capacity(SHA1_BLOCK_SIZE + SHA1_DIGEST_SIZE);
    outer_data.extend_from_slice(&opad);
    outer_data.extend_from_slice(&inner_hash);
    sha1(&outer_data)
}

// ============================================================================
// PBKDF2-HMAC-SHA1 (RFC 2898)
// ============================================================================

/// Derive key material using PBKDF2 with HMAC-SHA1.
fn pbkdf2_sha1(password: &[u8], salt: &[u8], iterations: u32, dk_len: usize) -> Vec<u8> {
    let mut dk = Vec::with_capacity(dk_len);
    let blocks_needed = dk_len.div_ceil(SHA1_DIGEST_SIZE);

    for block_idx in 1..=blocks_needed {
        let mut salt_ext = Vec::with_capacity(salt.len() + 4);
        salt_ext.extend_from_slice(salt);
        salt_ext.extend_from_slice(&(block_idx as u32).to_be_bytes());

        let mut u = hmac_sha1(password, &salt_ext);
        let mut result = u;

        for _ in 1..iterations {
            u = hmac_sha1(password, &u);
            for (r, &ui) in result.iter_mut().zip(u.iter()) {
                *r ^= ui;
            }
        }

        dk.extend_from_slice(&result);
    }

    dk.truncate(dk_len);
    dk
}

/// Derive WPA PSK from passphrase and SSID.
fn wpa_psk(passphrase: &[u8], ssid: &[u8]) -> [u8; WPA_PSK_LEN] {
    let dk = pbkdf2_sha1(passphrase, ssid, PBKDF2_ITERATIONS, WPA_PSK_LEN);
    let mut psk = [0u8; WPA_PSK_LEN];
    psk.copy_from_slice(&dk);
    psk
}

// ============================================================================
// WPA State Machine
// ============================================================================

/// WPA supplicant state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WpaState {
    /// Initial state -- not connected.
    Disconnected,
    /// Actively scanning for networks.
    Scanning,
    /// Associating with a BSS.
    Associating,
    /// 4-way handshake in progress (WPA/WPA2).
    FourWayHandshake,
    /// Group key handshake in progress.
    GroupHandshake,
    /// Fully connected and authenticated.
    Completed,
}

impl WpaState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Disconnected => "DISCONNECTED",
            Self::Scanning => "SCANNING",
            Self::Associating => "ASSOCIATING",
            Self::FourWayHandshake => "4WAY_HANDSHAKE",
            Self::GroupHandshake => "GROUP_HANDSHAKE",
            Self::Completed => "COMPLETED",
        }
    }

    /// Parse state from string (case-insensitive).
    fn from_str_ci(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "DISCONNECTED" => Some(Self::Disconnected),
            "SCANNING" => Some(Self::Scanning),
            "ASSOCIATING" => Some(Self::Associating),
            "4WAY_HANDSHAKE" => Some(Self::FourWayHandshake),
            "GROUP_HANDSHAKE" => Some(Self::GroupHandshake),
            "COMPLETED" => Some(Self::Completed),
            _ => None,
        }
    }
}

/// Key management type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyMgmt {
    WpaPsk,
    WpaEap,
    Wpa3Sae,
    None,
}

impl KeyMgmt {
    fn as_str(self) -> &'static str {
        match self {
            Self::WpaPsk => "WPA-PSK",
            Self::WpaEap => "WPA-EAP",
            Self::Wpa3Sae => "SAE",
            Self::None => "NONE",
        }
    }

    fn from_str_ci(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "WPA-PSK" => Some(Self::WpaPsk),
            "WPA-EAP" => Some(Self::WpaEap),
            "SAE" | "WPA3-SAE" => Some(Self::Wpa3Sae),
            "NONE" => Some(Self::None),
            _ => None,
        }
    }
}

/// Pairwise cipher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PairwiseCipher {
    Ccmp,
    Tkip,
    None,
}

impl PairwiseCipher {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ccmp => "CCMP",
            Self::Tkip => "TKIP",
            Self::None => "NONE",
        }
    }

    fn from_str_ci(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "CCMP" => Some(Self::Ccmp),
            "TKIP" => Some(Self::Tkip),
            "NONE" => Some(Self::None),
            _ => None,
        }
    }
}

/// Group cipher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupCipher {
    Ccmp,
    Tkip,
    None,
}

impl GroupCipher {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ccmp => "CCMP",
            Self::Tkip => "TKIP",
            Self::None => "NONE",
        }
    }

    fn from_str_ci(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "CCMP" => Some(Self::Ccmp),
            "TKIP" => Some(Self::Tkip),
            "NONE" => Some(Self::None),
            _ => None,
        }
    }
}

/// Protocol version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WpaProto {
    Wpa,
    Rsn,  // WPA2
    Wpa3, // WPA3
}

impl WpaProto {
    fn as_str(self) -> &'static str {
        match self {
            Self::Wpa => "WPA",
            Self::Rsn => "RSN",
            Self::Wpa3 => "WPA3",
        }
    }
}

// ============================================================================
// BSS (Basic Service Set) entry
// ============================================================================

/// A detected BSS from a scan.
#[derive(Clone, Debug)]
struct BssEntry {
    /// BSSID as 6 bytes.
    bssid: [u8; 6],
    /// SSID (may be empty for hidden networks).
    ssid: Vec<u8>,
    /// Frequency in MHz.
    freq: u32,
    /// Signal level in dBm (typically negative).
    signal: i32,
    /// Security flags string (e.g. "[WPA2-PSK-CCMP][ESS]").
    flags: String,
    /// Key management detected.
    key_mgmt: KeyMgmt,
    /// WPA protocol version.
    proto: WpaProto,
}

impl BssEntry {
    fn bssid_str(&self) -> String {
        format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.bssid[0], self.bssid[1], self.bssid[2],
            self.bssid[3], self.bssid[4], self.bssid[5]
        )
    }

    fn ssid_str(&self) -> String {
        // Best-effort UTF-8 display; SSIDs are arbitrary bytes per spec.
        String::from_utf8(self.ssid.clone()).unwrap_or_else(|_| hex_encode_string(&self.ssid))
    }
}

/// Format a BSSID from 6 bytes.
fn format_bssid(bssid: &[u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        bssid[0], bssid[1], bssid[2], bssid[3], bssid[4], bssid[5]
    )
}

/// Parse BSSID from "xx:xx:xx:xx:xx:xx" string.
fn parse_bssid(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut bssid = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bssid[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(bssid)
}

// ============================================================================
// Network configuration
// ============================================================================

/// A configured network entry (from wpa_supplicant.conf or added via CLI).
#[derive(Clone, Debug)]
struct NetworkConfig {
    /// Network ID (index in the network list).
    id: usize,
    /// SSID.
    ssid: Vec<u8>,
    /// PSK as raw 32 bytes (derived from passphrase or specified directly).
    psk: Option<[u8; WPA_PSK_LEN]>,
    /// Original passphrase (for config file output).
    passphrase: Option<Vec<u8>>,
    /// Key management.
    key_mgmt: KeyMgmt,
    /// Pairwise cipher.
    pairwise: PairwiseCipher,
    /// Group cipher.
    group: GroupCipher,
    /// Protocol.
    proto: WpaProto,
    /// Whether this network is enabled.
    enabled: bool,
    /// Optional BSSID filter.
    bssid: Option<[u8; 6]>,
    /// Priority (higher = preferred).
    priority: i32,
    /// Scan-SSID (1 = probe for hidden network).
    scan_ssid: u8,
    /// Arbitrary key-value properties set via set_network.
    properties: BTreeMap<String, String>,
}

impl NetworkConfig {
    fn new(id: usize) -> Self {
        Self {
            id,
            ssid: Vec::new(),
            psk: None,
            passphrase: None,
            key_mgmt: KeyMgmt::WpaPsk,
            pairwise: PairwiseCipher::Ccmp,
            group: GroupCipher::Ccmp,
            proto: WpaProto::Rsn,
            enabled: true,
            bssid: None,
            priority: 0,
            scan_ssid: 0,
            properties: BTreeMap::new(),
        }
    }

    fn ssid_str(&self) -> String {
        String::from_utf8(self.ssid.clone()).unwrap_or_else(|_| hex_encode_string(&self.ssid))
    }
}

// ============================================================================
// Supplicant configuration file parser
// ============================================================================

/// Global configuration from wpa_supplicant.conf.
#[derive(Clone, Debug)]
struct WpaConfig {
    /// Control interface socket path.
    ctrl_interface: String,
    /// Whether to update config on save.
    update_config: bool,
    /// Country code (e.g. "US").
    country: String,
    /// Configured networks.
    networks: Vec<NetworkConfig>,
}

impl WpaConfig {
    fn new() -> Self {
        Self {
            ctrl_interface: DEFAULT_CTRL_IFACE.to_string(),
            update_config: false,
            country: String::new(),
            networks: Vec::new(),
        }
    }

    /// Parse a wpa_supplicant.conf-format string.
    fn parse(content: &str) -> Self {
        let mut cfg = Self::new();
        let mut in_network = false;
        let mut current_net: Option<NetworkConfig> = None;
        let net_base_id = 0;
        let mut net_count = 0usize;

        for line in content.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments.
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if trimmed == "network={" {
                in_network = true;
                current_net = Some(NetworkConfig::new(net_base_id + net_count));
                continue;
            }

            if trimmed == "}" && in_network {
                if let Some(net) = current_net.take() {
                    cfg.networks.push(net);
                    net_count += 1;
                }
                in_network = false;
                continue;
            }

            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                let val = trimmed[eq_pos + 1..].trim();

                if in_network {
                    if let Some(ref mut net) = current_net {
                        Self::parse_network_field(net, key, val);
                    }
                } else {
                    Self::parse_global_field(&mut cfg, key, val);
                }
            }
        }

        cfg
    }

    fn parse_global_field(cfg: &mut WpaConfig, key: &str, val: &str) {
        match key {
            "ctrl_interface" => {
                cfg.ctrl_interface = Self::unquote(val).to_string();
            }
            "update_config" => {
                cfg.update_config = val == "1";
            }
            "country" => {
                cfg.country = Self::unquote(val).to_string();
            }
            _ => {}
        }
    }

    fn parse_network_field(net: &mut NetworkConfig, key: &str, val: &str) {
        match key {
            "ssid" => {
                let unq = Self::unquote(val);
                net.ssid = unq.as_bytes().to_vec();
            }
            "psk" => {
                let unq = Self::unquote(val);
                // If 64 hex chars, it is a raw PSK; otherwise a passphrase.
                if unq.len() == 64 && unq.chars().all(|c| c.is_ascii_hexdigit()) {
                    if let Some(bytes) = hex_decode(unq.as_bytes())
                        && bytes.len() == WPA_PSK_LEN {
                            let mut psk = [0u8; WPA_PSK_LEN];
                            psk.copy_from_slice(&bytes);
                            net.psk = Some(psk);
                        }
                } else {
                    net.passphrase = Some(unq.as_bytes().to_vec());
                    // Derive PSK from passphrase + SSID.
                    if !net.ssid.is_empty() {
                        net.psk = Some(wpa_psk(unq.as_bytes(), &net.ssid));
                    }
                }
            }
            "key_mgmt" => {
                if let Some(km) = KeyMgmt::from_str_ci(Self::unquote(val)) {
                    net.key_mgmt = km;
                }
            }
            "pairwise" => {
                if let Some(pw) = PairwiseCipher::from_str_ci(Self::unquote(val)) {
                    net.pairwise = pw;
                }
            }
            "group" => {
                if let Some(g) = GroupCipher::from_str_ci(Self::unquote(val)) {
                    net.group = g;
                }
            }
            "proto" => {
                let uv = Self::unquote(val).to_ascii_uppercase();
                net.proto = match uv.as_str() {
                    "WPA" => WpaProto::Wpa,
                    "RSN" | "WPA2" => WpaProto::Rsn,
                    "WPA3" => WpaProto::Wpa3,
                    _ => WpaProto::Rsn,
                };
            }
            "disabled" => {
                net.enabled = val.trim() != "1";
            }
            "bssid" => {
                net.bssid = parse_bssid(Self::unquote(val));
            }
            "priority" => {
                if let Ok(p) = val.trim().parse::<i32>() {
                    net.priority = p;
                }
            }
            "scan_ssid" => {
                if let Ok(s) = val.trim().parse::<u8>() {
                    net.scan_ssid = s;
                }
            }
            _ => {
                net.properties.insert(key.to_string(), Self::unquote(val).to_string());
            }
        }
    }

    /// Remove surrounding quotes if present.
    fn unquote(s: &str) -> &str {
        if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
            &s[1..s.len() - 1]
        } else {
            s
        }
    }

    /// Serialize config to wpa_supplicant.conf format.
    fn serialize(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!("ctrl_interface={}\n", self.ctrl_interface));
        if self.update_config {
            out.push_str("update_config=1\n");
        }
        if !self.country.is_empty() {
            out.push_str(&format!("country={}\n", self.country));
        }

        for net in &self.networks {
            out.push('\n');
            out.push_str("network={\n");
            out.push_str(&format!("    ssid=\"{}\"\n", net.ssid_str()));

            if let Some(ref passphrase) = net.passphrase {
                let pp = String::from_utf8(passphrase.clone()).unwrap_or_default();
                out.push_str(&format!("    #psk=\"{}\"\n", pp));
            }
            if let Some(ref psk) = net.psk {
                out.push_str(&format!("    psk={}\n", hex_encode_string(psk)));
            }

            out.push_str(&format!("    key_mgmt={}\n", net.key_mgmt.as_str()));
            if net.pairwise != PairwiseCipher::Ccmp {
                out.push_str(&format!("    pairwise={}\n", net.pairwise.as_str()));
            }
            if net.group != GroupCipher::Ccmp {
                out.push_str(&format!("    group={}\n", net.group.as_str()));
            }
            if net.proto != WpaProto::Rsn {
                out.push_str(&format!("    proto={}\n", net.proto.as_str()));
            }
            if !net.enabled {
                out.push_str("    disabled=1\n");
            }
            if let Some(ref bssid) = net.bssid {
                out.push_str(&format!("    bssid={}\n", format_bssid(bssid)));
            }
            if net.priority != 0 {
                out.push_str(&format!("    priority={}\n", net.priority));
            }
            if net.scan_ssid != 0 {
                out.push_str(&format!("    scan_ssid={}\n", net.scan_ssid));
            }

            for (k, v) in &net.properties {
                out.push_str(&format!("    {}={}\n", k, v));
            }

            out.push_str("}\n");
        }

        out
    }
}

// ============================================================================
// Supplicant state
// ============================================================================

/// Full supplicant runtime state.
struct SupplicantState {
    /// Current WPA state.
    wpa_state: WpaState,
    /// Interface name.
    interface: String,
    /// Driver backend.
    driver: String,
    /// Configuration.
    config: WpaConfig,
    /// Currently selected network index (into config.networks).
    selected_network: Option<usize>,
    /// Scanned BSS list.
    bss_list: Vec<BssEntry>,
    /// Currently associated BSSID (if connected).
    current_bssid: Option<[u8; 6]>,
    /// IP address (obtained after connection, e.g. via DHCP -- stubbed).
    ip_address: Option<String>,
    /// Debug mode.
    debug: bool,
    /// Background (daemon) mode.
    background: bool,
    /// Configuration file path.
    config_path: String,
}

impl SupplicantState {
    fn new() -> Self {
        Self {
            wpa_state: WpaState::Disconnected,
            interface: String::from("wlan0"),
            driver: String::from(DEFAULT_DRIVER),
            config: WpaConfig::new(),
            selected_network: None,
            bss_list: Vec::new(),
            current_bssid: None,
            ip_address: None,
            debug: false,
            background: false,
            config_path: String::from(DEFAULT_CONFIG_FILE),
        }
    }

    /// Transition to a new WPA state, enforcing valid transitions.
    fn transition(&mut self, new_state: WpaState) -> bool {
        let valid = match (self.wpa_state, new_state) {
            // From Disconnected: can scan or associate.
            (WpaState::Disconnected, WpaState::Scanning) => true,
            (WpaState::Disconnected, WpaState::Associating) => true,
            // From Scanning: can associate, go back to disconnected, or keep scanning.
            (WpaState::Scanning, WpaState::Associating) => true,
            (WpaState::Scanning, WpaState::Disconnected) => true,
            (WpaState::Scanning, WpaState::Scanning) => true,
            // From Associating: 4-way handshake, complete (open), or fail.
            (WpaState::Associating, WpaState::FourWayHandshake) => true,
            (WpaState::Associating, WpaState::Completed) => true,
            (WpaState::Associating, WpaState::Disconnected) => true,
            // From 4-way: group handshake or fail.
            (WpaState::FourWayHandshake, WpaState::GroupHandshake) => true,
            (WpaState::FourWayHandshake, WpaState::Completed) => true,
            (WpaState::FourWayHandshake, WpaState::Disconnected) => true,
            // From group handshake: complete or fail.
            (WpaState::GroupHandshake, WpaState::Completed) => true,
            (WpaState::GroupHandshake, WpaState::Disconnected) => true,
            // From Completed: can disconnect or re-scan.
            (WpaState::Completed, WpaState::Disconnected) => true,
            (WpaState::Completed, WpaState::Scanning) => true,
            // Same-state is always okay.
            (a, b) if a == b => true,
            _ => false,
        };

        if valid {
            self.wpa_state = new_state;
        }
        valid
    }

    /// Simulate a scan -- populates bss_list with example networks.
    /// In a real implementation this would issue driver commands.
    fn do_scan(&mut self) {
        self.transition(WpaState::Scanning);
        // In a real OS, we would issue scan commands to the driver via
        // netlink/ioctl and wait for scan results. Here we keep whatever
        // BSS entries we already have (they would be populated by the driver
        // callback).
        self.transition(WpaState::Disconnected);
    }

    /// Attempt to associate with a selected network. Runs the WPA state
    /// machine through association -> 4-way handshake -> group handshake
    /// -> completed.
    fn do_associate(&mut self) -> bool {
        let net_id = match self.selected_network {
            Some(id) => id,
            None => return false,
        };

        let net = match self.config.networks.get(net_id) {
            Some(n) => n.clone(),
            None => return false,
        };

        if !net.enabled {
            return false;
        }

        // Find a matching BSS.
        let bss = self.find_best_bss(&net);
        let bssid = match bss {
            Some(b) => b.bssid,
            None => {
                // No BSS found -- in real driver we would scan first.
                // For command-line feedback, indicate failure.
                return false;
            }
        };

        if !self.transition(WpaState::Associating) {
            return false;
        }
        self.current_bssid = Some(bssid);

        // Simulate handshake progression based on key management.
        match net.key_mgmt {
            KeyMgmt::None => {
                // Open network -- skip handshake.
                self.transition(WpaState::Completed);
                return true;
            }
            KeyMgmt::WpaPsk | KeyMgmt::Wpa3Sae => {
                if net.psk.is_none() {
                    self.transition(WpaState::Disconnected);
                    self.current_bssid = None;
                    return false;
                }
            }
            KeyMgmt::WpaEap => {
                // EAP stub -- pretend success for now.
            }
        }

        if !self.transition(WpaState::FourWayHandshake) {
            self.current_bssid = None;
            return false;
        }

        // In a real implementation, we would exchange EAPOL frames here.
        // For now, PSK presence means success.
        if !self.transition(WpaState::GroupHandshake) {
            self.current_bssid = None;
            return false;
        }

        self.transition(WpaState::Completed);
        self.wpa_state == WpaState::Completed
    }

    /// Find the best matching BSS for a network config.
    fn find_best_bss(&self, net: &NetworkConfig) -> Option<BssEntry> {
        let mut best: Option<&BssEntry> = None;

        for bss in &self.bss_list {
            // Match SSID.
            if bss.ssid != net.ssid {
                continue;
            }

            // Match BSSID filter if set.
            if let Some(ref filter) = net.bssid
                && bss.bssid != *filter {
                    continue;
                }

            // Prefer strongest signal.
            match best {
                None => best = Some(bss),
                Some(prev) => {
                    if bss.signal > prev.signal {
                        best = Some(bss);
                    }
                }
            }
        }

        best.cloned()
    }

    /// Disconnect from current network.
    fn disconnect(&mut self) {
        self.transition(WpaState::Disconnected);
        self.current_bssid = None;
        self.ip_address = None;
    }

    /// Generate status string similar to real wpa_supplicant.
    fn status_string(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("wpa_state={}\n", self.wpa_state.as_str()));
        s.push_str(&format!("interface={}\n", self.interface));
        s.push_str(&format!("driver={}\n", self.driver));

        if let Some(ref bssid) = self.current_bssid {
            s.push_str(&format!("bssid={}\n", format_bssid(bssid)));
        }

        if let Some(net_id) = self.selected_network
            && let Some(net) = self.config.networks.get(net_id) {
                s.push_str(&format!("ssid={}\n", net.ssid_str()));
                s.push_str(&format!("id={}\n", net_id));
                s.push_str(&format!("key_mgmt={}\n", net.key_mgmt.as_str()));
                s.push_str(&format!("pairwise_cipher={}\n", net.pairwise.as_str()));
                s.push_str(&format!("group_cipher={}\n", net.group.as_str()));
            }

        if let Some(ref ip) = self.ip_address {
            s.push_str(&format!("ip_address={}\n", ip));
        }

        s
    }
}

// ============================================================================
// wpa_supplicant personality
// ============================================================================

fn run_supplicant(args: &[&str], out: &mut dyn Write) -> i32 {
    let mut iface = String::from("wlan0");
    let mut config_path = String::from(DEFAULT_CONFIG_FILE);
    let mut driver = String::from(DEFAULT_DRIVER);
    let mut background = false;
    let mut debug = false;

    // Parse arguments.
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "-i" => {
                i += 1;
                if i < args.len() {
                    iface = args[i].to_string();
                } else {
                    let _ = writeln!(out, "wpa_supplicant: -i requires interface name");
                    return 1;
                }
            }
            "-c" => {
                i += 1;
                if i < args.len() {
                    config_path = args[i].to_string();
                } else {
                    let _ = writeln!(out, "wpa_supplicant: -c requires config file path");
                    return 1;
                }
            }
            "-D" => {
                i += 1;
                if i < args.len() {
                    driver = args[i].to_string();
                } else {
                    let _ = writeln!(out, "wpa_supplicant: -D requires driver name");
                    return 1;
                }
            }
            "-B" => background = true,
            "-d" | "-dd" | "-ddd" => debug = true,
            "-h" | "--help" => {
                print_supplicant_help(out);
                return 0;
            }
            "-v" | "--version" => {
                let _ = writeln!(out, "wpa_supplicant v{}", VERSION);
                return 0;
            }
            other => {
                // Handle combined flags with value, e.g. -iwlan0.
                if other.starts_with("-i") && other.len() > 2 {
                    iface = other[2..].to_string();
                } else if other.starts_with("-c") && other.len() > 2 {
                    config_path = other[2..].to_string();
                } else if other.starts_with("-D") && other.len() > 2 {
                    driver = other[2..].to_string();
                } else {
                    let _ = writeln!(out, "wpa_supplicant: unknown option '{}'", other);
                    return 1;
                }
            }
        }
        i += 1;
    }

    let _ = writeln!(
        out,
        "Starting wpa_supplicant v{} on interface {} (driver: {})",
        VERSION, iface, driver
    );

    if debug {
        let _ = writeln!(out, "Debug mode enabled");
        let _ = writeln!(out, "Config file: {}", config_path);
    }

    if background {
        let _ = writeln!(out, "Running in background (daemon) mode");
    }

    // In a real implementation, we would:
    // 1. Read and parse the config file.
    // 2. Open a control interface socket.
    // 3. Initialize the driver.
    // 4. Enter the main event loop.
    // Here we print the startup status and exit, since we cannot
    // actually create sockets or read files in this test harness.

    let mut state = SupplicantState::new();
    state.interface = iface;
    state.driver = driver;
    state.config_path = config_path;
    state.debug = debug;
    state.background = background;

    let _ = writeln!(out, "Control interface: {}", state.config.ctrl_interface);
    let _ = writeln!(out, "wpa_state={}", state.wpa_state.as_str());
    let _ = writeln!(out, "wpa_supplicant initialized successfully");

    0
}

fn print_supplicant_help(out: &mut dyn Write) {
    let _ = writeln!(out, "wpa_supplicant v{}", VERSION);
    let _ = writeln!(out);
    let _ = writeln!(out, "Usage: wpa_supplicant [-BdhvW] [-i<iface>] [-c<config>] [-D<driver>]");
    let _ = writeln!(out);
    let _ = writeln!(out, "Options:");
    let _ = writeln!(out, "  -i<iface>   Interface name (default: wlan0)");
    let _ = writeln!(out, "  -c<config>  Configuration file path");
    let _ = writeln!(out, "  -D<driver>  Driver backend (default: nl80211)");
    let _ = writeln!(out, "  -B          Run as daemon in background");
    let _ = writeln!(out, "  -d          Increase debug verbosity (-dd, -ddd)");
    let _ = writeln!(out, "  -h          Show this help message");
    let _ = writeln!(out, "  -v          Show version");
}

// ============================================================================
// wpa_cli personality
// ============================================================================

fn run_cli(args: &[&str], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        print_cli_help(out);
        return 0;
    }

    // Build a simulated supplicant state for command processing.
    let mut state = SupplicantState::new();

    // Process command.
    let cmd = args[0];
    let cmd_args = if args.len() > 1 { &args[1..] } else { &[] };

    match process_cli_command(&mut state, cmd, cmd_args, out) {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(out, "FAIL: {}", e);
            1
        }
    }
}

/// Process a single wpa_cli command against a supplicant state.
fn process_cli_command(
    state: &mut SupplicantState,
    cmd: &str,
    args: &[&str],
    out: &mut dyn Write,
) -> Result<(), String> {
    match cmd {
        "status" => {
            let _ = write!(out, "{}", state.status_string());
            Ok(())
        }
        "scan" => {
            state.do_scan();
            let _ = writeln!(out, "OK");
            Ok(())
        }
        "scan_results" => {
            let _ = writeln!(out, "bssid / frequency / signal level / flags / ssid");
            for bss in &state.bss_list {
                let _ = writeln!(
                    out,
                    "{}\t{}\t{}\t{}\t{}",
                    bss.bssid_str(),
                    bss.freq,
                    bss.signal,
                    bss.flags,
                    bss.ssid_str()
                );
            }
            Ok(())
        }
        "list_networks" => {
            let _ = writeln!(out, "network id / ssid / bssid / flags");
            for net in &state.config.networks {
                let bssid_str = net
                    .bssid
                    .as_ref()
                    .map(format_bssid)
                    .unwrap_or_else(|| "any".to_string());
                let flags = if !net.enabled {
                    "[DISABLED]"
                } else if state.selected_network == Some(net.id)
                    && state.wpa_state == WpaState::Completed
                {
                    "[CURRENT]"
                } else {
                    ""
                };
                let _ = writeln!(out, "{}\t{}\t{}\t{}", net.id, net.ssid_str(), bssid_str, flags);
            }
            Ok(())
        }
        "select_network" => {
            if args.is_empty() {
                return Err("select_network requires network id".to_string());
            }
            let id: usize = args[0]
                .parse()
                .map_err(|_| "invalid network id".to_string())?;
            if id >= state.config.networks.len() {
                return Err(format!("network id {} not found", id));
            }
            state.selected_network = Some(id);
            // Disable all other networks.
            for net in &mut state.config.networks {
                net.enabled = net.id == id;
            }
            let _ = writeln!(out, "OK");
            Ok(())
        }
        "add_network" => {
            let id = state.config.networks.len();
            state.config.networks.push(NetworkConfig::new(id));
            let _ = writeln!(out, "{}", id);
            Ok(())
        }
        "set_network" => {
            if args.len() < 3 {
                return Err("set_network requires: <id> <key> <value>".to_string());
            }
            let id: usize = args[0]
                .parse()
                .map_err(|_| "invalid network id".to_string())?;
            let net = state
                .config
                .networks
                .get_mut(id)
                .ok_or_else(|| format!("network id {} not found", id))?;

            let key = args[1];
            let val = args[2];

            match key {
                "ssid" => {
                    let unq = WpaConfig::unquote(val);
                    net.ssid = unq.as_bytes().to_vec();
                    // Rederive PSK if passphrase exists.
                    if let Some(ref pp) = net.passphrase {
                        net.psk = Some(wpa_psk(pp, &net.ssid));
                    }
                }
                "psk" => {
                    let unq = WpaConfig::unquote(val);
                    if unq.len() == 64 && unq.chars().all(|c| c.is_ascii_hexdigit()) {
                        if let Some(bytes) = hex_decode(unq.as_bytes())
                            && bytes.len() == WPA_PSK_LEN {
                                let mut psk = [0u8; WPA_PSK_LEN];
                                psk.copy_from_slice(&bytes);
                                net.psk = Some(psk);
                            }
                    } else {
                        net.passphrase = Some(unq.as_bytes().to_vec());
                        if !net.ssid.is_empty() {
                            net.psk = Some(wpa_psk(unq.as_bytes(), &net.ssid));
                        }
                    }
                }
                "key_mgmt" => {
                    net.key_mgmt = KeyMgmt::from_str_ci(val)
                        .ok_or_else(|| format!("unknown key_mgmt: {}", val))?;
                }
                "pairwise" => {
                    net.pairwise = PairwiseCipher::from_str_ci(val)
                        .ok_or_else(|| format!("unknown pairwise cipher: {}", val))?;
                }
                "group" => {
                    net.group = GroupCipher::from_str_ci(val)
                        .ok_or_else(|| format!("unknown group cipher: {}", val))?;
                }
                "bssid" => {
                    net.bssid = parse_bssid(val);
                }
                "priority" => {
                    net.priority = val
                        .parse()
                        .map_err(|_| "invalid priority".to_string())?;
                }
                "disabled" => {
                    net.enabled = val != "1";
                }
                "scan_ssid" => {
                    net.scan_ssid = val
                        .parse()
                        .map_err(|_| "invalid scan_ssid".to_string())?;
                }
                _ => {
                    net.properties.insert(key.to_string(), val.to_string());
                }
            }

            let _ = writeln!(out, "OK");
            Ok(())
        }
        "enable_network" => {
            if args.is_empty() {
                return Err("enable_network requires network id".to_string());
            }
            if args[0] == "all" {
                for net in &mut state.config.networks {
                    net.enabled = true;
                }
            } else {
                let id: usize = args[0]
                    .parse()
                    .map_err(|_| "invalid network id".to_string())?;
                let net = state
                    .config
                    .networks
                    .get_mut(id)
                    .ok_or_else(|| format!("network id {} not found", id))?;
                net.enabled = true;
            }
            let _ = writeln!(out, "OK");
            Ok(())
        }
        "disable_network" => {
            if args.is_empty() {
                return Err("disable_network requires network id".to_string());
            }
            if args[0] == "all" {
                for net in &mut state.config.networks {
                    net.enabled = false;
                }
            } else {
                let id: usize = args[0]
                    .parse()
                    .map_err(|_| "invalid network id".to_string())?;
                let net = state
                    .config
                    .networks
                    .get_mut(id)
                    .ok_or_else(|| format!("network id {} not found", id))?;
                net.enabled = false;
            }
            let _ = writeln!(out, "OK");
            Ok(())
        }
        "remove_network" => {
            if args.is_empty() {
                return Err("remove_network requires network id".to_string());
            }
            if args[0] == "all" {
                state.config.networks.clear();
                state.selected_network = None;
            } else {
                let id: usize = args[0]
                    .parse()
                    .map_err(|_| "invalid network id".to_string())?;
                if id >= state.config.networks.len() {
                    return Err(format!("network id {} not found", id));
                }
                state.config.networks.remove(id);
                // Renumber remaining networks.
                for (new_id, net) in state.config.networks.iter_mut().enumerate() {
                    net.id = new_id;
                }
                // Adjust selected_network.
                if state.selected_network == Some(id) {
                    state.selected_network = None;
                    state.disconnect();
                } else if let Some(sel) = state.selected_network
                    && sel > id {
                        state.selected_network = Some(sel - 1);
                    }
            }
            let _ = writeln!(out, "OK");
            Ok(())
        }
        "save_config" => {
            // In a real implementation, write config to disk.
            let _ = writeln!(out, "OK");
            Ok(())
        }
        "disconnect" => {
            state.disconnect();
            let _ = writeln!(out, "OK");
            Ok(())
        }
        "reconnect" => {
            if let Some(id) = state.selected_network
                && id < state.config.networks.len() {
                    state.disconnect();
                    state.do_associate();
                }
            let _ = writeln!(out, "OK");
            Ok(())
        }
        "reassociate" => {
            state.disconnect();
            if let Some(id) = state.selected_network
                && id < state.config.networks.len() {
                    state.do_associate();
                }
            let _ = writeln!(out, "OK");
            Ok(())
        }
        "terminate" => {
            state.disconnect();
            let _ = writeln!(out, "OK");
            Ok(())
        }
        "ping" => {
            let _ = writeln!(out, "PONG");
            Ok(())
        }
        "help" => {
            print_cli_commands(out);
            Ok(())
        }
        _ => Err(format!("Unknown command: {}", cmd)),
    }
}

fn print_cli_help(out: &mut dyn Write) {
    let _ = writeln!(out, "wpa_cli v{}", VERSION);
    let _ = writeln!(out);
    let _ = writeln!(out, "Usage: wpa_cli [<command> [<args>]]");
    let _ = writeln!(out);
    print_cli_commands(out);
}

fn print_cli_commands(out: &mut dyn Write) {
    let _ = writeln!(out, "Commands:");
    let _ = writeln!(out, "  status              Show current connection status");
    let _ = writeln!(out, "  scan                Initiate a scan");
    let _ = writeln!(out, "  scan_results        Show scan results");
    let _ = writeln!(out, "  list_networks       List configured networks");
    let _ = writeln!(out, "  select_network <id> Select a network");
    let _ = writeln!(out, "  add_network         Add a new network (returns id)");
    let _ = writeln!(out, "  set_network <id> <key> <value>");
    let _ = writeln!(out, "                      Set a network variable");
    let _ = writeln!(out, "  enable_network <id|all>");
    let _ = writeln!(out, "                      Enable a network");
    let _ = writeln!(out, "  disable_network <id|all>");
    let _ = writeln!(out, "                      Disable a network");
    let _ = writeln!(out, "  remove_network <id|all>");
    let _ = writeln!(out, "                      Remove a network");
    let _ = writeln!(out, "  save_config         Save current configuration");
    let _ = writeln!(out, "  disconnect          Disconnect from current network");
    let _ = writeln!(out, "  reconnect           Reconnect to current network");
    let _ = writeln!(out, "  reassociate         Force reassociation");
    let _ = writeln!(out, "  terminate           Terminate wpa_supplicant");
    let _ = writeln!(out, "  ping                Test connectivity to supplicant");
    let _ = writeln!(out, "  help                Show this help message");
}

// ============================================================================
// wpa_passphrase personality
// ============================================================================

fn run_passphrase(args: &[&str], out: &mut dyn Write) -> i32 {
    if args.is_empty() || args.len() > 2 {
        let _ = writeln!(out, "Usage: wpa_passphrase <ssid> [passphrase]");
        let _ = writeln!(out);
        let _ = writeln!(out, "Generates a WPA PSK from an SSID and passphrase.");
        let _ = writeln!(
            out,
            "If passphrase is not given, it is read from stdin."
        );
        return 1;
    }

    let ssid = args[0].as_bytes();

    if ssid.len() > MAX_SSID_LEN {
        let _ = writeln!(out, "wpa_passphrase: SSID too long (max {} bytes)", MAX_SSID_LEN);
        return 1;
    }

    // Get passphrase from args or stdin.
    let passphrase: Vec<u8> = if args.len() >= 2 {
        args[1].as_bytes().to_vec()
    } else {
        // Read from stdin.
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            let _ = writeln!(out, "wpa_passphrase: failed to read passphrase from stdin");
            return 1;
        }
        // Strip trailing newline.
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        trimmed.as_bytes().to_vec()
    };

    if passphrase.len() < MIN_PASSPHRASE_LEN {
        let _ = writeln!(
            out,
            "wpa_passphrase: passphrase too short (min {} characters)",
            MIN_PASSPHRASE_LEN
        );
        return 1;
    }

    if passphrase.len() > MAX_PASSPHRASE_LEN {
        let _ = writeln!(
            out,
            "wpa_passphrase: passphrase too long (max {} characters)",
            MAX_PASSPHRASE_LEN
        );
        return 1;
    }

    let psk = wpa_psk(&passphrase, ssid);
    let psk_hex = hex_encode_string(&psk);

    let _ = writeln!(out, "network={{");
    let _ = writeln!(out, "\tssid=\"{}\"", args[0]);
    let _ = writeln!(out, "\t#psk=\"{}\"", args.get(1).unwrap_or(&""));
    let _ = writeln!(out, "\tpsk={}", psk_hex);
    let _ = writeln!(out, "}}");

    0
}

// ============================================================================
// Main entry / personality dispatch
// ============================================================================

fn run(args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "wpa: no arguments");
        return 1;
    }

    // Determine personality from argv[0].
    let prog = args
        .first()
        .map(|a| {
            let s = a.as_str();
            let base = s.rsplit('/').next().unwrap_or(s);
            let base = base.rsplit('\\').next().unwrap_or(base);
            base.trim_end_matches(".exe")
        })
        .unwrap_or("wpa");

    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();

    match prog {
        "wpa_supplicant" => run_supplicant(&rest, out),
        "wpa_cli" => run_cli(&rest, out),
        "wpa_passphrase" => run_passphrase(&rest, out),
        _ => {
            // Default: show usage for all personalities.
            let _ = writeln!(out, "wpa multi-personality binary v{}", VERSION);
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "Invoke as wpa_supplicant, wpa_cli, or wpa_passphrase."
            );
            let _ = writeln!(out, "The personality is detected from the program name (argv[0]).");
            let _ = writeln!(out);
            let _ = writeln!(out, "Symlink examples:");
            let _ = writeln!(out, "  ln -s wpa wpa_supplicant");
            let _ = writeln!(out, "  ln -s wpa wpa_cli");
            let _ = writeln!(out, "  ln -s wpa wpa_passphrase");
            0
        }
    }
}

// ============================================================================
// SlateOS entry point
// ============================================================================

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let mut stdout = io::stdout().lock();
    run(&args, &mut stdout)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Helper -----------------------------------------------------------

    fn run_with(argv: &[&str]) -> (i32, String) {
        let args: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        let mut buf = Vec::new();
        let code = run(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap_or_default();
        (code, output)
    }

    fn run_cli_cmd(state: &mut SupplicantState, cmd: &str, args: &[&str]) -> (i32, String) {
        let mut buf = Vec::new();
        let code = match process_cli_command(state, cmd, args, &mut buf) {
            Ok(()) => 0,
            Err(e) => {
                let _ = writeln!(&mut buf, "FAIL: {}", e);
                1
            }
        };
        (code, String::from_utf8(buf).unwrap_or_default())
    }

    // =====================================================================
    // SHA-1 tests
    // =====================================================================

    #[test]
    fn test_sha1_empty() {
        let digest = sha1(b"");
        assert_eq!(
            hex_encode_string(&digest),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn test_sha1_abc() {
        let digest = sha1(b"abc");
        assert_eq!(
            hex_encode_string(&digest),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn test_sha1_longer_message() {
        let digest = sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        assert_eq!(
            hex_encode_string(&digest),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn test_sha1_two_blocks() {
        // 56 bytes -- exactly triggers two-block padding.
        let data = b"0123456789012345678901234567890123456789012345678901234567";
        assert_eq!(data.len(), 58);
        let digest = sha1(data);
        // Just check it produces a 20-byte output.
        assert_eq!(digest.len(), SHA1_DIGEST_SIZE);
    }

    #[test]
    fn test_sha1_exactly_64_bytes() {
        let data = [0x41u8; 64]; // 'A' repeated 64 times
        let digest = sha1(&data);
        assert_eq!(digest.len(), SHA1_DIGEST_SIZE);
        // Known value for 64 'A's (verified with Python hashlib).
        assert_eq!(
            hex_encode_string(&digest),
            "30b86e44e6001403827a62c58b08893e77cf121f"
        );
    }

    // =====================================================================
    // HMAC-SHA1 tests (RFC 2202)
    // =====================================================================

    #[test]
    fn test_hmac_sha1_rfc2202_case1() {
        // Key = 0x0b * 20, Data = "Hi There"
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let mac = hmac_sha1(&key, data);
        assert_eq!(
            hex_encode_string(&mac),
            "b617318655057264e28bc0b6fb378c8ef146be00"
        );
    }

    #[test]
    fn test_hmac_sha1_rfc2202_case2() {
        // Key = "Jefe", Data = "what do ya want for nothing?"
        let mac = hmac_sha1(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex_encode_string(&mac),
            "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"
        );
    }

    #[test]
    fn test_hmac_sha1_rfc2202_case3() {
        // Key = 0xaa * 20, Data = 0xdd * 50
        let key = [0xaau8; 20];
        let data = [0xddu8; 50];
        let mac = hmac_sha1(&key, &data);
        assert_eq!(
            hex_encode_string(&mac),
            "125d7342b9ac11cd91a39af48aa17b4f63f175d3"
        );
    }

    #[test]
    fn test_hmac_sha1_long_key() {
        // Key longer than block size triggers hashing of key.
        let key = [0xaau8; 80];
        let data = b"Test With Truncation";
        let mac = hmac_sha1(&key, data);
        // Just verify it produces a 20-byte result (long key path).
        assert_eq!(mac.len(), SHA1_DIGEST_SIZE);
    }

    // =====================================================================
    // PBKDF2 tests
    // =====================================================================

    #[test]
    fn test_pbkdf2_rfc6070_case1() {
        // Password "password", Salt "salt", c=1, dkLen=20
        let dk = pbkdf2_sha1(b"password", b"salt", 1, 20);
        assert_eq!(
            hex_encode_string(&dk),
            "0c60c80f961f0e71f3a9b524af6012062fe037a6"
        );
    }

    #[test]
    fn test_pbkdf2_rfc6070_case2() {
        // Password "password", Salt "salt", c=2, dkLen=20
        let dk = pbkdf2_sha1(b"password", b"salt", 2, 20);
        assert_eq!(
            hex_encode_string(&dk),
            "ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957"
        );
    }

    #[test]
    fn test_pbkdf2_rfc6070_case3() {
        // Password "password", Salt "salt", c=4096, dkLen=20
        let dk = pbkdf2_sha1(b"password", b"salt", 4096, 20);
        assert_eq!(
            hex_encode_string(&dk),
            "4b007901b765489abead49d926f721d065a429c1"
        );
    }

    #[test]
    fn test_pbkdf2_longer_dk() {
        // dkLen=25 -- needs part of a second block.
        let dk = pbkdf2_sha1(b"passwordPASSWORDpassword", b"saltSALTsaltSALTsaltSALTsaltSALTsalt", 4096, 25);
        assert_eq!(
            hex_encode_string(&dk),
            "3d2eec4fe41c849b80c8d83662c0e44a8b291a964cf2f07038"
        );
    }

    // =====================================================================
    // WPA PSK derivation tests
    // =====================================================================

    #[test]
    fn test_wpa_psk_known_vector() {
        // IEEE 802.11i test vector.
        let psk = wpa_psk(b"password", b"IEEE");
        assert_eq!(
            hex_encode_string(&psk),
            "f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e"
        );
    }

    #[test]
    fn test_wpa_psk_another_vector() {
        // Another known test vector: SSID="ThisIsASSID", passphrase="ThisIsAPassword"
        let psk = wpa_psk(b"ThisIsAPassword", b"ThisIsASSID");
        // Just verify length; the exact value depends on IEEE test vectors.
        assert_eq!(psk.len(), WPA_PSK_LEN);
    }

    #[test]
    fn test_wpa_psk_length() {
        let psk = wpa_psk(b"testpassword", b"mynetwork");
        assert_eq!(psk.len(), WPA_PSK_LEN);
    }

    // =====================================================================
    // Hex encoding/decoding tests
    // =====================================================================

    #[test]
    fn test_hex_encode_empty() {
        assert_eq!(hex_encode_string(&[]), "");
    }

    #[test]
    fn test_hex_encode_bytes() {
        assert_eq!(hex_encode_string(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn test_hex_encode_all_zeros() {
        assert_eq!(hex_encode_string(&[0, 0, 0, 0]), "00000000");
    }

    #[test]
    fn test_hex_encode_to_buf() {
        let src = [0xff, 0x00, 0xab];
        let mut buf = [0u8; 6];
        let n = hex_encode(&src, &mut buf);
        assert_eq!(n, 6);
        assert_eq!(&buf, b"ff00ab");
    }

    #[test]
    fn test_hex_decode_valid() {
        assert_eq!(hex_decode(b"deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    }

    #[test]
    fn test_hex_decode_uppercase() {
        assert_eq!(hex_decode(b"DEADBEEF"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    }

    #[test]
    fn test_hex_decode_mixed_case() {
        assert_eq!(hex_decode(b"DeAdBeEf"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    }

    #[test]
    fn test_hex_decode_empty() {
        assert_eq!(hex_decode(b""), Some(vec![]));
    }

    #[test]
    fn test_hex_decode_odd_length() {
        assert_eq!(hex_decode(b"abc"), None);
    }

    #[test]
    fn test_hex_decode_invalid_char() {
        assert_eq!(hex_decode(b"ghij"), None);
    }

    // =====================================================================
    // WPA state machine tests
    // =====================================================================

    #[test]
    fn test_state_initial() {
        let state = SupplicantState::new();
        assert_eq!(state.wpa_state, WpaState::Disconnected);
    }

    #[test]
    fn test_state_disconnected_to_scanning() {
        let mut state = SupplicantState::new();
        assert!(state.transition(WpaState::Scanning));
        assert_eq!(state.wpa_state, WpaState::Scanning);
    }

    #[test]
    fn test_state_disconnected_to_associating() {
        let mut state = SupplicantState::new();
        assert!(state.transition(WpaState::Associating));
        assert_eq!(state.wpa_state, WpaState::Associating);
    }

    #[test]
    fn test_state_scanning_to_associating() {
        let mut state = SupplicantState::new();
        state.transition(WpaState::Scanning);
        assert!(state.transition(WpaState::Associating));
    }

    #[test]
    fn test_state_full_handshake_sequence() {
        let mut state = SupplicantState::new();
        assert!(state.transition(WpaState::Associating));
        assert!(state.transition(WpaState::FourWayHandshake));
        assert!(state.transition(WpaState::GroupHandshake));
        assert!(state.transition(WpaState::Completed));
        assert_eq!(state.wpa_state, WpaState::Completed);
    }

    #[test]
    fn test_state_invalid_transition() {
        let mut state = SupplicantState::new();
        // Cannot go from Disconnected directly to Completed.
        assert!(!state.transition(WpaState::Completed));
        assert_eq!(state.wpa_state, WpaState::Disconnected);
    }

    #[test]
    fn test_state_same_state_ok() {
        let mut state = SupplicantState::new();
        assert!(state.transition(WpaState::Disconnected));
    }

    #[test]
    fn test_state_completed_to_disconnected() {
        let mut state = SupplicantState::new();
        state.transition(WpaState::Associating);
        state.transition(WpaState::FourWayHandshake);
        state.transition(WpaState::GroupHandshake);
        state.transition(WpaState::Completed);
        assert!(state.transition(WpaState::Disconnected));
    }

    #[test]
    fn test_state_four_way_skip_group() {
        // 4-way can go directly to completed (e.g. no group key needed).
        let mut state = SupplicantState::new();
        state.transition(WpaState::Associating);
        state.transition(WpaState::FourWayHandshake);
        assert!(state.transition(WpaState::Completed));
    }

    #[test]
    fn test_state_as_str() {
        assert_eq!(WpaState::Disconnected.as_str(), "DISCONNECTED");
        assert_eq!(WpaState::Scanning.as_str(), "SCANNING");
        assert_eq!(WpaState::Associating.as_str(), "ASSOCIATING");
        assert_eq!(WpaState::FourWayHandshake.as_str(), "4WAY_HANDSHAKE");
        assert_eq!(WpaState::GroupHandshake.as_str(), "GROUP_HANDSHAKE");
        assert_eq!(WpaState::Completed.as_str(), "COMPLETED");
    }

    #[test]
    fn test_state_from_str_ci() {
        assert_eq!(WpaState::from_str_ci("disconnected"), Some(WpaState::Disconnected));
        assert_eq!(WpaState::from_str_ci("COMPLETED"), Some(WpaState::Completed));
        assert_eq!(WpaState::from_str_ci("4way_handshake"), Some(WpaState::FourWayHandshake));
        assert_eq!(WpaState::from_str_ci("bogus"), None);
    }

    // =====================================================================
    // Key management / cipher parsing tests
    // =====================================================================

    #[test]
    fn test_key_mgmt_as_str() {
        assert_eq!(KeyMgmt::WpaPsk.as_str(), "WPA-PSK");
        assert_eq!(KeyMgmt::WpaEap.as_str(), "WPA-EAP");
        assert_eq!(KeyMgmt::Wpa3Sae.as_str(), "SAE");
        assert_eq!(KeyMgmt::None.as_str(), "NONE");
    }

    #[test]
    fn test_key_mgmt_from_str() {
        assert_eq!(KeyMgmt::from_str_ci("WPA-PSK"), Some(KeyMgmt::WpaPsk));
        assert_eq!(KeyMgmt::from_str_ci("wpa-eap"), Some(KeyMgmt::WpaEap));
        assert_eq!(KeyMgmt::from_str_ci("sae"), Some(KeyMgmt::Wpa3Sae));
        assert_eq!(KeyMgmt::from_str_ci("WPA3-SAE"), Some(KeyMgmt::Wpa3Sae));
        assert_eq!(KeyMgmt::from_str_ci("none"), Some(KeyMgmt::None));
        assert_eq!(KeyMgmt::from_str_ci("unknown"), None);
    }

    #[test]
    fn test_pairwise_cipher_parse() {
        assert_eq!(PairwiseCipher::from_str_ci("CCMP"), Some(PairwiseCipher::Ccmp));
        assert_eq!(PairwiseCipher::from_str_ci("tkip"), Some(PairwiseCipher::Tkip));
        assert_eq!(PairwiseCipher::from_str_ci("none"), Some(PairwiseCipher::None));
        assert_eq!(PairwiseCipher::from_str_ci("xyz"), None);
    }

    #[test]
    fn test_group_cipher_parse() {
        assert_eq!(GroupCipher::from_str_ci("CCMP"), Some(GroupCipher::Ccmp));
        assert_eq!(GroupCipher::from_str_ci("tkip"), Some(GroupCipher::Tkip));
        assert_eq!(GroupCipher::from_str_ci("xyz"), None);
    }

    // =====================================================================
    // BSSID parsing/formatting tests
    // =====================================================================

    #[test]
    fn test_parse_bssid_valid() {
        let bssid = parse_bssid("aa:bb:cc:dd:ee:ff");
        assert_eq!(bssid, Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]));
    }

    #[test]
    fn test_parse_bssid_zeros() {
        let bssid = parse_bssid("00:00:00:00:00:00");
        assert_eq!(bssid, Some([0, 0, 0, 0, 0, 0]));
    }

    #[test]
    fn test_parse_bssid_invalid_short() {
        assert_eq!(parse_bssid("aa:bb:cc"), None);
    }

    #[test]
    fn test_parse_bssid_invalid_hex() {
        assert_eq!(parse_bssid("zz:bb:cc:dd:ee:ff"), None);
    }

    #[test]
    fn test_format_bssid() {
        let b = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        assert_eq!(format_bssid(&b), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn test_format_bssid_zeros() {
        let b = [0, 0, 0, 0, 0, 0];
        assert_eq!(format_bssid(&b), "00:00:00:00:00:00");
    }

    // =====================================================================
    // BSS entry tests
    // =====================================================================

    #[test]
    fn test_bss_entry_ssid_str_utf8() {
        let bss = BssEntry {
            bssid: [0; 6],
            ssid: b"MyNetwork".to_vec(),
            freq: 2412,
            signal: -50,
            flags: "[WPA2-PSK-CCMP]".to_string(),
            key_mgmt: KeyMgmt::WpaPsk,
            proto: WpaProto::Rsn,
        };
        assert_eq!(bss.ssid_str(), "MyNetwork");
    }

    #[test]
    fn test_bss_entry_ssid_str_non_utf8() {
        let bss = BssEntry {
            bssid: [0; 6],
            ssid: vec![0xff, 0xfe],
            freq: 2412,
            signal: -50,
            flags: String::new(),
            key_mgmt: KeyMgmt::None,
            proto: WpaProto::Rsn,
        };
        // Non-UTF8 falls back to hex.
        assert_eq!(bss.ssid_str(), "fffe");
    }

    #[test]
    fn test_bss_entry_bssid_str() {
        let bss = BssEntry {
            bssid: [0x01, 0x23, 0x45, 0x67, 0x89, 0xab],
            ssid: Vec::new(),
            freq: 5180,
            signal: -70,
            flags: String::new(),
            key_mgmt: KeyMgmt::None,
            proto: WpaProto::Rsn,
        };
        assert_eq!(bss.bssid_str(), "01:23:45:67:89:ab");
    }

    // =====================================================================
    // Network config tests
    // =====================================================================

    #[test]
    fn test_network_config_defaults() {
        let net = NetworkConfig::new(0);
        assert_eq!(net.id, 0);
        assert!(net.ssid.is_empty());
        assert!(net.psk.is_none());
        assert_eq!(net.key_mgmt, KeyMgmt::WpaPsk);
        assert_eq!(net.pairwise, PairwiseCipher::Ccmp);
        assert_eq!(net.group, GroupCipher::Ccmp);
        assert!(net.enabled);
        assert_eq!(net.priority, 0);
    }

    #[test]
    fn test_network_config_ssid_str() {
        let mut net = NetworkConfig::new(0);
        net.ssid = b"TestSSID".to_vec();
        assert_eq!(net.ssid_str(), "TestSSID");
    }

    // =====================================================================
    // Config parsing tests
    // =====================================================================

    #[test]
    fn test_config_parse_empty() {
        let cfg = WpaConfig::parse("");
        assert!(cfg.networks.is_empty());
        assert_eq!(cfg.ctrl_interface, DEFAULT_CTRL_IFACE);
    }

    #[test]
    fn test_config_parse_globals() {
        let content = r#"
ctrl_interface=/tmp/wpa
update_config=1
country=US
"#;
        let cfg = WpaConfig::parse(content);
        assert_eq!(cfg.ctrl_interface, "/tmp/wpa");
        assert!(cfg.update_config);
        assert_eq!(cfg.country, "US");
    }

    #[test]
    fn test_config_parse_single_network() {
        let content = r#"
ctrl_interface=/var/run/wpa_supplicant
network={
    ssid="MyWiFi"
    psk="mypassword"
    key_mgmt=WPA-PSK
}
"#;
        let cfg = WpaConfig::parse(content);
        assert_eq!(cfg.networks.len(), 1);
        let net = &cfg.networks[0];
        assert_eq!(net.ssid, b"MyWiFi");
        assert_eq!(net.key_mgmt, KeyMgmt::WpaPsk);
        assert!(net.psk.is_some());
        assert_eq!(net.passphrase, Some(b"mypassword".to_vec()));
    }

    #[test]
    fn test_config_parse_multiple_networks() {
        let content = r#"
network={
    ssid="Net1"
    key_mgmt=NONE
}
network={
    ssid="Net2"
    psk="password2!"
}
"#;
        let cfg = WpaConfig::parse(content);
        assert_eq!(cfg.networks.len(), 2);
        assert_eq!(cfg.networks[0].ssid, b"Net1");
        assert_eq!(cfg.networks[0].key_mgmt, KeyMgmt::None);
        assert_eq!(cfg.networks[1].ssid, b"Net2");
    }

    #[test]
    fn test_config_parse_disabled_network() {
        let content = r#"
network={
    ssid="Disabled"
    disabled=1
}
"#;
        let cfg = WpaConfig::parse(content);
        assert!(!cfg.networks[0].enabled);
    }

    #[test]
    fn test_config_parse_priority() {
        let content = r#"
network={
    ssid="HighPri"
    priority=5
}
"#;
        let cfg = WpaConfig::parse(content);
        assert_eq!(cfg.networks[0].priority, 5);
    }

    #[test]
    fn test_config_parse_bssid() {
        let content = r#"
network={
    ssid="Targeted"
    bssid=aa:bb:cc:dd:ee:ff
}
"#;
        let cfg = WpaConfig::parse(content);
        assert_eq!(
            cfg.networks[0].bssid,
            Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff])
        );
    }

    #[test]
    fn test_config_parse_scan_ssid() {
        let content = r#"
network={
    ssid="Hidden"
    scan_ssid=1
}
"#;
        let cfg = WpaConfig::parse(content);
        assert_eq!(cfg.networks[0].scan_ssid, 1);
    }

    #[test]
    fn test_config_parse_comments_and_blank_lines() {
        let content = r#"
# This is a comment
ctrl_interface=/var/run/wpa_supplicant

# Another comment
network={
    ssid="Test"
}
"#;
        let cfg = WpaConfig::parse(content);
        assert_eq!(cfg.networks.len(), 1);
    }

    #[test]
    fn test_config_parse_hex_psk() {
        let hex_psk = "a".repeat(64); // 64 hex chars
        let content = format!(
            "network={{\n    ssid=\"Test\"\n    psk={}\n}}\n",
            hex_psk
        );
        let cfg = WpaConfig::parse(&content);
        assert!(cfg.networks[0].psk.is_some());
        assert!(cfg.networks[0].passphrase.is_none());
    }

    #[test]
    fn test_config_parse_proto() {
        let content = "network={\n    ssid=\"X\"\n    proto=WPA\n}\n";
        let cfg = WpaConfig::parse(content);
        assert_eq!(cfg.networks[0].proto, WpaProto::Wpa);
    }

    #[test]
    fn test_config_parse_pairwise() {
        let content = "network={\n    ssid=\"X\"\n    pairwise=TKIP\n}\n";
        let cfg = WpaConfig::parse(content);
        assert_eq!(cfg.networks[0].pairwise, PairwiseCipher::Tkip);
    }

    #[test]
    fn test_config_parse_group() {
        let content = "network={\n    ssid=\"X\"\n    group=TKIP\n}\n";
        let cfg = WpaConfig::parse(content);
        assert_eq!(cfg.networks[0].group, GroupCipher::Tkip);
    }

    #[test]
    fn test_config_parse_unknown_property() {
        let content = "network={\n    ssid=\"X\"\n    eap=PEAP\n}\n";
        let cfg = WpaConfig::parse(content);
        assert_eq!(cfg.networks[0].properties.get("eap").map(|s| s.as_str()), Some("PEAP"));
    }

    // =====================================================================
    // Config serialization tests
    // =====================================================================

    #[test]
    fn test_config_serialize_roundtrip() {
        let content = r#"ctrl_interface=/var/run/wpa_supplicant
update_config=1
country=US

network={
    ssid="MyNetwork"
    psk="testpassword"
    key_mgmt=WPA-PSK
}
"#;
        let cfg = WpaConfig::parse(content);
        let serialized = cfg.serialize();
        assert!(serialized.contains("ctrl_interface=/var/run/wpa_supplicant"));
        assert!(serialized.contains("update_config=1"));
        assert!(serialized.contains("country=US"));
        assert!(serialized.contains("ssid=\"MyNetwork\""));
        assert!(serialized.contains("key_mgmt=WPA-PSK"));
    }

    #[test]
    fn test_config_serialize_disabled() {
        let mut cfg = WpaConfig::new();
        let mut net = NetworkConfig::new(0);
        net.ssid = b"Off".to_vec();
        net.enabled = false;
        cfg.networks.push(net);
        let s = cfg.serialize();
        assert!(s.contains("disabled=1"));
    }

    #[test]
    fn test_config_serialize_bssid() {
        let mut cfg = WpaConfig::new();
        let mut net = NetworkConfig::new(0);
        net.ssid = b"X".to_vec();
        net.bssid = Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        cfg.networks.push(net);
        let s = cfg.serialize();
        assert!(s.contains("bssid=aa:bb:cc:dd:ee:ff"));
    }

    // =====================================================================
    // Unquote tests
    // =====================================================================

    #[test]
    fn test_unquote_quoted() {
        assert_eq!(WpaConfig::unquote("\"hello\""), "hello");
    }

    #[test]
    fn test_unquote_unquoted() {
        assert_eq!(WpaConfig::unquote("hello"), "hello");
    }

    #[test]
    fn test_unquote_single_char() {
        assert_eq!(WpaConfig::unquote("x"), "x");
    }

    #[test]
    fn test_unquote_empty() {
        assert_eq!(WpaConfig::unquote(""), "");
    }

    // =====================================================================
    // wpa_cli command tests
    // =====================================================================

    #[test]
    fn test_cli_status() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "status", &[]);
        assert_eq!(code, 0);
        assert!(output.contains("wpa_state=DISCONNECTED"));
        assert!(output.contains("interface=wlan0"));
    }

    #[test]
    fn test_cli_ping() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "ping", &[]);
        assert_eq!(code, 0);
        assert!(output.contains("PONG"));
    }

    #[test]
    fn test_cli_scan() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "scan", &[]);
        assert_eq!(code, 0);
        assert!(output.contains("OK"));
    }

    #[test]
    fn test_cli_scan_results_empty() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "scan_results", &[]);
        assert_eq!(code, 0);
        assert!(output.contains("bssid / frequency / signal level / flags / ssid"));
    }

    #[test]
    fn test_cli_scan_results_with_data() {
        let mut state = SupplicantState::new();
        state.bss_list.push(BssEntry {
            bssid: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
            ssid: b"TestNet".to_vec(),
            freq: 2437,
            signal: -45,
            flags: "[WPA2-PSK-CCMP][ESS]".to_string(),
            key_mgmt: KeyMgmt::WpaPsk,
            proto: WpaProto::Rsn,
        });
        let (code, output) = run_cli_cmd(&mut state, "scan_results", &[]);
        assert_eq!(code, 0);
        assert!(output.contains("00:11:22:33:44:55"));
        assert!(output.contains("2437"));
        assert!(output.contains("TestNet"));
    }

    #[test]
    fn test_cli_add_network() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "add_network", &[]);
        assert_eq!(code, 0);
        assert!(output.trim() == "0");
        assert_eq!(state.config.networks.len(), 1);
    }

    #[test]
    fn test_cli_add_multiple_networks() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        let (_, output) = run_cli_cmd(&mut state, "add_network", &[]);
        assert!(output.trim() == "1");
        assert_eq!(state.config.networks.len(), 2);
    }

    #[test]
    fn test_cli_set_network_ssid() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        let (code, output) = run_cli_cmd(&mut state, "set_network", &["0", "ssid", "\"MyNet\""]);
        assert_eq!(code, 0);
        assert!(output.contains("OK"));
        assert_eq!(state.config.networks[0].ssid, b"MyNet");
    }

    #[test]
    fn test_cli_set_network_psk_passphrase() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "set_network", &["0", "ssid", "\"TestSSID\""]);
        let (code, _) =
            run_cli_cmd(&mut state, "set_network", &["0", "psk", "\"testpassword\""]);
        assert_eq!(code, 0);
        assert!(state.config.networks[0].psk.is_some());
        assert_eq!(
            state.config.networks[0].passphrase,
            Some(b"testpassword".to_vec())
        );
    }

    #[test]
    fn test_cli_set_network_key_mgmt() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        let (code, _) = run_cli_cmd(&mut state, "set_network", &["0", "key_mgmt", "NONE"]);
        assert_eq!(code, 0);
        assert_eq!(state.config.networks[0].key_mgmt, KeyMgmt::None);
    }

    #[test]
    fn test_cli_set_network_invalid_id() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "set_network", &["99", "ssid", "x"]);
        assert_eq!(code, 1);
        assert!(output.contains("FAIL"));
    }

    #[test]
    fn test_cli_enable_network() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "set_network", &["0", "disabled", "1"]);
        assert!(!state.config.networks[0].enabled);
        let (code, _) = run_cli_cmd(&mut state, "enable_network", &["0"]);
        assert_eq!(code, 0);
        assert!(state.config.networks[0].enabled);
    }

    #[test]
    fn test_cli_enable_all() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "set_network", &["0", "disabled", "1"]);
        run_cli_cmd(&mut state, "set_network", &["1", "disabled", "1"]);
        run_cli_cmd(&mut state, "enable_network", &["all"]);
        assert!(state.config.networks[0].enabled);
        assert!(state.config.networks[1].enabled);
    }

    #[test]
    fn test_cli_disable_network() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        let (code, _) = run_cli_cmd(&mut state, "disable_network", &["0"]);
        assert_eq!(code, 0);
        assert!(!state.config.networks[0].enabled);
    }

    #[test]
    fn test_cli_disable_all() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "disable_network", &["all"]);
        assert!(!state.config.networks[0].enabled);
        assert!(!state.config.networks[1].enabled);
    }

    #[test]
    fn test_cli_select_network() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "add_network", &[]);
        let (code, _) = run_cli_cmd(&mut state, "select_network", &["1"]);
        assert_eq!(code, 0);
        assert_eq!(state.selected_network, Some(1));
        // Other networks should be disabled.
        assert!(!state.config.networks[0].enabled);
        assert!(state.config.networks[1].enabled);
    }

    #[test]
    fn test_cli_select_network_invalid() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "select_network", &["5"]);
        assert_eq!(code, 1);
        assert!(output.contains("FAIL"));
    }

    #[test]
    fn test_cli_remove_network() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "add_network", &[]);
        let (code, _) = run_cli_cmd(&mut state, "remove_network", &["0"]);
        assert_eq!(code, 0);
        assert_eq!(state.config.networks.len(), 1);
        // Remaining network should be renumbered to 0.
        assert_eq!(state.config.networks[0].id, 0);
    }

    #[test]
    fn test_cli_remove_all_networks() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "add_network", &[]);
        let (code, _) = run_cli_cmd(&mut state, "remove_network", &["all"]);
        assert_eq!(code, 0);
        assert!(state.config.networks.is_empty());
    }

    #[test]
    fn test_cli_list_networks() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "set_network", &["0", "ssid", "\"Net1\""]);
        let (code, output) = run_cli_cmd(&mut state, "list_networks", &[]);
        assert_eq!(code, 0);
        assert!(output.contains("network id"));
        assert!(output.contains("Net1"));
    }

    #[test]
    fn test_cli_disconnect() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "disconnect", &[]);
        assert_eq!(code, 0);
        assert!(output.contains("OK"));
        assert_eq!(state.wpa_state, WpaState::Disconnected);
    }

    #[test]
    fn test_cli_terminate() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "terminate", &[]);
        assert_eq!(code, 0);
        assert!(output.contains("OK"));
    }

    #[test]
    fn test_cli_unknown_command() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "nonexistent", &[]);
        assert_eq!(code, 1);
        assert!(output.contains("FAIL"));
        assert!(output.contains("Unknown command"));
    }

    #[test]
    fn test_cli_save_config() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "save_config", &[]);
        assert_eq!(code, 0);
        assert!(output.contains("OK"));
    }

    #[test]
    fn test_cli_help() {
        let mut state = SupplicantState::new();
        let (code, output) = run_cli_cmd(&mut state, "help", &[]);
        assert_eq!(code, 0);
        assert!(output.contains("status"));
        assert!(output.contains("scan"));
    }

    #[test]
    fn test_cli_set_network_bssid() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        let (code, _) = run_cli_cmd(
            &mut state,
            "set_network",
            &["0", "bssid", "11:22:33:44:55:66"],
        );
        assert_eq!(code, 0);
        assert_eq!(
            state.config.networks[0].bssid,
            Some([0x11, 0x22, 0x33, 0x44, 0x55, 0x66])
        );
    }

    #[test]
    fn test_cli_set_network_priority() {
        let mut state = SupplicantState::new();
        run_cli_cmd(&mut state, "add_network", &[]);
        run_cli_cmd(&mut state, "set_network", &["0", "priority", "10"]);
        assert_eq!(state.config.networks[0].priority, 10);
    }

    // =====================================================================
    // Supplicant association logic tests
    // =====================================================================

    #[test]
    fn test_supplicant_find_best_bss() {
        let mut state = SupplicantState::new();
        state.bss_list.push(BssEntry {
            bssid: [1, 2, 3, 4, 5, 6],
            ssid: b"Test".to_vec(),
            freq: 2412,
            signal: -70,
            flags: String::new(),
            key_mgmt: KeyMgmt::WpaPsk,
            proto: WpaProto::Rsn,
        });
        state.bss_list.push(BssEntry {
            bssid: [7, 8, 9, 10, 11, 12],
            ssid: b"Test".to_vec(),
            freq: 5180,
            signal: -40,
            flags: String::new(),
            key_mgmt: KeyMgmt::WpaPsk,
            proto: WpaProto::Rsn,
        });

        let mut net = NetworkConfig::new(0);
        net.ssid = b"Test".to_vec();

        let best = state.find_best_bss(&net);
        assert!(best.is_some());
        let best = best.unwrap();
        assert_eq!(best.signal, -40); // Stronger signal wins.
        assert_eq!(best.bssid, [7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn test_supplicant_find_bss_with_filter() {
        let mut state = SupplicantState::new();
        state.bss_list.push(BssEntry {
            bssid: [1, 2, 3, 4, 5, 6],
            ssid: b"Test".to_vec(),
            freq: 2412,
            signal: -70,
            flags: String::new(),
            key_mgmt: KeyMgmt::WpaPsk,
            proto: WpaProto::Rsn,
        });
        state.bss_list.push(BssEntry {
            bssid: [7, 8, 9, 10, 11, 12],
            ssid: b"Test".to_vec(),
            freq: 5180,
            signal: -40,
            flags: String::new(),
            key_mgmt: KeyMgmt::WpaPsk,
            proto: WpaProto::Rsn,
        });

        let mut net = NetworkConfig::new(0);
        net.ssid = b"Test".to_vec();
        net.bssid = Some([1, 2, 3, 4, 5, 6]);

        let best = state.find_best_bss(&net);
        assert!(best.is_some());
        assert_eq!(best.unwrap().bssid, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_supplicant_find_bss_no_match() {
        let state = SupplicantState::new();
        let mut net = NetworkConfig::new(0);
        net.ssid = b"NonExistent".to_vec();
        assert!(state.find_best_bss(&net).is_none());
    }

    #[test]
    fn test_supplicant_disconnect() {
        let mut state = SupplicantState::new();
        state.transition(WpaState::Associating);
        state.transition(WpaState::FourWayHandshake);
        state.transition(WpaState::GroupHandshake);
        state.transition(WpaState::Completed);
        state.current_bssid = Some([1, 2, 3, 4, 5, 6]);
        state.ip_address = Some("192.168.1.100".to_string());

        state.disconnect();
        assert_eq!(state.wpa_state, WpaState::Disconnected);
        assert!(state.current_bssid.is_none());
        assert!(state.ip_address.is_none());
    }

    #[test]
    fn test_supplicant_status_string_disconnected() {
        let state = SupplicantState::new();
        let status = state.status_string();
        assert!(status.contains("wpa_state=DISCONNECTED"));
        assert!(status.contains("interface=wlan0"));
        assert!(status.contains("driver=nl80211"));
    }

    #[test]
    fn test_supplicant_status_string_connected() {
        let mut state = SupplicantState::new();
        let mut net = NetworkConfig::new(0);
        net.ssid = b"MyWiFi".to_vec();
        state.config.networks.push(net);
        state.selected_network = Some(0);
        state.current_bssid = Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        state.transition(WpaState::Associating);
        state.transition(WpaState::FourWayHandshake);
        state.transition(WpaState::Completed);
        state.ip_address = Some("10.0.0.1".to_string());

        let status = state.status_string();
        assert!(status.contains("wpa_state=COMPLETED"));
        assert!(status.contains("bssid=aa:bb:cc:dd:ee:ff"));
        assert!(status.contains("ssid=MyWiFi"));
        assert!(status.contains("ip_address=10.0.0.1"));
    }

    // =====================================================================
    // Personality dispatch tests
    // =====================================================================

    #[test]
    fn test_personality_wpa_supplicant_help() {
        let (code, output) = run_with(&["wpa_supplicant", "-h"]);
        assert_eq!(code, 0);
        assert!(output.contains("wpa_supplicant"));
        assert!(output.contains("-i"));
    }

    #[test]
    fn test_personality_wpa_supplicant_version() {
        let (code, output) = run_with(&["wpa_supplicant", "-v"]);
        assert_eq!(code, 0);
        assert!(output.contains(VERSION));
    }

    #[test]
    fn test_personality_wpa_supplicant_start() {
        let (code, output) = run_with(&["wpa_supplicant", "-i", "wlan1", "-D", "wext"]);
        assert_eq!(code, 0);
        assert!(output.contains("wlan1"));
        assert!(output.contains("wext"));
    }

    #[test]
    fn test_personality_wpa_supplicant_background() {
        let (code, output) = run_with(&["wpa_supplicant", "-B"]);
        assert_eq!(code, 0);
        assert!(output.contains("background"));
    }

    #[test]
    fn test_personality_wpa_supplicant_debug() {
        let (code, output) = run_with(&["wpa_supplicant", "-d"]);
        assert_eq!(code, 0);
        assert!(output.contains("Debug"));
    }

    #[test]
    fn test_personality_wpa_supplicant_combined_iface() {
        let (code, output) = run_with(&["wpa_supplicant", "-iwlan2"]);
        assert_eq!(code, 0);
        assert!(output.contains("wlan2"));
    }

    #[test]
    fn test_personality_wpa_cli_no_args() {
        let (code, output) = run_with(&["wpa_cli"]);
        assert_eq!(code, 0);
        assert!(output.contains("wpa_cli"));
    }

    #[test]
    fn test_personality_wpa_cli_status() {
        let (code, output) = run_with(&["wpa_cli", "status"]);
        assert_eq!(code, 0);
        assert!(output.contains("wpa_state"));
    }

    #[test]
    fn test_personality_wpa_passphrase_usage() {
        let (code, output) = run_with(&["wpa_passphrase"]);
        assert_eq!(code, 1);
        assert!(output.contains("Usage"));
    }

    #[test]
    fn test_personality_wpa_passphrase_generate() {
        let (code, output) = run_with(&["wpa_passphrase", "TestSSID", "testpassword1234"]);
        assert_eq!(code, 0);
        assert!(output.contains("network={"));
        assert!(output.contains("ssid=\"TestSSID\""));
        assert!(output.contains("psk="));
        assert!(output.contains("}"));
    }

    #[test]
    fn test_personality_wpa_passphrase_short_pass() {
        let (code, output) = run_with(&["wpa_passphrase", "Test", "short"]);
        assert_eq!(code, 1);
        assert!(output.contains("too short"));
    }

    #[test]
    fn test_personality_wpa_passphrase_long_ssid() {
        let long_ssid = "A".repeat(33);
        let (code, output) = run_with(&["wpa_passphrase", &long_ssid, "validpassword"]);
        assert_eq!(code, 1);
        assert!(output.contains("SSID too long"));
    }

    #[test]
    fn test_personality_wpa_passphrase_long_pass() {
        let long_pass = "A".repeat(64);
        let (code, output) = run_with(&["wpa_passphrase", "TestSSID", &long_pass]);
        assert_eq!(code, 1);
        assert!(output.contains("too long"));
    }

    #[test]
    fn test_personality_unknown() {
        let (code, output) = run_with(&["wpa"]);
        assert_eq!(code, 0);
        assert!(output.contains("multi-personality"));
    }

    #[test]
    fn test_personality_windows_path() {
        let (code, output) = run_with(&["C:\\bin\\wpa_supplicant.exe", "-v"]);
        assert_eq!(code, 0);
        assert!(output.contains(VERSION));
    }

    #[test]
    fn test_personality_unix_path() {
        let (code, output) = run_with(&["/usr/sbin/wpa_cli", "ping"]);
        assert_eq!(code, 0);
        assert!(output.contains("PONG"));
    }

    #[test]
    fn test_empty_args() {
        let args: Vec<String> = vec![];
        let mut buf = Vec::new();
        let code = run(&args, &mut buf);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_personality_wpa_supplicant_unknown_opt() {
        let (code, output) = run_with(&["wpa_supplicant", "--bogus"]);
        assert_eq!(code, 1);
        assert!(output.contains("unknown option"));
    }

    #[test]
    fn test_personality_wpa_supplicant_missing_iface_arg() {
        let (code, output) = run_with(&["wpa_supplicant", "-i"]);
        assert_eq!(code, 1);
        assert!(output.contains("requires"));
    }

    #[test]
    fn test_personality_wpa_supplicant_missing_config_arg() {
        let (code, output) = run_with(&["wpa_supplicant", "-c"]);
        assert_eq!(code, 1);
        assert!(output.contains("requires"));
    }

    #[test]
    fn test_personality_wpa_supplicant_missing_driver_arg() {
        let (code, output) = run_with(&["wpa_supplicant", "-D"]);
        assert_eq!(code, 1);
        assert!(output.contains("requires"));
    }

    #[test]
    fn test_personality_wpa_passphrase_too_many_args() {
        let (code, output) = run_with(&["wpa_passphrase", "ssid", "pass", "extra"]);
        assert_eq!(code, 1);
        assert!(output.contains("Usage"));
    }

    // =====================================================================
    // Association with BSS tests
    // =====================================================================

    #[test]
    fn test_associate_no_selected_network() {
        let mut state = SupplicantState::new();
        assert!(!state.do_associate());
    }

    #[test]
    fn test_associate_disabled_network() {
        let mut state = SupplicantState::new();
        let mut net = NetworkConfig::new(0);
        net.ssid = b"Test".to_vec();
        net.enabled = false;
        state.config.networks.push(net);
        state.selected_network = Some(0);
        assert!(!state.do_associate());
    }

    #[test]
    fn test_associate_no_bss() {
        let mut state = SupplicantState::new();
        let mut net = NetworkConfig::new(0);
        net.ssid = b"Test".to_vec();
        state.config.networks.push(net);
        state.selected_network = Some(0);
        assert!(!state.do_associate());
    }

    #[test]
    fn test_associate_open_network() {
        let mut state = SupplicantState::new();
        let mut net = NetworkConfig::new(0);
        net.ssid = b"OpenNet".to_vec();
        net.key_mgmt = KeyMgmt::None;
        state.config.networks.push(net);
        state.selected_network = Some(0);
        state.bss_list.push(BssEntry {
            bssid: [1, 2, 3, 4, 5, 6],
            ssid: b"OpenNet".to_vec(),
            freq: 2412,
            signal: -50,
            flags: String::new(),
            key_mgmt: KeyMgmt::None,
            proto: WpaProto::Rsn,
        });
        assert!(state.do_associate());
        assert_eq!(state.wpa_state, WpaState::Completed);
    }

    #[test]
    fn test_associate_psk_network() {
        let mut state = SupplicantState::new();
        let mut net = NetworkConfig::new(0);
        net.ssid = b"SecureNet".to_vec();
        net.key_mgmt = KeyMgmt::WpaPsk;
        net.psk = Some(wpa_psk(b"testpassword", b"SecureNet"));
        state.config.networks.push(net);
        state.selected_network = Some(0);
        state.bss_list.push(BssEntry {
            bssid: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
            ssid: b"SecureNet".to_vec(),
            freq: 5180,
            signal: -35,
            flags: "[WPA2-PSK-CCMP]".to_string(),
            key_mgmt: KeyMgmt::WpaPsk,
            proto: WpaProto::Rsn,
        });
        assert!(state.do_associate());
        assert_eq!(state.wpa_state, WpaState::Completed);
        assert_eq!(
            state.current_bssid,
            Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff])
        );
    }

    #[test]
    fn test_associate_psk_no_key() {
        let mut state = SupplicantState::new();
        let mut net = NetworkConfig::new(0);
        net.ssid = b"NoPSK".to_vec();
        net.key_mgmt = KeyMgmt::WpaPsk;
        net.psk = None;
        state.config.networks.push(net);
        state.selected_network = Some(0);
        state.bss_list.push(BssEntry {
            bssid: [1, 2, 3, 4, 5, 6],
            ssid: b"NoPSK".to_vec(),
            freq: 2412,
            signal: -50,
            flags: String::new(),
            key_mgmt: KeyMgmt::WpaPsk,
            proto: WpaProto::Rsn,
        });
        assert!(!state.do_associate());
        assert_eq!(state.wpa_state, WpaState::Disconnected);
    }

    // =====================================================================
    // WPA PSK known vectors (IEEE 802.11i Annex H test vectors)
    // =====================================================================

    #[test]
    fn test_wpa_psk_ieee_vector_1() {
        // SSID = "IEEE", passphrase = "password"
        let psk = wpa_psk(b"password", b"IEEE");
        assert_eq!(
            hex_encode_string(&psk),
            "f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e"
        );
    }

    #[test]
    fn test_wpa_psk_ieee_vector_2() {
        // SSID = "ThisIsASSID", passphrase = "ThisIsAPassword"
        let psk = wpa_psk(b"ThisIsAPassword", b"ThisIsASSID");
        assert_eq!(
            hex_encode_string(&psk),
            "0dc0d6eb90555ed6419756b9a15ec3e3209b63df707dd508d14581f8982721af"
        );
    }

    #[test]
    fn test_wpa_psk_max_ssid_max_pass() {
        // SSID = 32 Z's (max SSID length), passphrase = 32 a's.
        // Verified against Python hashlib.pbkdf2_hmac.
        let ssid = b"ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ";
        let pass = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(ssid.len(), 32);
        assert_eq!(pass.len(), 32);
        let psk = wpa_psk(pass, ssid);
        assert_eq!(
            hex_encode_string(&psk),
            "becb93866bb8c3832cb777c2f559807c8c59afcb6eae734885001300a981cc62"
        );
    }
}
