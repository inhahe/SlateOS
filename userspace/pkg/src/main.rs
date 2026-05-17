//! pkg — OurOS package manager.
//!
//! Content-addressed package store with atomic generational updates.
//!
//! Usage:
//!   pkg install <PACKAGE>...     Install packages
//!   pkg remove <PACKAGE>...      Remove packages
//!   pkg update                   Refresh repository metadata
//!   pkg upgrade [PACKAGE...]     Upgrade packages (all if none specified)
//!   pkg list [--installed]       List packages
//!   pkg search <QUERY>           Search available packages
//!   pkg info <PACKAGE>           Show package details
//!   pkg rollback [GENERATION]    Rollback to a previous generation
//!   pkg generations              List system generations
//!   pkg gc [--keep N]            Garbage-collect old generations
//!   pkg verify [PACKAGE...]      Verify installed package integrity
//!   pkg files <PACKAGE>          List files owned by a package
//!   pkg which <PATH>             Show which package owns a file
//!
//! Packages are stored content-addressed (SHA-256). System state changes
//! are atomic — each install/remove/upgrade creates a new generation that
//! can be rolled back to if something breaks.

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Configuration
// ============================================================================

/// Root directory for all pkg state.
const PKG_ROOT: &str = "/var/pkg";

/// Where content-addressed blobs live.
const CAS_DIR: &str = "/var/pkg/cas";

/// Where generation metadata lives.
const GEN_DIR: &str = "/var/pkg/generations";

/// Where repository metadata is cached.
const REPO_DIR: &str = "/var/pkg/repos";

/// Default repository URL (placeholder — will point to actual repo server).
const DEFAULT_REPO: &str = "https://repo.ouros.org/stable";

/// Config file location (used when network fetching is implemented).
#[allow(dead_code)]
const CONFIG_PATH: &str = "/etc/pkg.conf";

// ============================================================================
// SHA-256 (inline, no external crate)
// ============================================================================

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_len: u64,
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
                0x1f83d9ab, 0x5be0cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        let mut offset = 0;
        self.total_len += data.len() as u64;

        if self.buffer_len > 0 {
            let space = 64 - self.buffer_len;
            let copy = space.min(data.len());
            self.buffer[self.buffer_len..self.buffer_len + copy].copy_from_slice(&data[..copy]);
            self.buffer_len += copy;
            offset = copy;

            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
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
            self.buffer[..remaining].copy_from_slice(&data[offset..]);
            self.buffer_len = remaining;
        }
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

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len * 8;
        let mut padding = vec![0x80u8];
        let pad_len = (55 - self.buffer_len as isize).rem_euclid(64) as usize;
        padding.extend(vec![0u8; pad_len]);
        padding.extend_from_slice(&bit_len.to_be_bytes());
        self.update(&padding);

        let mut hash = [0u8; 32];
        for (i, &word) in self.state.iter().enumerate() {
            hash[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        hash
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

// ============================================================================
// Version parsing and comparison
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
struct Version {
    major: u32,
    minor: u32,
    patch: u32,
    pre: String, // pre-release suffix (empty = release)
}

impl Version {
    fn parse(s: &str) -> Option<Self> {
        let (version_part, pre) = if let Some((v, p)) = s.split_once('-') {
            (v, p.to_string())
        } else {
            (s, String::new())
        };

        let parts: Vec<&str> = version_part.split('.').collect();
        let major = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    fn satisfies(&self, constraint: &VersionConstraint) -> bool {
        match constraint {
            VersionConstraint::Any => true,
            VersionConstraint::Exact(v) => self == v,
            VersionConstraint::Gte(v) => self >= v,
            VersionConstraint::Gt(v) => self > v,
            VersionConstraint::Lte(v) => self <= v,
            VersionConstraint::Lt(v) => self < v,
            VersionConstraint::Compatible(v) => {
                // ^version: same major, >= minor.patch
                self.major == v.major && self >= v
            }
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| {
                // Pre-release versions sort before release
                match (self.pre.is_empty(), other.pre.is_empty()) {
                    (true, true) => std::cmp::Ordering::Equal,
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    (false, false) => self.pre.cmp(&other.pre),
                }
            })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
enum VersionConstraint {
    Any,
    Exact(Version),
    Gte(Version),
    Gt(Version),
    Lte(Version),
    Lt(Version),
    Compatible(Version), // ^version
}

impl VersionConstraint {
    fn parse(s: &str) -> Self {
        let s = s.trim();
        if s == "*" || s.is_empty() {
            return Self::Any;
        }
        if let Some(rest) = s.strip_prefix(">=") {
            return Version::parse(rest.trim()).map_or(Self::Any, Self::Gte);
        }
        if let Some(rest) = s.strip_prefix("<=") {
            return Version::parse(rest.trim()).map_or(Self::Any, Self::Lte);
        }
        if let Some(rest) = s.strip_prefix('>') {
            return Version::parse(rest.trim()).map_or(Self::Any, Self::Gt);
        }
        if let Some(rest) = s.strip_prefix('<') {
            return Version::parse(rest.trim()).map_or(Self::Any, Self::Lt);
        }
        if let Some(rest) = s.strip_prefix('^') {
            return Version::parse(rest.trim()).map_or(Self::Any, Self::Compatible);
        }
        if let Some(rest) = s.strip_prefix("==") {
            return Version::parse(rest.trim()).map_or(Self::Any, Self::Exact);
        }
        // Bare version = exact match
        Version::parse(s).map_or(Self::Any, Self::Exact)
    }
}

impl fmt::Display for VersionConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => write!(f, "*"),
            Self::Exact(v) => write!(f, "=={v}"),
            Self::Gte(v) => write!(f, ">={v}"),
            Self::Gt(v) => write!(f, ">{v}"),
            Self::Lte(v) => write!(f, "<={v}"),
            Self::Lt(v) => write!(f, "<{v}"),
            Self::Compatible(v) => write!(f, "^{v}"),
        }
    }
}

// ============================================================================
// Package manifest
// ============================================================================

/// Dependency of a package.
#[derive(Clone, Debug)]
struct Dependency {
    name: String,
    constraint: VersionConstraint,
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.name, self.constraint)
    }
}

/// A file entry in a package.
#[derive(Clone, Debug)]
struct PackageFile {
    /// Path within the package archive.
    src: String,
    /// Destination path on the filesystem.
    dst: String,
    /// File mode (octal).
    mode: u32,
    /// SHA-256 hash of the file content.
    hash: String,
    /// File size in bytes.
    size: u64,
}

/// A capability that the package requests.
#[derive(Clone, Debug)]
struct Capability {
    name: String,
    #[allow(dead_code)] // Used in future capability-checking UI
    description: String,
}

/// Package manifest — describes a package's metadata, dependencies, files,
/// and required capabilities.
#[derive(Clone, Debug)]
struct PackageManifest {
    name: String,
    version: Version,
    description: String,
    license: String,
    authors: Vec<String>,
    homepage: String,
    depends: Vec<Dependency>,
    provides: Vec<String>,       // paths this package provides
    capabilities: Vec<Capability>, // capabilities this package needs
    files: Vec<PackageFile>,
    /// SHA-256 of the entire package archive.
    archive_hash: String,
    /// Size of the package archive.
    archive_size: u64,
}

impl PackageManifest {
    /// Parse a manifest from our simple key-value format.
    /// Format (one-per-line, indented continuations):
    /// ```
    /// name: mypackage
    /// version: 1.2.3
    /// description: A useful package
    /// license: MIT
    /// depends: libc >= 0.1.0, libcrypto >= 1.0.0
    /// provides: /usr/bin/mycommand, /usr/lib/libmything.so
    /// capabilities: net.connect, fs.home
    /// file: bin/cmd -> /usr/bin/cmd 0755 <sha256> <size>
    /// archive_hash: <sha256>
    /// archive_size: <size>
    /// ```
    fn parse(text: &str) -> Option<Self> {
        let mut name = String::new();
        let mut version = Version::parse("0.0.0")?;
        let mut description = String::new();
        let mut license = String::new();
        let mut authors = Vec::new();
        let mut homepage = String::new();
        let mut depends = Vec::new();
        let mut provides = Vec::new();
        let mut capabilities = Vec::new();
        let mut files = Vec::new();
        let mut archive_hash = String::new();
        let mut archive_size = 0u64;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let (key, value) = match line.split_once(':') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => continue,
            };

            match key {
                "name" => name = value.to_string(),
                "version" => {
                    if let Some(v) = Version::parse(value) {
                        version = v;
                    }
                }
                "description" => description = value.to_string(),
                "license" => license = value.to_string(),
                "author" | "authors" => {
                    authors = value.split(',').map(|s| s.trim().to_string()).collect();
                }
                "homepage" => homepage = value.to_string(),
                "depends" => {
                    for dep_str in value.split(',') {
                        let dep_str = dep_str.trim();
                        if dep_str.is_empty() {
                            continue;
                        }
                        let parts: Vec<&str> = dep_str.splitn(2, ' ').collect();
                        let dep_name = parts[0].to_string();
                        let constraint = if parts.len() > 1 {
                            VersionConstraint::parse(parts[1])
                        } else {
                            VersionConstraint::Any
                        };
                        depends.push(Dependency {
                            name: dep_name,
                            constraint,
                        });
                    }
                }
                "provides" => {
                    provides = value.split(',').map(|s| s.trim().to_string()).collect();
                }
                "capabilities" => {
                    capabilities = value
                        .split(',')
                        .map(|s| Capability {
                            name: s.trim().to_string(),
                            description: String::new(),
                        })
                        .collect();
                }
                "file" => {
                    // file: src -> dst mode hash size
                    if let Some(pf) = parse_file_entry(value) {
                        files.push(pf);
                    }
                }
                "archive_hash" => archive_hash = value.to_string(),
                "archive_size" => archive_size = value.parse().unwrap_or(0),
                _ => {} // ignore unknown keys for forward compat
            }
        }

        if name.is_empty() {
            return None;
        }

        Some(Self {
            name,
            version,
            description,
            license,
            authors,
            homepage,
            depends,
            provides,
            capabilities,
            files,
            archive_hash,
            archive_size,
        })
    }

    /// Serialize manifest to our text format.
    fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("name: {}\n", self.name));
        out.push_str(&format!("version: {}\n", self.version));
        if !self.description.is_empty() {
            out.push_str(&format!("description: {}\n", self.description));
        }
        if !self.license.is_empty() {
            out.push_str(&format!("license: {}\n", self.license));
        }
        if !self.authors.is_empty() {
            out.push_str(&format!("authors: {}\n", self.authors.join(", ")));
        }
        if !self.homepage.is_empty() {
            out.push_str(&format!("homepage: {}\n", self.homepage));
        }
        if !self.depends.is_empty() {
            let deps: Vec<String> = self.depends.iter().map(|d| d.to_string()).collect();
            out.push_str(&format!("depends: {}\n", deps.join(", ")));
        }
        if !self.provides.is_empty() {
            out.push_str(&format!("provides: {}\n", self.provides.join(", ")));
        }
        if !self.capabilities.is_empty() {
            let caps: Vec<&str> = self.capabilities.iter().map(|c| c.name.as_str()).collect();
            out.push_str(&format!("capabilities: {}\n", caps.join(", ")));
        }
        for file in &self.files {
            out.push_str(&format!(
                "file: {} -> {} {:o} {} {}\n",
                file.src, file.dst, file.mode, file.hash, file.size
            ));
        }
        if !self.archive_hash.is_empty() {
            out.push_str(&format!("archive_hash: {}\n", self.archive_hash));
        }
        if self.archive_size > 0 {
            out.push_str(&format!("archive_size: {}\n", self.archive_size));
        }
        out
    }
}

fn parse_file_entry(value: &str) -> Option<PackageFile> {
    // src -> dst mode hash size
    let (src, rest) = value.split_once("->")?;
    let src = src.trim().to_string();
    let parts: Vec<&str> = rest.trim().split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    let dst = parts[0].to_string();
    let mode = u32::from_str_radix(parts[1], 8).unwrap_or(0o644);
    let hash = parts.get(2).unwrap_or(&"").to_string();
    let size = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    Some(PackageFile {
        src,
        dst,
        mode,
        hash,
        size,
    })
}

// ============================================================================
// Content-Addressed Store
// ============================================================================

struct ContentStore {
    root: PathBuf,
}

impl ContentStore {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn ensure_dirs(&self) {
        let _ = fs::create_dir_all(&self.root);
    }

    /// Store data and return its SHA-256 hash.
    fn put(&self, data: &[u8]) -> io::Result<String> {
        let hash = sha256_hex(data);
        let path = self.blob_path(&hash);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, data)?;
        }
        Ok(hash)
    }

    /// Retrieve data by hash.
    fn get(&self, hash: &str) -> io::Result<Vec<u8>> {
        fs::read(self.blob_path(hash))
    }

    /// Check if a blob exists.
    fn has(&self, hash: &str) -> bool {
        self.blob_path(hash).exists()
    }

    /// Remove a blob.
    fn remove(&self, hash: &str) -> io::Result<()> {
        let path = self.blob_path(hash);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Path for a blob. Uses first 2 chars as directory prefix for
    /// filesystem efficiency (like Git's object store).
    fn blob_path(&self, hash: &str) -> PathBuf {
        let (prefix, rest) = hash.split_at(2.min(hash.len()));
        self.root.join(prefix).join(rest)
    }

    /// List all blob hashes.
    fn list_blobs(&self) -> io::Result<Vec<String>> {
        let mut hashes = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let prefix = entry.file_name().to_string_lossy().to_string();
                if prefix.len() != 2 {
                    continue;
                }
                if let Ok(sub_entries) = fs::read_dir(entry.path()) {
                    for sub in sub_entries.flatten() {
                        let rest = sub.file_name().to_string_lossy().to_string();
                        hashes.push(format!("{prefix}{rest}"));
                    }
                }
            }
        }
        Ok(hashes)
    }

    /// Verify a blob's integrity.
    fn verify(&self, hash: &str) -> io::Result<bool> {
        let data = self.get(hash)?;
        Ok(sha256_hex(&data) == hash)
    }

    /// Total size of all blobs (used by gc reporting).
    #[allow(dead_code)]
    fn total_size(&self) -> io::Result<u64> {
        let mut total = 0u64;
        for hash in self.list_blobs()? {
            if let Ok(meta) = fs::metadata(self.blob_path(&hash)) {
                total += meta.len();
            }
        }
        Ok(total)
    }
}

// ============================================================================
// Generation — atomic system state snapshot
// ============================================================================

/// A generation represents an atomic snapshot of installed packages.
/// Each install/remove/upgrade creates a new generation.
#[derive(Clone, Debug)]
struct Generation {
    /// Monotonically increasing generation ID.
    id: u64,
    /// Unix timestamp when this generation was created.
    timestamp: u64,
    /// Description of what changed.
    description: String,
    /// Map of package name → installed version info.
    packages: BTreeMap<String, InstalledPackage>,
    /// ID of the previous generation (0 if first).
    parent: u64,
}

/// Info about an installed package within a generation.
#[derive(Clone, Debug)]
struct InstalledPackage {
    version: Version,
    /// Hash of the manifest in CAS.
    manifest_hash: String,
    /// Hashes of all installed files in CAS.
    file_hashes: Vec<(String, String)>, // (dst_path, content_hash)
    /// When this package was installed.
    installed_at: u64,
    /// Whether this was explicitly installed (vs pulled as dependency).
    explicit: bool,
}

impl Generation {
    fn new(id: u64, description: &str, parent: u64) -> Self {
        Self {
            id,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            description: description.to_string(),
            packages: BTreeMap::new(),
            parent,
        }
    }

    /// Serialize generation to text format for storage.
    fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("generation: {}\n", self.id));
        out.push_str(&format!("timestamp: {}\n", self.timestamp));
        out.push_str(&format!("parent: {}\n", self.parent));
        out.push_str(&format!("description: {}\n", self.description));
        out.push_str("---\n");

        for (name, pkg) in &self.packages {
            out.push_str(&format!("package: {}\n", name));
            out.push_str(&format!("  version: {}\n", pkg.version));
            out.push_str(&format!("  manifest: {}\n", pkg.manifest_hash));
            out.push_str(&format!("  installed_at: {}\n", pkg.installed_at));
            out.push_str(&format!(
                "  explicit: {}\n",
                if pkg.explicit { "yes" } else { "no" }
            ));
            for (path, hash) in &pkg.file_hashes {
                out.push_str(&format!("  file: {} {}\n", path, hash));
            }
        }

        out
    }

    /// Parse generation from text format.
    fn parse(text: &str) -> Option<Self> {
        let mut id = 0u64;
        let mut timestamp = 0u64;
        let mut parent = 0u64;
        let mut description = String::new();
        let mut packages = BTreeMap::new();

        let mut current_pkg: Option<String> = None;
        let mut current_version = Version::parse("0.0.0")?;
        let mut current_manifest = String::new();
        let mut current_installed_at = 0u64;
        let mut current_explicit = false;
        let mut current_files: Vec<(String, String)> = Vec::new();

        let mut in_header = true;

        for line in text.lines() {
            let line_trimmed = line.trim();
            if line_trimmed.is_empty() {
                continue;
            }

            if line_trimmed == "---" {
                in_header = false;
                continue;
            }

            if in_header {
                if let Some((key, val)) = line_trimmed.split_once(':') {
                    let val = val.trim();
                    match key.trim() {
                        "generation" => id = val.parse().unwrap_or(0),
                        "timestamp" => timestamp = val.parse().unwrap_or(0),
                        "parent" => parent = val.parse().unwrap_or(0),
                        "description" => description = val.to_string(),
                        _ => {}
                    }
                }
            } else if line_trimmed.starts_with("package:") {
                // Save previous package if any
                if let Some(ref pkg_name) = current_pkg {
                    packages.insert(
                        pkg_name.clone(),
                        InstalledPackage {
                            version: current_version.clone(),
                            manifest_hash: current_manifest.clone(),
                            file_hashes: current_files.clone(),
                            installed_at: current_installed_at,
                            explicit: current_explicit,
                        },
                    );
                }
                current_pkg = Some(
                    line_trimmed
                        .strip_prefix("package:")
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                );
                current_version = Version::parse("0.0.0")?;
                current_manifest.clear();
                current_installed_at = 0;
                current_explicit = false;
                current_files.clear();
            } else if let Some((key, val)) = line_trimmed.split_once(':') {
                let val = val.trim();
                match key.trim() {
                    "version" => {
                        if let Some(v) = Version::parse(val) {
                            current_version = v;
                        }
                    }
                    "manifest" => current_manifest = val.to_string(),
                    "installed_at" => current_installed_at = val.parse().unwrap_or(0),
                    "explicit" => current_explicit = val == "yes",
                    "file" => {
                        if let Some((path, hash)) = val.split_once(' ') {
                            current_files
                                .push((path.trim().to_string(), hash.trim().to_string()));
                        }
                    }
                    _ => {}
                }
            }
        }

        // Save last package
        if let Some(ref pkg_name) = current_pkg {
            packages.insert(
                pkg_name.clone(),
                InstalledPackage {
                    version: current_version,
                    manifest_hash: current_manifest,
                    file_hashes: current_files,
                    installed_at: current_installed_at,
                    explicit: current_explicit,
                },
            );
        }

        Some(Self {
            id,
            timestamp,
            description,
            packages,
            parent,
        })
    }
}

// ============================================================================
// Package database — manages generations and the CAS
// ============================================================================

struct PackageDb {
    cas: ContentStore,
    gen_dir: PathBuf,
    repo_dir: PathBuf,
}

impl PackageDb {
    fn new() -> Self {
        let cas = ContentStore::new(Path::new(CAS_DIR));
        Self {
            cas,
            gen_dir: PathBuf::from(GEN_DIR),
            repo_dir: PathBuf::from(REPO_DIR),
        }
    }

    fn ensure_dirs(&self) {
        let _ = fs::create_dir_all(PKG_ROOT);
        self.cas.ensure_dirs();
        let _ = fs::create_dir_all(&self.gen_dir);
        let _ = fs::create_dir_all(&self.repo_dir);
    }

    /// Get the current (latest) generation ID.
    fn current_generation_id(&self) -> u64 {
        let current_path = self.gen_dir.join("current");
        if let Ok(content) = fs::read_to_string(&current_path) {
            content.trim().parse().unwrap_or(0)
        } else {
            0
        }
    }

    /// Set the current generation pointer.
    fn set_current_generation(&self, id: u64) -> io::Result<()> {
        fs::write(self.gen_dir.join("current"), id.to_string())
    }

    /// Load a generation by ID.
    fn load_generation(&self, id: u64) -> io::Result<Generation> {
        let path = self.gen_dir.join(format!("{id}.gen"));
        let text = fs::read_to_string(&path)?;
        Generation::parse(&text).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid generation file")
        })
    }

    /// Save a generation.
    fn save_generation(&self, generation: &Generation) -> io::Result<()> {
        let path = self.gen_dir.join(format!("{}.gen", generation.id));
        fs::write(&path, generation.serialize())
    }

    /// Get the current generation (or create generation 0 if none exists).
    fn current_generation(&self) -> Generation {
        let id = self.current_generation_id();
        if id == 0 {
            Generation::new(0, "initial empty state", 0)
        } else {
            self.load_generation(id)
                .unwrap_or_else(|_| Generation::new(0, "initial empty state", 0))
        }
    }

    /// Create a new generation based on the current one.
    fn next_generation(&self, description: &str) -> Generation {
        let current = self.current_generation();
        let next_id = current.id + 1;
        let mut next = Generation::new(next_id, description, current.id);
        next.packages = current.packages.clone();
        next
    }

    /// List all generation IDs.
    fn list_generations(&self) -> io::Result<Vec<u64>> {
        let mut ids = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.gen_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(id_str) = name.strip_suffix(".gen") {
                    if let Ok(id) = id_str.parse::<u64>() {
                        ids.push(id);
                    }
                }
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Load available packages from cached repository metadata.
    fn load_repo_index(&self) -> io::Result<Vec<PackageManifest>> {
        let index_path = self.repo_dir.join("index");
        if !index_path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&index_path)?;
        let mut packages = Vec::new();

        // Index format: packages separated by blank lines
        for chunk in text.split("\n\n") {
            let chunk = chunk.trim();
            if chunk.is_empty() {
                continue;
            }
            if let Some(manifest) = PackageManifest::parse(chunk) {
                packages.push(manifest);
            }
        }

        Ok(packages)
    }

    /// Find a package in the repo index by name.
    fn find_in_repo(&self, name: &str) -> io::Result<Option<PackageManifest>> {
        let packages = self.load_repo_index()?;
        Ok(packages.into_iter().find(|p| p.name == name))
    }

    /// Search repo index by query (substring match on name or description).
    fn search_repo(&self, query: &str) -> io::Result<Vec<PackageManifest>> {
        let packages = self.load_repo_index()?;
        let query_lower = query.to_lowercase();
        Ok(packages
            .into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&query_lower)
                    || p.description.to_lowercase().contains(&query_lower)
            })
            .collect())
    }

    /// Collect all content hashes referenced by a generation.
    fn generation_hashes(&self, generation: &Generation) -> HashSet<String> {
        let mut hashes = HashSet::new();
        for pkg in generation.packages.values() {
            hashes.insert(pkg.manifest_hash.clone());
            for (_, hash) in &pkg.file_hashes {
                hashes.insert(hash.clone());
            }
        }
        hashes
    }
}

// ============================================================================
// Dependency resolver
// ============================================================================

/// Simple dependency resolver using backtracking.
/// For a full OS package manager, this would need SAT solving (like libsolv),
/// but for our initial implementation a greedy approach is sufficient.
struct Resolver<'a> {
    _db: &'a PackageDb,
    available: Vec<PackageManifest>,
}

impl<'a> Resolver<'a> {
    fn new(db: &'a PackageDb) -> io::Result<Self> {
        let available = db.load_repo_index()?;
        Ok(Self { _db: db, available })
    }

    /// Resolve dependencies for installing a set of packages.
    /// Returns the full list of packages to install (including dependencies),
    /// in topological order.
    fn resolve(
        &self,
        requested: &[String],
        current: &Generation,
    ) -> Result<Vec<PackageManifest>, String> {
        let mut to_install: Vec<PackageManifest> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut resolving: HashSet<String> = HashSet::new(); // cycle detection

        for name in requested {
            self.resolve_one(name, current, &mut to_install, &mut visited, &mut resolving)?;
        }

        Ok(to_install)
    }

    fn resolve_one(
        &self,
        name: &str,
        current: &Generation,
        result: &mut Vec<PackageManifest>,
        visited: &mut HashSet<String>,
        resolving: &mut HashSet<String>,
    ) -> Result<(), String> {
        if visited.contains(name) {
            return Ok(());
        }

        if resolving.contains(name) {
            return Err(format!("circular dependency detected: {name}"));
        }

        resolving.insert(name.to_string());

        // Already installed and satisfies constraints? Skip.
        if current.packages.contains_key(name) {
            visited.insert(name.to_string());
            resolving.remove(name);
            return Ok(());
        }

        // Find in available packages
        let manifest = self
            .available
            .iter()
            .filter(|p| p.name == name)
            .max_by(|a, b| a.version.cmp(&b.version))
            .ok_or_else(|| format!("package not found: {name}"))?
            .clone();

        // Resolve dependencies first
        for dep in &manifest.depends {
            // Check if already installed and satisfies constraint
            if let Some(installed) = current.packages.get(&dep.name) {
                if installed.version.satisfies(&dep.constraint) {
                    continue;
                }
                return Err(format!(
                    "dependency conflict: {} requires {} {}, but {} is installed",
                    name, dep.name, dep.constraint, installed.version
                ));
            }
            self.resolve_one(&dep.name, current, result, visited, resolving)?;
        }

        visited.insert(name.to_string());
        resolving.remove(name);
        result.push(manifest);

        Ok(())
    }
}

// ============================================================================
// CLI commands
// ============================================================================

fn cmd_install(db: &PackageDb, packages: &[String], dry_run: bool) {
    if packages.is_empty() {
        eprintln!("pkg: no packages specified");
        process::exit(1);
    }

    db.ensure_dirs();

    let current = db.current_generation();

    // Check for already-installed packages
    let mut to_install = Vec::new();
    for name in packages {
        if let Some(existing) = current.packages.get(name) {
            println!("{name} {}: already installed", existing.version);
        } else {
            to_install.push(name.clone());
        }
    }

    if to_install.is_empty() {
        println!("Nothing to do.");
        return;
    }

    // Resolve dependencies
    let resolver = match Resolver::new(db) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("pkg: failed to load repository index: {e}");
            eprintln!("pkg: try 'pkg update' first");
            process::exit(1);
        }
    };

    let resolved = match resolver.resolve(&to_install, &current) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("pkg: dependency resolution failed: {e}");
            process::exit(1);
        }
    };

    if resolved.is_empty() {
        println!("Nothing to install.");
        return;
    }

    // Show what will be installed
    println!("The following packages will be installed:");
    let mut total_size = 0u64;
    for manifest in &resolved {
        println!("  {} {}", manifest.name, manifest.version);
        total_size += manifest.archive_size;
    }
    println!(
        "\n{} package(s), {} total download size.",
        resolved.len(),
        format_size(total_size)
    );

    // Show capabilities
    let mut has_caps = false;
    for manifest in &resolved {
        if !manifest.capabilities.is_empty() {
            if !has_caps {
                println!("\nCapabilities requested:");
                has_caps = true;
            }
            let caps: Vec<&str> = manifest.capabilities.iter().map(|c| c.name.as_str()).collect();
            println!("  {}: {}", manifest.name, caps.join(", "));
        }
    }

    if dry_run {
        println!("\n(dry run — no changes made)");
        return;
    }

    // Create new generation
    let desc = format!(
        "install {}",
        to_install
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut new_gen = db.next_generation(&desc);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for manifest in &resolved {
        // In a real implementation, this would:
        // 1. Download the archive from the repository
        // 2. Verify archive_hash
        // 3. Extract files, storing each in the CAS
        // 4. Install files to their destinations using a filesystem transaction
        //
        // For now, store the manifest in CAS and record the installation.
        let manifest_data = manifest.serialize();
        let manifest_hash = match db.cas.put(manifest_data.as_bytes()) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("pkg: failed to store manifest for {}: {e}", manifest.name);
                continue;
            }
        };

        let file_hashes: Vec<(String, String)> = manifest
            .files
            .iter()
            .map(|f| (f.dst.clone(), f.hash.clone()))
            .collect();

        let is_explicit = packages.contains(&manifest.name);

        new_gen.packages.insert(
            manifest.name.clone(),
            InstalledPackage {
                version: manifest.version.clone(),
                manifest_hash,
                file_hashes,
                installed_at: now,
                explicit: is_explicit,
            },
        );

        println!("  Installing {} {}...", manifest.name, manifest.version);
    }

    // Commit generation atomically
    match db.save_generation(&new_gen) {
        Ok(()) => {
            if let Err(e) = db.set_current_generation(new_gen.id) {
                eprintln!("pkg: CRITICAL — saved generation but failed to update pointer: {e}");
                eprintln!("pkg: manually run: echo {} > {}/current", new_gen.id, GEN_DIR);
                process::exit(1);
            }
            println!(
                "\nDone. Generation {} created.",
                new_gen.id
            );
        }
        Err(e) => {
            eprintln!("pkg: failed to save generation: {e}");
            process::exit(1);
        }
    }
}

fn cmd_remove(db: &PackageDb, packages: &[String], dry_run: bool) {
    if packages.is_empty() {
        eprintln!("pkg: no packages specified");
        process::exit(1);
    }

    let current = db.current_generation();

    // Check all packages exist
    for name in packages {
        if !current.packages.contains_key(name) {
            eprintln!("pkg: {name}: not installed");
            process::exit(1);
        }
    }

    // Check for reverse dependencies
    let repo_index = db.load_repo_index().unwrap_or_default();
    let removing: HashSet<&str> = packages.iter().map(|s| s.as_str()).collect();

    for (name, _pkg) in &current.packages {
        if removing.contains(name.as_str()) {
            continue;
        }
        // Find this package's manifest to check its deps
        if let Some(manifest) = repo_index.iter().find(|m| m.name == *name) {
            for dep in &manifest.depends {
                if removing.contains(dep.name.as_str()) {
                    eprintln!(
                        "pkg: cannot remove {}: required by {name}",
                        dep.name
                    );
                    process::exit(1);
                }
            }
        }
    }

    println!("The following packages will be removed:");
    for name in packages {
        if let Some(pkg) = current.packages.get(name) {
            println!("  {name} {}", pkg.version);
        }
    }

    if dry_run {
        println!("\n(dry run — no changes made)");
        return;
    }

    let desc = format!(
        "remove {}",
        packages
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut new_gen = db.next_generation(&desc);

    for name in packages {
        new_gen.packages.remove(name);
        println!("  Removing {name}...");
    }

    match db.save_generation(&new_gen) {
        Ok(()) => {
            let _ = db.set_current_generation(new_gen.id);
            println!("\nDone. Generation {} created.", new_gen.id);
        }
        Err(e) => {
            eprintln!("pkg: failed to save generation: {e}");
            process::exit(1);
        }
    }
}

fn cmd_list(db: &PackageDb, installed_only: bool) {
    let current = db.current_generation();

    if installed_only || current.packages.is_empty() {
        if current.packages.is_empty() {
            println!("No packages installed.");
            return;
        }
        println!(
            "{:<30} {:<15} {:<10}",
            "PACKAGE", "VERSION", "STATUS"
        );
        for (name, pkg) in &current.packages {
            let status = if pkg.explicit { "explicit" } else { "auto" };
            println!("{:<30} {:<15} {:<10}", name, pkg.version, status);
        }
        println!("\n{} package(s) installed.", current.packages.len());
    } else {
        // Show available too
        let available = db.load_repo_index().unwrap_or_default();
        println!(
            "{:<30} {:<15} {:<15} {:<10}",
            "PACKAGE", "INSTALLED", "AVAILABLE", "STATUS"
        );
        let mut shown: HashSet<String> = HashSet::new();
        for (name, pkg) in &current.packages {
            let avail_ver = available
                .iter()
                .filter(|p| p.name == *name)
                .max_by(|a, b| a.version.cmp(&b.version))
                .map(|p| p.version.to_string())
                .unwrap_or_else(|| "-".to_string());
            let status = if pkg.explicit { "explicit" } else { "auto" };
            println!(
                "{:<30} {:<15} {:<15} {:<10}",
                name, pkg.version, avail_ver, status
            );
            shown.insert(name.clone());
        }
        for pkg in &available {
            if !shown.contains(&pkg.name) {
                println!(
                    "{:<30} {:<15} {:<15} {:<10}",
                    pkg.name, "-", pkg.version, "available"
                );
            }
        }
    }
}

fn cmd_search(db: &PackageDb, query: &str) {
    match db.search_repo(query) {
        Ok(results) => {
            if results.is_empty() {
                println!("No packages found matching '{query}'.");
                return;
            }
            let current = db.current_generation();
            for pkg in &results {
                let status = if current.packages.contains_key(&pkg.name) {
                    "[installed]"
                } else {
                    ""
                };
                println!(
                    "{} {} {status}",
                    pkg.name, pkg.version
                );
                if !pkg.description.is_empty() {
                    println!("  {}", pkg.description);
                }
            }
        }
        Err(e) => {
            eprintln!("pkg: search failed: {e}");
            process::exit(1);
        }
    }
}

fn cmd_info(db: &PackageDb, name: &str) {
    let current = db.current_generation();

    // Check installed first
    if let Some(pkg) = current.packages.get(name) {
        println!("Name:      {name}");
        println!("Version:   {}", pkg.version);
        println!("Status:    installed");
        println!(
            "Type:      {}",
            if pkg.explicit {
                "explicitly installed"
            } else {
                "auto-installed (dependency)"
            }
        );
        println!("Files:     {}", pkg.file_hashes.len());
        println!("Manifest:  {}", pkg.manifest_hash);

        // Try to get full manifest from CAS for more details
        if let Ok(data) = db.cas.get(&pkg.manifest_hash) {
            if let Ok(text) = String::from_utf8(data) {
                if let Some(manifest) = PackageManifest::parse(&text) {
                    if !manifest.description.is_empty() {
                        println!("Desc:      {}", manifest.description);
                    }
                    if !manifest.license.is_empty() {
                        println!("License:   {}", manifest.license);
                    }
                    if !manifest.depends.is_empty() {
                        println!("Depends:");
                        for dep in &manifest.depends {
                            println!("  {dep}");
                        }
                    }
                    if !manifest.capabilities.is_empty() {
                        println!("Capabilities:");
                        for cap in &manifest.capabilities {
                            println!("  {}", cap.name);
                        }
                    }
                }
            }
        }
        return;
    }

    // Check repo
    match db.find_in_repo(name) {
        Ok(Some(manifest)) => {
            println!("Name:      {}", manifest.name);
            println!("Version:   {}", manifest.version);
            println!("Status:    available (not installed)");
            if !manifest.description.is_empty() {
                println!("Desc:      {}", manifest.description);
            }
            if !manifest.license.is_empty() {
                println!("License:   {}", manifest.license);
            }
            if !manifest.authors.is_empty() {
                println!("Authors:   {}", manifest.authors.join(", "));
            }
            if !manifest.homepage.is_empty() {
                println!("Homepage:  {}", manifest.homepage);
            }
            if !manifest.depends.is_empty() {
                println!("Depends:");
                for dep in &manifest.depends {
                    println!("  {dep}");
                }
            }
            if !manifest.capabilities.is_empty() {
                println!("Capabilities:");
                for cap in &manifest.capabilities {
                    println!("  {}", cap.name);
                }
            }
            if manifest.archive_size > 0 {
                println!("Size:      {}", format_size(manifest.archive_size));
            }
        }
        Ok(None) => {
            eprintln!("pkg: {name}: not found");
            process::exit(1);
        }
        Err(e) => {
            eprintln!("pkg: {e}");
            process::exit(1);
        }
    }
}

fn cmd_generations(db: &PackageDb) {
    let current_id = db.current_generation_id();
    match db.list_generations() {
        Ok(ids) => {
            if ids.is_empty() {
                println!("No generations recorded.");
                return;
            }
            println!(
                "{:<6} {:<20} {:<8} {}",
                "GEN", "DATE", "PKGS", "DESCRIPTION"
            );
            for id in &ids {
                if let Ok(g) = db.load_generation(*id) {
                    let marker = if *id == current_id { " *" } else { "" };
                    let date = format_timestamp(g.timestamp);
                    println!(
                        "{:<6} {:<20} {:<8} {}{}",
                        g.id,
                        date,
                        g.packages.len(),
                        g.description,
                        marker
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("pkg: failed to list generations: {e}");
            process::exit(1);
        }
    }
}

fn cmd_rollback(db: &PackageDb, target_gen: Option<u64>) {
    let current_id = db.current_generation_id();

    let target = match target_gen {
        Some(id) => id,
        None => {
            // Rollback to parent
            let current = db.current_generation();
            if current.parent == 0 && current.id == 0 {
                eprintln!("pkg: no previous generation to rollback to");
                process::exit(1);
            }
            current.parent
        }
    };

    // Verify target generation exists
    if db.load_generation(target).is_err() {
        eprintln!("pkg: generation {target} not found");
        process::exit(1);
    }

    let target_gen = db.load_generation(target).expect("just checked");
    let current_gen = db.current_generation();

    // Show diff
    println!("Rolling back from generation {current_id} to {target}:");

    let current_pkgs: HashSet<&str> = current_gen.packages.keys().map(|s| s.as_str()).collect();
    let target_pkgs: HashSet<&str> = target_gen.packages.keys().map(|s| s.as_str()).collect();

    for name in current_pkgs.difference(&target_pkgs) {
        println!("  - {name} (will be removed)");
    }
    for name in target_pkgs.difference(&current_pkgs) {
        println!("  + {name} (will be restored)");
    }
    for name in current_pkgs.intersection(&target_pkgs) {
        let cv = &current_gen.packages[*name].version;
        let tv = &target_gen.packages[*name].version;
        if cv != tv {
            println!("  ~ {name} ({cv} → {tv})");
        }
    }

    // Create a new generation that represents the rollback
    // (we don't delete generations — the history is append-only)
    let desc = format!("rollback to generation {target}");
    let mut rollback_gen = db.next_generation(&desc);
    rollback_gen.packages = target_gen.packages.clone();

    match db.save_generation(&rollback_gen) {
        Ok(()) => {
            let _ = db.set_current_generation(rollback_gen.id);
            println!(
                "\nRolled back. Generation {} created (based on {target}).",
                rollback_gen.id
            );
        }
        Err(e) => {
            eprintln!("pkg: failed to save rollback generation: {e}");
            process::exit(1);
        }
    }
}

fn cmd_gc(db: &PackageDb, keep: usize) {
    let current_id = db.current_generation_id();
    let all_ids = match db.list_generations() {
        Ok(ids) => ids,
        Err(e) => {
            eprintln!("pkg: {e}");
            process::exit(1);
        }
    };

    if all_ids.len() <= keep {
        println!("Nothing to garbage-collect ({} generation(s), keeping {keep}).", all_ids.len());
        return;
    }

    // Collect all hashes referenced by generations we're keeping
    let mut keep_hashes: HashSet<String> = HashSet::new();
    let mut remove_ids = Vec::new();

    let keep_ids: HashSet<u64> = all_ids
        .iter()
        .rev()
        .take(keep)
        .copied()
        .collect();

    // Always keep current
    let mut keep_ids = keep_ids;
    keep_ids.insert(current_id);

    for &id in &all_ids {
        if keep_ids.contains(&id) {
            if let Ok(g) = db.load_generation(id) {
                keep_hashes.extend(db.generation_hashes(&g));
            }
        } else {
            remove_ids.push(id);
        }
    }

    if remove_ids.is_empty() {
        println!("Nothing to garbage-collect.");
        return;
    }

    println!(
        "Removing {} old generation(s)...",
        remove_ids.len()
    );

    // Remove generation files
    for id in &remove_ids {
        let path = db.gen_dir.join(format!("{id}.gen"));
        if let Err(e) = fs::remove_file(&path) {
            eprintln!("  warning: failed to remove {}.gen: {e}", id);
        }
    }

    // Remove orphaned blobs (not referenced by any kept generation)
    let mut orphaned = 0;
    let mut freed = 0u64;
    if let Ok(all_blobs) = db.cas.list_blobs() {
        for hash in &all_blobs {
            if !keep_hashes.contains(hash) {
                if let Ok(meta) = fs::metadata(db.cas.blob_path(hash)) {
                    freed += meta.len();
                }
                let _ = db.cas.remove(hash);
                orphaned += 1;
            }
        }
    }

    println!(
        "Removed {} generation(s), {} orphaned blob(s) ({} freed).",
        remove_ids.len(),
        orphaned,
        format_size(freed)
    );
}

fn cmd_verify(db: &PackageDb, packages: &[String]) {
    let current = db.current_generation();

    let to_verify: Vec<(&String, &InstalledPackage)> = if packages.is_empty() {
        current.packages.iter().collect()
    } else {
        let mut v = Vec::new();
        for name in packages {
            match current.packages.get(name) {
                Some(pkg) => v.push((name, pkg)),
                None => {
                    eprintln!("pkg: {name}: not installed");
                    process::exit(1);
                }
            }
        }
        v
    };

    let mut ok = 0;
    let mut bad = 0;

    for (name, pkg) in &to_verify {
        // Verify manifest blob
        let manifest_ok = db.cas.has(&pkg.manifest_hash)
            && db.cas.verify(&pkg.manifest_hash).unwrap_or(false);

        if !manifest_ok {
            println!("CORRUPT: {name} — manifest blob missing or corrupted");
            bad += 1;
            continue;
        }

        // Verify each file
        let mut pkg_ok = true;
        for (path, hash) in &pkg.file_hashes {
            if hash.is_empty() {
                continue;
            }
            if !db.cas.has(hash) {
                println!("MISSING: {name}: {path} — blob {hash} not in store");
                pkg_ok = false;
            } else if !db.cas.verify(hash).unwrap_or(false) {
                println!("CORRUPT: {name}: {path} — blob {hash} hash mismatch");
                pkg_ok = false;
            }
        }

        if pkg_ok {
            println!("OK: {name} {}", pkg.version);
            ok += 1;
        } else {
            bad += 1;
        }
    }

    println!("\n{ok} OK, {bad} problems.");
    if bad > 0 {
        process::exit(1);
    }
}

fn cmd_files(db: &PackageDb, name: &str) {
    let current = db.current_generation();
    match current.packages.get(name) {
        Some(pkg) => {
            for (path, _hash) in &pkg.file_hashes {
                println!("{path}");
            }
        }
        None => {
            eprintln!("pkg: {name}: not installed");
            process::exit(1);
        }
    }
}

fn cmd_which(db: &PackageDb, path: &str) {
    let current = db.current_generation();
    for (name, pkg) in &current.packages {
        for (file_path, _) in &pkg.file_hashes {
            if file_path == path {
                println!("{name} {}", pkg.version);
                return;
            }
        }
    }
    eprintln!("pkg: {path}: not owned by any package");
    process::exit(1);
}

fn cmd_upgrade(db: &PackageDb, packages: &[String], dry_run: bool) {
    let current = db.current_generation();
    let available = match db.load_repo_index() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("pkg: failed to load repo index: {e}");
            eprintln!("pkg: try 'pkg update' first");
            process::exit(1);
        }
    };

    let to_check: Vec<&String> = if packages.is_empty() {
        current.packages.keys().collect()
    } else {
        let mut v = Vec::new();
        for name in packages {
            if !current.packages.contains_key(name) {
                eprintln!("pkg: {name}: not installed");
                process::exit(1);
            }
            v.push(name);
        }
        v
    };

    let mut upgrades: Vec<(&str, &Version, &Version)> = Vec::new();
    for name in &to_check {
        let installed = &current.packages[name.as_str()];
        if let Some(latest) = available
            .iter()
            .filter(|p| p.name == **name)
            .max_by(|a, b| a.version.cmp(&b.version))
        {
            if latest.version > installed.version {
                upgrades.push((name, &installed.version, &latest.version));
            }
        }
    }

    if upgrades.is_empty() {
        println!("All packages are up to date.");
        return;
    }

    println!("The following packages will be upgraded:");
    for (name, old, new) in &upgrades {
        println!("  {name}: {old} → {new}");
    }

    if dry_run {
        println!("\n(dry run — no changes made)");
        return;
    }

    // Install the newer versions
    let names: Vec<String> = upgrades.iter().map(|(n, _, _)| n.to_string()).collect();
    let desc = format!("upgrade {}", names.join(", "));
    let mut new_gen = db.next_generation(&desc);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for (name, _, new_ver) in &upgrades {
        if let Some(manifest) = available
            .iter()
            .find(|p| p.name == *name && &p.version == *new_ver)
        {
            let manifest_data = manifest.serialize();
            let manifest_hash = match db.cas.put(manifest_data.as_bytes()) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("pkg: failed to store manifest for {name}: {e}");
                    continue;
                }
            };

            let old_explicit = new_gen
                .packages
                .get(*name)
                .map_or(true, |p| p.explicit);

            let file_hashes: Vec<(String, String)> = manifest
                .files
                .iter()
                .map(|f| (f.dst.clone(), f.hash.clone()))
                .collect();

            new_gen.packages.insert(
                name.to_string(),
                InstalledPackage {
                    version: manifest.version.clone(),
                    manifest_hash,
                    file_hashes,
                    installed_at: now,
                    explicit: old_explicit,
                },
            );

            println!("  Upgrading {name} to {}...", manifest.version);
        }
    }

    match db.save_generation(&new_gen) {
        Ok(()) => {
            let _ = db.set_current_generation(new_gen.id);
            println!("\nDone. Generation {} created.", new_gen.id);
        }
        Err(e) => {
            eprintln!("pkg: failed to save generation: {e}");
            process::exit(1);
        }
    }
}

fn cmd_update(_db: &PackageDb) {
    // In a real implementation, this would:
    // 1. Read repository URLs from /etc/pkg.conf
    // 2. Download index files from each repo
    // 3. Verify signatures
    // 4. Store in REPO_DIR
    //
    // For now, just check if the index exists and report status.
    let index_path = Path::new(REPO_DIR).join("index");
    if index_path.exists() {
        if let Ok(meta) = fs::metadata(&index_path) {
            println!(
                "Repository index: {} ({} entries)",
                format_size(meta.len()),
                count_index_entries(&index_path)
            );
        }
    } else {
        println!("No repository index found.");
        println!("Repository URL: {DEFAULT_REPO}");
        println!("(Network fetching not yet implemented — place index file manually at {REPO_DIR}/index)");
    }
    println!("\nTo populate the index, create {} with package manifests", REPO_DIR);
    println!("separated by blank lines. See 'pkg help' for manifest format.");
}

fn count_index_entries(path: &Path) -> usize {
    fs::read_to_string(path)
        .unwrap_or_default()
        .split("\n\n")
        .filter(|chunk| !chunk.trim().is_empty())
        .count()
}

// ============================================================================
// Utility functions
// ============================================================================

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

fn format_timestamp(unix_secs: u64) -> String {
    // Simple timestamp formatting without chrono
    // Returns YYYY-MM-DD HH:MM:SS
    let secs = unix_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Days since epoch to date (simplified)
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

    let months_days: [i64; 12] = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0;
    for (i, &md) in months_days.iter().enumerate() {
        if remaining_days < md {
            m = i + 1;
            break;
        }
        remaining_days -= md;
    }
    if m == 0 {
        m = 12;
    }

    let d = remaining_days + 1;
    format!("{y:04}-{m:02}-{d:02} {hours:02}:{minutes:02}:{seconds:02}")
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage() {
    println!(
        "pkg — OurOS package manager

Usage: pkg <command> [options] [arguments]

Commands:
  install <pkg>...       Install packages
  remove <pkg>...        Remove packages
  update                 Refresh repository metadata
  upgrade [pkg...]       Upgrade packages (all if none specified)
  list [--installed]     List packages
  search <query>         Search available packages
  info <pkg>             Show package details
  rollback [gen]         Rollback to a previous generation
  generations            List system generations
  gc [--keep N]          Garbage-collect old generations (default: keep 5)
  verify [pkg...]        Verify installed package integrity
  files <pkg>            List files owned by a package
  which <path>           Show which package owns a file
  help                   Show this help

Options:
  --dry-run              Show what would be done without making changes

Generation System:
  Each install/remove/upgrade creates a new generation. Generations
  are atomic snapshots of the system's package state. Rollback to any
  previous generation if something breaks.

Content-Addressed Store:
  All package files are stored by their SHA-256 hash, enabling
  deduplication and integrity verification. The store lives at
  {CAS_DIR}.

Package Manifest Format:
  name: mypackage
  version: 1.2.3
  description: A useful package
  license: MIT
  depends: libc >= 0.1.0, libcrypto >= 1.0.0
  provides: /usr/bin/mycommand
  capabilities: net.connect, fs.home
  file: bin/cmd -> /usr/bin/cmd 755 <sha256> <size>
  archive_hash: <sha256>
  archive_size: <bytes>"
    );
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        print_usage();
        process::exit(0);
    }

    let db = PackageDb::new();
    let command = args[0].as_str();
    let rest: Vec<String> = args[1..].to_vec();

    // Check for --dry-run flag
    let dry_run = rest.iter().any(|a| a == "--dry-run");
    let rest_filtered: Vec<String> = rest
        .iter()
        .filter(|a| a.as_str() != "--dry-run")
        .cloned()
        .collect();

    match command {
        "install" => cmd_install(&db, &rest_filtered, dry_run),
        "remove" | "uninstall" => cmd_remove(&db, &rest_filtered, dry_run),
        "update" => cmd_update(&db),
        "upgrade" => cmd_upgrade(&db, &rest_filtered, dry_run),
        "list" | "ls" => {
            let installed_only = rest.iter().any(|a| a == "--installed" || a == "-i");
            cmd_list(&db, installed_only);
        }
        "search" => {
            if rest_filtered.is_empty() {
                eprintln!("pkg: search requires a query");
                process::exit(1);
            }
            cmd_search(&db, &rest_filtered.join(" "));
        }
        "info" | "show" => {
            if rest_filtered.is_empty() {
                eprintln!("pkg: info requires a package name");
                process::exit(1);
            }
            cmd_info(&db, &rest_filtered[0]);
        }
        "generations" | "gen" => cmd_generations(&db),
        "rollback" => {
            let target = rest_filtered.first().and_then(|s| s.parse().ok());
            cmd_rollback(&db, target);
        }
        "gc" => {
            let mut keep = 5usize;
            let mut i = 0;
            while i < rest_filtered.len() {
                if rest_filtered[i] == "--keep" {
                    if let Some(n) = rest_filtered.get(i + 1).and_then(|s| s.parse().ok()) {
                        keep = n;
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            cmd_gc(&db, keep);
        }
        "verify" => cmd_verify(&db, &rest_filtered),
        "files" => {
            if rest_filtered.is_empty() {
                eprintln!("pkg: files requires a package name");
                process::exit(1);
            }
            cmd_files(&db, &rest_filtered[0]);
        }
        "which" | "owns" => {
            if rest_filtered.is_empty() {
                eprintln!("pkg: which requires a file path");
                process::exit(1);
            }
            cmd_which(&db, &rest_filtered[0]);
        }
        "help" | "--help" | "-h" => print_usage(),
        _ => {
            eprintln!("pkg: unknown command: {command}");
            eprintln!("Run 'pkg help' for usage.");
            process::exit(1);
        }
    }
}
