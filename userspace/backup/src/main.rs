//! OurOS Backup Utility
//!
//! Snapshot-based backup tool supporting full and incremental backups with
//! SHA-256 integrity verification and manifest tracking.
//!
//! # Backup types
//!
//! - **Full backup**: copies every file from the source directory, creates
//!   a manifest recording each file's path, size, modification time, and
//!   SHA-256 hash.
//!
//! - **Incremental backup**: compares the source against the last full or
//!   incremental backup's manifest. Only copies files that are new or
//!   changed (different size or mtime). The incremental manifest references
//!   the parent backup for unchanged files.
//!
//! # Manifest format
//!
//! ```text
//! # backup-manifest v1
//! # type: full|incremental
//! # parent: <parent-id or "none">
//! # created: 2026-05-17 12:00:00
//! # source: /home/user
//! # files: 1234
//! # bytes: 56789012
//!
//! F <sha256> <size> <mtime> <path>
//! D <mtime> <path>
//! L <target> <path>
//! ```

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// SHA-256 — self-contained implementation (no external crates)
// ============================================================================

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
    0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
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
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.total_len += data.len() as u64;
        let mut offset = 0;

        if self.buf_len > 0 {
            let space = 64 - self.buf_len;
            let copy = data.len().min(space);
            self.buf[self.buf_len..self.buf_len + copy].copy_from_slice(&data[..copy]);
            self.buf_len += copy;
            offset = copy;

            if self.buf_len == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buf_len = 0;
            }
        }

        while offset + 64 <= data.len() {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[offset..offset + 64]);
            self.compress(&block);
            offset += 64;
        }

        if offset < data.len() {
            let remaining = data.len() - offset;
            self.buf[..remaining].copy_from_slice(&data[offset..]);
            self.buf_len = remaining;
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len * 8;
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;

        if self.buf_len > 56 {
            for i in self.buf_len..64 {
                self.buf[i] = 0;
            }
            let block = self.buf;
            self.compress(&block);
            self.buf = [0u8; 64];
            self.buf_len = 0;
        }

        for i in self.buf_len..56 {
            self.buf[i] = 0;
        }
        self.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buf;
        self.compress(&block);

        let mut out = [0u8; 32];
        for (i, &word) in self.state.iter().enumerate() {
            out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
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
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;

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

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

/// Compute SHA-256 hash of a file, returning hex string.
fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    let hash = hasher.finalize();
    Ok(hex_encode(&hash))
}

fn hex_encode(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for &byte in data {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

// ============================================================================
// Timestamp formatting
// ============================================================================

fn format_timestamp(unix_secs: u64) -> String {
    let secs = unix_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let mut y = 1970i64;
    let mut remaining_days = days as i64;

    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }

    let month_days = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md as i64 {
            m = i + 1;
            break;
        }
        remaining_days -= md as i64;
    }
    if m == 0 {
        m = 12;
    }
    let d = remaining_days + 1;

    format!(
        "{y:04}-{m:02}-{:02} {:02}:{:02}:{:02}",
        d, hours, minutes, seconds
    )
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ============================================================================
// Manifest — records what's in a backup
// ============================================================================

const MANIFEST_NAME: &str = "manifest.txt";

/// Type of backup.
#[derive(Clone, Copy, Debug, PartialEq)]
enum BackupType {
    Full,
    Incremental,
}

/// A single entry in the manifest.
#[derive(Clone, Debug)]
enum ManifestEntry {
    File {
        hash: String,
        size: u64,
        mtime: u64,
        path: String,
    },
    Directory {
        mtime: u64,
        path: String,
    },
    Symlink {
        target: String,
        path: String,
    },
}

/// Parsed backup manifest.
#[derive(Clone, Debug)]
struct Manifest {
    backup_type: BackupType,
    parent_id: String,
    created: u64,
    source: String,
    backup_id: String,
    entries: Vec<ManifestEntry>,
}

impl Manifest {
    fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str("# backup-manifest v1\n");
        out.push_str(&format!(
            "# type: {}\n",
            match self.backup_type {
                BackupType::Full => "full",
                BackupType::Incremental => "incremental",
            }
        ));
        out.push_str(&format!("# parent: {}\n", self.parent_id));
        out.push_str(&format!("# created: {}\n", self.created));
        out.push_str(&format!("# source: {}\n", self.source));
        out.push_str(&format!("# id: {}\n", self.backup_id));

        let file_count = self.entries.iter().filter(|e| matches!(e, ManifestEntry::File { .. })).count();
        let total_bytes: u64 = self.entries.iter().filter_map(|e| {
            if let ManifestEntry::File { size, .. } = e { Some(*size) } else { None }
        }).sum();

        out.push_str(&format!("# files: {}\n", file_count));
        out.push_str(&format!("# bytes: {}\n", total_bytes));
        out.push('\n');

        for entry in &self.entries {
            match entry {
                ManifestEntry::File { hash, size, mtime, path } => {
                    out.push_str(&format!("F {hash} {size} {mtime} {path}\n"));
                }
                ManifestEntry::Directory { mtime, path } => {
                    out.push_str(&format!("D {mtime} {path}\n"));
                }
                ManifestEntry::Symlink { target, path } => {
                    out.push_str(&format!("L {target} {path}\n"));
                }
            }
        }

        out
    }

    fn parse(text: &str) -> Result<Self, String> {
        let mut backup_type = BackupType::Full;
        let mut parent_id = String::from("none");
        let mut created = 0u64;
        let mut source = String::new();
        let mut backup_id = String::new();
        let mut entries = Vec::new();
        let mut found_header = false;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with("# backup-manifest") {
                found_header = true;
                continue;
            }

            if trimmed.starts_with('#') {
                if let Some((key, val)) = trimmed.trim_start_matches('#').trim().split_once(':') {
                    let val = val.trim();
                    match key.trim() {
                        "type" => {
                            backup_type = if val == "incremental" {
                                BackupType::Incremental
                            } else {
                                BackupType::Full
                            };
                        }
                        "parent" => parent_id = val.to_string(),
                        "created" => created = val.parse().unwrap_or(0),
                        "source" => source = val.to_string(),
                        "id" => backup_id = val.to_string(),
                        _ => {}
                    }
                }
                continue;
            }

            if !found_header {
                return Err("not a valid backup manifest".to_string());
            }

            // Parse entry lines
            let parts: Vec<&str> = trimmed.splitn(5, ' ').collect();
            match parts.first().copied() {
                Some("F") if parts.len() >= 5 => {
                    entries.push(ManifestEntry::File {
                        hash: parts[1].to_string(),
                        size: parts[2].parse().unwrap_or(0),
                        mtime: parts[3].parse().unwrap_or(0),
                        path: parts[4].to_string(),
                    });
                }
                Some("D") if parts.len() >= 3 => {
                    let path_parts: Vec<&str> = trimmed.splitn(3, ' ').collect();
                    entries.push(ManifestEntry::Directory {
                        mtime: path_parts[1].parse().unwrap_or(0),
                        path: path_parts[2].to_string(),
                    });
                }
                Some("L") if parts.len() >= 3 => {
                    let link_parts: Vec<&str> = trimmed.splitn(3, ' ').collect();
                    entries.push(ManifestEntry::Symlink {
                        target: link_parts[1].to_string(),
                        path: link_parts[2].to_string(),
                    });
                }
                _ => {
                    return Err(format!("invalid manifest line: {trimmed}"));
                }
            }
        }

        if !found_header {
            return Err("missing backup-manifest header".to_string());
        }

        Ok(Manifest {
            backup_type,
            parent_id,
            created,
            source,
            backup_id,
            entries,
        })
    }

    /// Build a lookup map of path → (hash, size, mtime) for quick comparison.
    fn file_index(&self) -> BTreeMap<&str, (&str, u64, u64)> {
        let mut map = BTreeMap::new();
        for entry in &self.entries {
            if let ManifestEntry::File { hash, size, mtime, path } = entry {
                map.insert(path.as_str(), (hash.as_str(), *size, *mtime));
            }
        }
        map
    }
}

// ============================================================================
// Backup operations
// ============================================================================

/// Generate a unique backup ID from timestamp.
fn generate_backup_id() -> String {
    let ts = now_secs();
    format!("backup-{ts}")
}

/// Walk a directory tree, collecting all files/dirs/symlinks with relative paths.
fn walk_source(root: &Path) -> io::Result<Vec<(PathBuf, fs::Metadata)>> {
    let mut result = Vec::new();
    walk_recursive(root, root, &mut result)?;
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

fn walk_recursive(
    root: &Path,
    current: &Path,
    result: &mut Vec<(PathBuf, fs::Metadata)>,
) -> io::Result<()> {
    let entries = match fs::read_dir(current) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("  warning: cannot read {}: {e}", current.display());
            return Ok(());
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("  warning: readdir error: {e}");
                continue;
            }
        };

        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("  warning: stat {}: {e}", path.display());
                continue;
            }
        };

        let rel_path = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        result.push((rel_path, meta.clone()));

        if meta.is_dir() {
            walk_recursive(root, &path, result)?;
        }
    }

    Ok(())
}

/// Perform a full backup.
fn cmd_backup_full(source: &Path, dest: &Path, exclude: &[String]) {
    if !source.is_dir() {
        eprintln!("backup: source is not a directory: {}", source.display());
        process::exit(1);
    }

    let backup_id = generate_backup_id();
    let backup_dir = dest.join(&backup_id);

    if let Err(e) = fs::create_dir_all(&backup_dir) {
        eprintln!("backup: cannot create {}: {e}", backup_dir.display());
        process::exit(1);
    }

    let data_dir = backup_dir.join("data");
    if let Err(e) = fs::create_dir_all(&data_dir) {
        eprintln!("backup: cannot create data dir: {e}");
        process::exit(1);
    }

    println!("Full backup: {} → {}", source.display(), backup_dir.display());
    println!("Scanning source...");

    let tree = match walk_source(source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("backup: scan failed: {e}");
            process::exit(1);
        }
    };

    let mut entries = Vec::new();
    let mut files_copied = 0u64;
    let mut bytes_copied = 0u64;
    let mut errors = 0u64;

    for (rel_path, meta) in &tree {
        let rel_str = rel_path.to_string_lossy().replace('\\', "/");

        // Apply exclusion filters
        if exclude.iter().any(|ex| rel_str.starts_with(ex.as_str()) || rel_str.contains(ex.as_str())) {
            continue;
        }

        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if meta.is_dir() {
            entries.push(ManifestEntry::Directory {
                mtime,
                path: rel_str,
            });
            continue;
        }

        if meta.is_symlink() {
            let target = fs::read_link(source.join(rel_path))
                .map(|t| t.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            entries.push(ManifestEntry::Symlink {
                target,
                path: rel_str,
            });
            continue;
        }

        if meta.is_file() {
            let src_path = source.join(rel_path);
            let hash = match sha256_file(&src_path) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("  error: hash {}: {e}", src_path.display());
                    errors += 1;
                    continue;
                }
            };

            // Copy file to data directory
            let dst_path = data_dir.join(rel_path);
            if let Some(parent) = dst_path.parent() {
                let _ = fs::create_dir_all(parent);
            }

            match fs::copy(&src_path, &dst_path) {
                Ok(n) => {
                    files_copied += 1;
                    bytes_copied += n;
                }
                Err(e) => {
                    eprintln!("  error: copy {}: {e}", rel_str);
                    errors += 1;
                    continue;
                }
            }

            entries.push(ManifestEntry::File {
                hash,
                size: meta.len(),
                mtime,
                path: rel_str,
            });
        }
    }

    // Write manifest
    let manifest = Manifest {
        backup_type: BackupType::Full,
        parent_id: String::from("none"),
        created: now_secs(),
        source: source.to_string_lossy().replace('\\', "/"),
        backup_id: backup_id.clone(),
        entries,
    };

    let manifest_text = manifest.serialize();
    let manifest_path = backup_dir.join(MANIFEST_NAME);

    if let Err(e) = fs::write(&manifest_path, &manifest_text) {
        eprintln!("backup: failed to write manifest: {e}");
        process::exit(1);
    }

    println!("\nBackup complete: {backup_id}");
    println!(
        "  {} file(s) copied, {} total",
        files_copied,
        format_size(bytes_copied)
    );
    if errors > 0 {
        eprintln!("  {errors} error(s) — some files were not backed up");
    }
    println!("  Manifest: {}", manifest_path.display());
}

/// Perform an incremental backup against the most recent backup in dest.
fn cmd_backup_incremental(source: &Path, dest: &Path, exclude: &[String]) {
    if !source.is_dir() {
        eprintln!("backup: source is not a directory: {}", source.display());
        process::exit(1);
    }

    // Find the most recent backup
    let parent_manifest = match find_latest_backup(dest) {
        Some((id, manifest)) => {
            println!("Incremental backup based on: {id}");
            (id, manifest)
        }
        None => {
            println!("No previous backup found — falling back to full backup.");
            cmd_backup_full(source, dest, exclude);
            return;
        }
    };

    let (parent_id, parent) = parent_manifest;
    let parent_index = parent.file_index();

    let backup_id = generate_backup_id();
    let backup_dir = dest.join(&backup_id);

    if let Err(e) = fs::create_dir_all(&backup_dir) {
        eprintln!("backup: cannot create {}: {e}", backup_dir.display());
        process::exit(1);
    }

    let data_dir = backup_dir.join("data");
    if let Err(e) = fs::create_dir_all(&data_dir) {
        eprintln!("backup: cannot create data dir: {e}");
        process::exit(1);
    }

    println!(
        "Incremental backup: {} → {}",
        source.display(),
        backup_dir.display()
    );
    println!("Scanning source...");

    let tree = match walk_source(source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("backup: scan failed: {e}");
            process::exit(1);
        }
    };

    let mut entries = Vec::new();
    let mut files_copied = 0u64;
    let mut files_unchanged = 0u64;
    let mut bytes_copied = 0u64;
    let mut errors = 0u64;

    for (rel_path, meta) in &tree {
        let rel_str = rel_path.to_string_lossy().replace('\\', "/");

        if exclude.iter().any(|ex| rel_str.starts_with(ex.as_str()) || rel_str.contains(ex.as_str())) {
            continue;
        }

        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if meta.is_dir() {
            entries.push(ManifestEntry::Directory {
                mtime,
                path: rel_str,
            });
            continue;
        }

        if meta.is_symlink() {
            let target = fs::read_link(source.join(rel_path))
                .map(|t| t.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            entries.push(ManifestEntry::Symlink {
                target,
                path: rel_str,
            });
            continue;
        }

        if meta.is_file() {
            // Check against parent manifest: skip if size and mtime match
            let needs_copy = match parent_index.get(rel_str.as_str()) {
                Some((_old_hash, old_size, old_mtime)) => {
                    meta.len() != *old_size || mtime != *old_mtime
                }
                None => true, // New file
            };

            let src_path = source.join(rel_path);

            if needs_copy {
                let hash = match sha256_file(&src_path) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("  error: hash {}: {e}", src_path.display());
                        errors += 1;
                        continue;
                    }
                };

                // Check if hash actually changed (size/mtime might differ but content same)
                let content_changed = match parent_index.get(rel_str.as_str()) {
                    Some((old_hash, _, _)) => *old_hash != hash.as_str(),
                    None => true,
                };

                if content_changed {
                    // Copy file to data directory
                    let dst_path = data_dir.join(rel_path);
                    if let Some(parent) = dst_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }

                    match fs::copy(&src_path, &dst_path) {
                        Ok(n) => {
                            files_copied += 1;
                            bytes_copied += n;
                        }
                        Err(e) => {
                            eprintln!("  error: copy {}: {e}", rel_str);
                            errors += 1;
                            continue;
                        }
                    }
                } else {
                    files_unchanged += 1;
                }

                entries.push(ManifestEntry::File {
                    hash,
                    size: meta.len(),
                    mtime,
                    path: rel_str,
                });
            } else {
                // Unchanged — carry forward from parent manifest
                files_unchanged += 1;
                if let Some((old_hash, old_size, old_mtime)) = parent_index.get(rel_str.as_str()) {
                    entries.push(ManifestEntry::File {
                        hash: old_hash.to_string(),
                        size: *old_size,
                        mtime: *old_mtime,
                        path: rel_str,
                    });
                }
            }
        }
    }

    // Write manifest
    let manifest = Manifest {
        backup_type: BackupType::Incremental,
        parent_id: parent_id.clone(),
        created: now_secs(),
        source: source.to_string_lossy().replace('\\', "/"),
        backup_id: backup_id.clone(),
        entries,
    };

    let manifest_text = manifest.serialize();
    let manifest_path = backup_dir.join(MANIFEST_NAME);

    if let Err(e) = fs::write(&manifest_path, &manifest_text) {
        eprintln!("backup: failed to write manifest: {e}");
        process::exit(1);
    }

    println!("\nIncremental backup complete: {backup_id}");
    println!(
        "  {} file(s) copied ({}), {} unchanged",
        files_copied,
        format_size(bytes_copied),
        files_unchanged
    );
    if errors > 0 {
        eprintln!("  {errors} error(s) — some files were not backed up");
    }
    println!("  Based on: {parent_id}");
}

/// Find the most recent backup in the destination directory.
fn find_latest_backup(dest: &Path) -> Option<(String, Manifest)> {
    if !dest.is_dir() {
        return None;
    }

    let mut backups: Vec<(String, u64)> = Vec::new();

    if let Ok(entries) = fs::read_dir(dest) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let manifest_path = path.join(MANIFEST_NAME);
                if manifest_path.exists() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    // Extract timestamp from backup-NNNNNN format
                    if let Some(ts_str) = name.strip_prefix("backup-")
                        && let Ok(ts) = ts_str.parse::<u64>() {
                            backups.push((name, ts));
                        }
                }
            }
        }
    }

    backups.sort_by_key(|b| std::cmp::Reverse(b.1)); // Most recent first

    for (name, _) in &backups {
        let manifest_path = dest.join(name).join(MANIFEST_NAME);
        if let Ok(text) = fs::read_to_string(&manifest_path)
            && let Ok(manifest) = Manifest::parse(&text) {
                return Some((name.clone(), manifest));
            }
    }

    None
}

/// Restore files from a backup.
fn cmd_restore(backup_path: &Path, dest: &Path, files_filter: &[String]) {
    let manifest_path = backup_path.join(MANIFEST_NAME);
    let manifest_text = match fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("backup: cannot read manifest: {e}");
            process::exit(1);
        }
    };

    let manifest = match Manifest::parse(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("backup: {e}");
            process::exit(1);
        }
    };

    let data_dir = backup_path.join("data");

    println!("Restoring from {} to {}", manifest.backup_id, dest.display());

    let mut restored = 0u64;
    let mut bytes_restored = 0u64;
    let mut errors = 0u64;
    let mut dirs_created = 0u64;

    for entry in &manifest.entries {
        match entry {
            ManifestEntry::Directory { path, .. } => {
                if !files_filter.is_empty()
                    && !files_filter.iter().any(|f| path.starts_with(f.as_str()))
                {
                    continue;
                }

                let dst = dest.join(path);
                if !dst.exists() {
                    if let Err(e) = fs::create_dir_all(&dst) {
                        eprintln!("  error: mkdir {}: {e}", dst.display());
                        errors += 1;
                    } else {
                        dirs_created += 1;
                    }
                }
            }
            ManifestEntry::File { path, .. } => {
                if !files_filter.is_empty()
                    && !files_filter.iter().any(|f| path.starts_with(f.as_str()))
                {
                    continue;
                }

                let src = data_dir.join(path);
                let dst = dest.join(path);

                if let Some(parent) = dst.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                if src.exists() {
                    match fs::copy(&src, &dst) {
                        Ok(n) => {
                            restored += 1;
                            bytes_restored += n;
                        }
                        Err(e) => {
                            eprintln!("  error: restore {}: {e}", path);
                            errors += 1;
                        }
                    }
                } else if manifest.backup_type == BackupType::Incremental {
                    // For incremental backups, unchanged files aren't in data_dir.
                    // Need to find them in the parent backup.
                    eprintln!(
                        "  warning: {} not in this backup (unchanged from parent)",
                        path
                    );
                    eprintln!(
                        "           Restore the parent backup ({}) first for full restore.",
                        manifest.parent_id
                    );
                } else {
                    eprintln!("  error: {} missing from backup data", path);
                    errors += 1;
                }
            }
            ManifestEntry::Symlink { target: _, path } => {
                if !files_filter.is_empty()
                    && !files_filter.iter().any(|f| path.starts_with(f.as_str()))
                {
                    continue;
                }

                let dst = dest.join(path);
                if let Some(parent) = dst.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                // Remove existing file/link before creating symlink
                let _ = fs::remove_file(&dst);

                #[cfg(unix)]
                {
                    use std::os::unix::fs::symlink;
                    if let Err(e) = symlink(target, &dst) {
                        eprintln!("  error: symlink {}: {e}", path);
                        errors += 1;
                    }
                }
                #[cfg(not(unix))]
                {
                    eprintln!("  warning: symlinks not supported on this platform: {path}");
                }
            }
        }
    }

    println!("\nRestore complete.");
    println!(
        "  {} file(s) restored ({}), {} dir(s) created",
        restored,
        format_size(bytes_restored),
        dirs_created
    );
    if errors > 0 {
        eprintln!("  {errors} error(s) during restore");
    }
}

/// Verify backup integrity by checking SHA-256 hashes.
fn cmd_verify(backup_path: &Path) {
    let manifest_path = backup_path.join(MANIFEST_NAME);
    let manifest_text = match fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("backup: cannot read manifest: {e}");
            process::exit(1);
        }
    };

    let manifest = match Manifest::parse(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("backup: {e}");
            process::exit(1);
        }
    };

    let data_dir = backup_path.join("data");

    println!("Verifying backup: {}", manifest.backup_id);
    println!(
        "  Type: {}",
        if manifest.backup_type == BackupType::Full {
            "full"
        } else {
            "incremental"
        }
    );

    let mut ok = 0u64;
    let mut missing = 0u64;
    let mut corrupted = 0u64;
    let mut skipped = 0u64;

    for entry in &manifest.entries {
        if let ManifestEntry::File { hash, path, .. } = entry {
            let file_path = data_dir.join(path);

            if !file_path.exists() {
                if manifest.backup_type == BackupType::Incremental {
                    // Unchanged files aren't stored in incremental backups
                    skipped += 1;
                } else {
                    eprintln!("  MISSING: {path}");
                    missing += 1;
                }
                continue;
            }

            match sha256_file(&file_path) {
                Ok(computed) => {
                    if computed == *hash {
                        ok += 1;
                    } else {
                        eprintln!("  CORRUPT: {path}");
                        eprintln!("    expected: {hash}");
                        eprintln!("    actual:   {computed}");
                        corrupted += 1;
                    }
                }
                Err(e) => {
                    eprintln!("  ERROR: {path}: {e}");
                    corrupted += 1;
                }
            }
        }
    }

    println!("\nVerification complete.");
    println!("  {ok} file(s) OK");
    if skipped > 0 {
        println!("  {skipped} file(s) skipped (unchanged from parent)");
    }
    if missing > 0 {
        eprintln!("  {missing} file(s) MISSING");
    }
    if corrupted > 0 {
        eprintln!("  {corrupted} file(s) CORRUPTED");
    }

    if missing == 0 && corrupted == 0 {
        println!("  All files verified successfully.");
    } else {
        process::exit(1);
    }
}

/// List backups in the destination directory.
fn cmd_list(dest: &Path) {
    if !dest.is_dir() {
        eprintln!("backup: not a directory: {}", dest.display());
        process::exit(1);
    }

    let mut backups: Vec<(String, Manifest)> = Vec::new();

    if let Ok(entries) = fs::read_dir(dest) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let manifest_path = path.join(MANIFEST_NAME);
                if let Ok(text) = fs::read_to_string(&manifest_path)
                    && let Ok(manifest) = Manifest::parse(&text) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        backups.push((name, manifest));
                    }
            }
        }
    }

    if backups.is_empty() {
        println!("No backups found in {}", dest.display());
        return;
    }

    // Sort by creation time
    backups.sort_by_key(|a| a.1.created);

    println!(
        "{:<24} {:<12} {:<20} {:<8} {:<12} SOURCE",
        "BACKUP ID", "TYPE", "CREATED", "FILES", "SIZE"
    );

    for (name, manifest) in &backups {
        let type_str = if manifest.backup_type == BackupType::Full {
            "full"
        } else {
            "incremental"
        };

        let file_count = manifest.entries.iter().filter(|e| matches!(e, ManifestEntry::File { .. })).count();
        let total_bytes: u64 = manifest.entries.iter().filter_map(|e| {
            if let ManifestEntry::File { size, .. } = e { Some(*size) } else { None }
        }).sum();

        println!(
            "{:<24} {:<12} {:<20} {:<8} {:<12} {}",
            name,
            type_str,
            format_timestamp(manifest.created),
            file_count,
            format_size(total_bytes),
            manifest.source,
        );
    }

    println!("\n{} backup(s) total.", backups.len());
}

/// Show detailed contents of a specific backup.
fn cmd_show(backup_path: &Path) {
    let manifest_path = backup_path.join(MANIFEST_NAME);
    let manifest_text = match fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("backup: cannot read manifest: {e}");
            process::exit(1);
        }
    };

    let manifest = match Manifest::parse(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("backup: {e}");
            process::exit(1);
        }
    };

    let type_str = if manifest.backup_type == BackupType::Full {
        "full"
    } else {
        "incremental"
    };

    let file_count = manifest.entries.iter().filter(|e| matches!(e, ManifestEntry::File { .. })).count();
    let dir_count = manifest.entries.iter().filter(|e| matches!(e, ManifestEntry::Directory { .. })).count();
    let link_count = manifest.entries.iter().filter(|e| matches!(e, ManifestEntry::Symlink { .. })).count();
    let total_bytes: u64 = manifest.entries.iter().filter_map(|e| {
        if let ManifestEntry::File { size, .. } = e { Some(*size) } else { None }
    }).sum();

    println!("Backup: {}", manifest.backup_id);
    println!("  Type:    {type_str}");
    println!("  Created: {}", format_timestamp(manifest.created));
    println!("  Source:  {}", manifest.source);
    println!("  Parent:  {}", manifest.parent_id);
    println!(
        "  Content: {} file(s), {} dir(s), {} symlink(s)",
        file_count, dir_count, link_count
    );
    println!("  Size:    {}", format_size(total_bytes));

    println!("\nFiles:");
    for entry in &manifest.entries {
        match entry {
            ManifestEntry::File { size, path, .. } => {
                println!("  F {:>10}  {path}", format_size(*size));
            }
            ManifestEntry::Directory { path, .. } => {
                println!("  D            {path}/");
            }
            ManifestEntry::Symlink { target, path } => {
                println!("  L            {path} -> {target}");
            }
        }
    }
}

/// Delete a specific backup.
fn cmd_delete(backup_path: &Path) {
    let manifest_path = backup_path.join(MANIFEST_NAME);
    if !manifest_path.exists() {
        eprintln!(
            "backup: not a backup directory: {}",
            backup_path.display()
        );
        process::exit(1);
    }

    match fs::remove_dir_all(backup_path) {
        Ok(()) => {
            println!("Deleted backup: {}", backup_path.display());
        }
        Err(e) => {
            eprintln!("backup: failed to delete: {e}");
            process::exit(1);
        }
    }
}

// ============================================================================
// Main
// ============================================================================

fn print_usage() {
    println!(
        "backup — OurOS backup utility

Usage: backup <command> [options]

Commands:
  full <source> <dest>        Create a full backup of source directory
  incr <source> <dest>        Create an incremental backup (based on latest)
  restore <backup> <dest>     Restore files from a backup
  verify <backup>             Verify backup integrity (SHA-256 check)
  list <dest>                 List all backups in destination directory
  show <backup>               Show detailed contents of a backup
  delete <backup>             Delete a backup
  help                        Show this help

Options:
  --exclude <pattern>         Exclude paths matching pattern (repeatable)

Examples:
  backup full /home/user /mnt/backup/user
  backup incr /home/user /mnt/backup/user
  backup list /mnt/backup/user
  backup verify /mnt/backup/user/backup-1716000000
  backup restore /mnt/backup/user/backup-1716000000 /home/user

Full vs Incremental:
  A full backup copies every file and records SHA-256 hashes in a manifest.
  An incremental backup compares against the most recent backup and only
  copies files with different size or modification time. Both types create
  a complete manifest — incremental just stores fewer files on disk.

Restore:
  restore from a full backup gives you everything. Restore from an
  incremental backup gives you only the changed files — restore the
  parent (full) backup first for a complete restore."
    );
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        print_usage();
        process::exit(0);
    }

    // Parse --exclude flags
    let mut exclude: Vec<String> = Vec::new();
    let mut filtered_args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--exclude"
            && let Some(pattern) = args.get(i + 1) {
                exclude.push(pattern.clone());
                i += 2;
                continue;
            }
        filtered_args.push(args[i].clone());
        i += 1;
    }

    let command = filtered_args[0].as_str();
    let rest = &filtered_args[1..];

    match command {
        "full" => {
            if rest.len() < 2 {
                eprintln!("Usage: backup full <source> <dest>");
                process::exit(1);
            }
            cmd_backup_full(Path::new(&rest[0]), Path::new(&rest[1]), &exclude);
        }
        "incr" | "incremental" => {
            if rest.len() < 2 {
                eprintln!("Usage: backup incr <source> <dest>");
                process::exit(1);
            }
            cmd_backup_incremental(Path::new(&rest[0]), Path::new(&rest[1]), &exclude);
        }
        "restore" => {
            if rest.len() < 2 {
                eprintln!("Usage: backup restore <backup-dir> <dest>");
                process::exit(1);
            }
            let files_filter: Vec<String> = rest[2..].to_vec();
            cmd_restore(Path::new(&rest[0]), Path::new(&rest[1]), &files_filter);
        }
        "verify" => {
            if rest.is_empty() {
                eprintln!("Usage: backup verify <backup-dir>");
                process::exit(1);
            }
            cmd_verify(Path::new(&rest[0]));
        }
        "list" | "ls" => {
            if rest.is_empty() {
                eprintln!("Usage: backup list <dest-dir>");
                process::exit(1);
            }
            cmd_list(Path::new(&rest[0]));
        }
        "show" | "info" => {
            if rest.is_empty() {
                eprintln!("Usage: backup show <backup-dir>");
                process::exit(1);
            }
            cmd_show(Path::new(&rest[0]));
        }
        "delete" | "rm" => {
            if rest.is_empty() {
                eprintln!("Usage: backup delete <backup-dir>");
                process::exit(1);
            }
            cmd_delete(Path::new(&rest[0]));
        }
        "help" | "--help" | "-h" => print_usage(),
        _ => {
            eprintln!("backup: unknown command: {command}");
            eprintln!("Run 'backup help' for usage.");
            process::exit(1);
        }
    }
}
