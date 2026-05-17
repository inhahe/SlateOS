//! pkg — OurOS package manager.
//!
//! Content-addressed package store with atomic generational updates.
//!
//! Usage:
//!   pkg install <PACKAGE>...     Install packages (repo names or local .pkg files)
//!   pkg remove <PACKAGE>...      Remove packages
//!   pkg update                   Refresh repository metadata
//!   pkg upgrade [PACKAGE...]     Upgrade packages (all if none specified)
//!   pkg fetch <PACKAGE>...       Download packages to CAS without installing
//!   pkg pack <MANIFEST>          Create a .pkg archive from source files
//!   pkg repo [subcommand]       Manage package repositories
//!   pkg log [N|--all]           Show transaction history
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

use httpclient::Client as HttpClient;

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

/// Config file location.
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
    /// Paths that are config files (protected from overwrite during upgrade).
    conffiles: Vec<String>,
    /// Lifecycle hooks.
    hook_pre_install: String,
    hook_post_install: String,
    hook_pre_remove: String,
    hook_post_remove: String,
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
        let mut conffiles = Vec::new();
        let mut hook_pre_install = String::new();
        let mut hook_post_install = String::new();
        let mut hook_pre_remove = String::new();
        let mut hook_post_remove = String::new();

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
                "conffiles" => {
                    conffiles = value.split(',').map(|s| s.trim().to_string()).collect();
                }
                "file" => {
                    // file: src -> dst mode hash size
                    if let Some(pf) = parse_file_entry(value) {
                        files.push(pf);
                    }
                }
                "archive_hash" => archive_hash = value.to_string(),
                "archive_size" => archive_size = value.parse().unwrap_or(0),
                "hook-pre-install" => hook_pre_install = value.to_string(),
                "hook-post-install" => hook_post_install = value.to_string(),
                "hook-pre-remove" => hook_pre_remove = value.to_string(),
                "hook-post-remove" => hook_post_remove = value.to_string(),
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
            conffiles,
            hook_pre_install,
            hook_post_install,
            hook_pre_remove,
            hook_post_remove,
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
        if !self.conffiles.is_empty() {
            out.push_str(&format!("conffiles: {}\n", self.conffiles.join(", ")));
        }
        if !self.archive_hash.is_empty() {
            out.push_str(&format!("archive_hash: {}\n", self.archive_hash));
        }
        if self.archive_size > 0 {
            out.push_str(&format!("archive_size: {}\n", self.archive_size));
        }
        if !self.hook_pre_install.is_empty() {
            out.push_str(&format!("hook-pre-install: {}\n", self.hook_pre_install));
        }
        if !self.hook_post_install.is_empty() {
            out.push_str(&format!("hook-post-install: {}\n", self.hook_post_install));
        }
        if !self.hook_pre_remove.is_empty() {
            out.push_str(&format!("hook-pre-remove: {}\n", self.hook_pre_remove));
        }
        if !self.hook_post_remove.is_empty() {
            out.push_str(&format!("hook-post-remove: {}\n", self.hook_post_remove));
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
// Repository configuration
// ============================================================================

/// A configured package repository.
#[derive(Clone, Debug)]
struct RepoConfig {
    /// Short name for the repository (e.g. "stable", "community").
    name: String,
    /// Base URL for the repository (e.g. "https://repo.ouros.org/stable").
    url: String,
    /// Priority: lower numbers are preferred. Official repos use 100, user
    /// repos default to 500.
    priority: u32,
    /// Whether this repository is enabled.
    enabled: bool,
}

/// Complete package manager configuration loaded from /etc/pkg.conf.
struct PkgConfig {
    repos: Vec<RepoConfig>,
}

impl PkgConfig {
    /// Load configuration from the config file, falling back to the default
    /// single repository if the file doesn't exist or can't be parsed.
    fn load() -> Self {
        if let Ok(text) = fs::read_to_string(CONFIG_PATH) {
            if let Some(config) = Self::parse(&text) {
                return config;
            }
            eprintln!("pkg: warning: failed to parse {CONFIG_PATH}, using defaults");
        }
        Self::default()
    }

    /// Default config with just the official repository.
    fn default() -> Self {
        Self {
            repos: vec![RepoConfig {
                name: "stable".to_string(),
                url: DEFAULT_REPO.to_string(),
                priority: 100,
                enabled: true,
            }],
        }
    }

    /// Parse config from YAML-ish text format.
    ///
    /// Format:
    /// ```yaml
    /// # OurOS package manager configuration
    /// repo: stable
    ///   url: https://repo.ouros.org/stable
    ///   priority: 100
    ///   enabled: yes
    ///
    /// repo: community
    ///   url: https://community.ouros.org/packages
    ///   priority: 500
    ///   enabled: yes
    /// ```
    fn parse(text: &str) -> Option<Self> {
        let mut repos = Vec::new();
        let mut current_name: Option<String> = None;
        let mut current_url = String::new();
        let mut current_priority = 500u32;
        let mut current_enabled = true;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("repo:") {
                // Save previous repo
                if let Some(ref name) = current_name {
                    if !current_url.is_empty() {
                        repos.push(RepoConfig {
                            name: name.clone(),
                            url: current_url.clone(),
                            priority: current_priority,
                            enabled: current_enabled,
                        });
                    }
                }
                current_name = Some(rest.trim().to_string());
                current_url.clear();
                current_priority = 500;
                current_enabled = true;
            } else if let Some((key, val)) = trimmed.split_once(':') {
                let key = key.trim();
                let val = val.trim();
                match key {
                    "url" => current_url = val.to_string(),
                    "priority" => current_priority = val.parse().unwrap_or(500),
                    "enabled" => current_enabled = val == "yes" || val == "true" || val == "1",
                    _ => {} // ignore unknown keys for forward compat
                }
            }
        }

        // Save last repo
        if let Some(ref name) = current_name {
            if !current_url.is_empty() {
                repos.push(RepoConfig {
                    name: name.clone(),
                    url: current_url,
                    priority: current_priority,
                    enabled: current_enabled,
                });
            }
        }

        if repos.is_empty() {
            return None;
        }

        Some(Self { repos })
    }

    /// Serialize config to text format for saving.
    fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str("# OurOS package manager configuration\n");
        out.push_str("# Repositories are checked in priority order (lower = preferred).\n\n");

        for repo in &self.repos {
            out.push_str(&format!("repo: {}\n", repo.name));
            out.push_str(&format!("  url: {}\n", repo.url));
            out.push_str(&format!("  priority: {}\n", repo.priority));
            out.push_str(&format!(
                "  enabled: {}\n",
                if repo.enabled { "yes" } else { "no" }
            ));
            out.push('\n');
        }

        out
    }

    /// Save configuration to the config file.
    fn save(&self) -> io::Result<()> {
        if let Some(parent) = Path::new(CONFIG_PATH).parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(CONFIG_PATH, self.serialize())
    }

    /// Get all enabled repositories sorted by priority (lower first).
    fn enabled_repos(&self) -> Vec<&RepoConfig> {
        let mut repos: Vec<&RepoConfig> = self.repos.iter().filter(|r| r.enabled).collect();
        repos.sort_by_key(|r| r.priority);
        repos
    }

    /// Add a new repository. Returns error if name already exists.
    fn add_repo(&mut self, name: &str, url: &str, priority: u32) -> Result<(), String> {
        if self.repos.iter().any(|r| r.name == name) {
            return Err(format!("repository '{name}' already exists"));
        }
        self.repos.push(RepoConfig {
            name: name.to_string(),
            url: url.to_string(),
            priority,
            enabled: true,
        });
        Ok(())
    }

    /// Remove a repository by name.
    fn remove_repo(&mut self, name: &str) -> Result<(), String> {
        let initial_len = self.repos.len();
        self.repos.retain(|r| r.name != name);
        if self.repos.len() == initial_len {
            return Err(format!("repository '{name}' not found"));
        }
        Ok(())
    }

    /// Enable or disable a repository.
    fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<(), String> {
        for repo in &mut self.repos {
            if repo.name == name {
                repo.enabled = enabled;
                return Ok(());
            }
        }
        Err(format!("repository '{name}' not found"))
    }
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

    /// Deploy a blob to a destination path using hardlink deduplication.
    ///
    /// This creates a hard link from `dst` to the CAS blob identified by `hash`.
    /// If the blob is shared between multiple packages, they all point to the same
    /// physical storage — zero additional disk space for duplicated files.
    ///
    /// Falls back to a regular file copy if hardlinks fail (e.g., cross-device).
    fn deploy_hardlink(&self, hash: &str, dst: &Path) -> io::Result<()> {
        let blob_path = self.blob_path(hash);
        if !blob_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("blob {hash} not found in CAS"),
            ));
        }

        // Ensure parent directory exists
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }

        // Remove existing file at destination if any
        if dst.exists() {
            let _ = fs::remove_file(dst);
        }

        // Try hardlink first (zero-copy, shared storage)
        match fs::hard_link(&blob_path, dst) {
            Ok(()) => Ok(()),
            Err(_) => {
                // Fallback: copy the file (cross-device or unsupported filesystem)
                fs::copy(&blob_path, dst)?;
                Ok(())
            }
        }
    }

    /// Deploy all files from a package manifest to the filesystem.
    ///
    /// Uses hardlinks from CAS for deduplication. Returns the number of
    /// files successfully deployed and the number of bytes saved by dedup.
    fn deploy_package_files(&self, manifest: &PackageManifest) -> io::Result<DeployStats> {
        let mut stats = DeployStats::default();

        for file in &manifest.files {
            if file.hash.is_empty() {
                continue;
            }

            let dst = Path::new(&file.dst);
            match self.deploy_hardlink(&file.hash, dst) {
                Ok(()) => {
                    stats.deployed += 1;
                    stats.total_bytes += file.size;
                    // Check if the blob has more than one link (dedup savings)
                    if let Ok(meta) = fs::metadata(self.blob_path(&file.hash)) {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::MetadataExt;
                            if meta.nlink() > 1 {
                                stats.dedup_bytes += file.size;
                            }
                        }
                        #[cfg(not(unix))]
                        {
                            // On non-unix, we can't easily check nlink.
                            // Assume dedup if the file was already in CAS.
                            let _ = meta;
                        }
                    }

                    // Set file permissions
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = fs::set_permissions(dst, fs::Permissions::from_mode(file.mode));
                    }
                }
                Err(e) => {
                    eprintln!(
                        "  warning: failed to deploy {}: {e}",
                        file.dst
                    );
                    stats.failed += 1;
                }
            }
        }

        Ok(stats)
    }

    /// Deploy files with config file protection for upgrades.
    ///
    /// Config files listed in the manifest's `conffiles` field are checked
    /// against their previously-deployed hash. If the user modified the file,
    /// the new version is saved as `<path>.pkg-new` instead of overwriting.
    fn deploy_package_files_upgrade(
        &self,
        manifest: &PackageManifest,
        old_file_hashes: &[(String, String)],
    ) -> io::Result<DeployStats> {
        let mut stats = DeployStats::default();

        // Build lookup of old hashes by path
        let old_hashes: BTreeMap<&str, &str> = old_file_hashes
            .iter()
            .map(|(path, hash)| (path.as_str(), hash.as_str()))
            .collect();

        let conffile_set: HashSet<&str> = manifest.conffiles.iter().map(|s| s.as_str()).collect();

        for file in &manifest.files {
            if file.hash.is_empty() {
                continue;
            }

            let dst = Path::new(&file.dst);

            // Check if this is a config file that might be user-modified
            if conffile_set.contains(file.dst.as_str()) {
                if let Some(&old_hash) = old_hashes.get(file.dst.as_str()) {
                    match ConfigFileTracker::deploy_config(self, &file.hash, dst, old_hash) {
                        Ok(replaced) => {
                            stats.deployed += 1;
                            stats.total_bytes += file.size;
                            if !replaced {
                                stats.config_preserved += 1;
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "  warning: failed to deploy config {}: {e}",
                                file.dst
                            );
                            stats.failed += 1;
                        }
                    }
                    continue;
                }
            }

            // Normal file deployment
            match self.deploy_hardlink(&file.hash, dst) {
                Ok(()) => {
                    stats.deployed += 1;
                    stats.total_bytes += file.size;
                    if let Ok(meta) = fs::metadata(self.blob_path(&file.hash)) {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::MetadataExt;
                            if meta.nlink() > 1 {
                                stats.dedup_bytes += file.size;
                            }
                        }
                        #[cfg(not(unix))]
                        {
                            let _ = meta;
                        }
                    }

                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = fs::set_permissions(dst, fs::Permissions::from_mode(file.mode));
                    }
                }
                Err(e) => {
                    eprintln!(
                        "  warning: failed to deploy {}: {e}",
                        file.dst
                    );
                    stats.failed += 1;
                }
            }
        }

        Ok(stats)
    }

    /// Remove deployed files for a package (cleanup on remove/rollback).
    fn undeploy_package_files(&self, file_hashes: &[(String, String)]) -> u64 {
        let mut removed = 0u64;
        for (dst_path, _hash) in file_hashes {
            let dst = Path::new(dst_path);
            if dst.exists() {
                if fs::remove_file(dst).is_ok() {
                    removed += 1;
                }
                // Clean up empty parent directories
                if let Some(parent) = dst.parent() {
                    let _ = fs::remove_dir(parent); // Only succeeds if empty
                }
            }
        }
        removed
    }
}

/// Statistics from a package file deployment operation.
#[derive(Default)]
struct DeployStats {
    /// Number of files successfully deployed.
    deployed: u64,
    /// Number of files that failed to deploy.
    failed: u64,
    /// Total bytes of all deployed files.
    total_bytes: u64,
    /// Bytes saved through hardlink deduplication.
    dedup_bytes: u64,
    /// Number of config files preserved (user-modified, new version saved as .pkg-new).
    config_preserved: u64,
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

    /// Load available packages from all cached repository indices.
    ///
    /// Aggregates packages from all enabled repositories. When the same package
    /// appears in multiple repos, the version from the higher-priority (lower
    /// number) repository is preferred.
    fn load_repo_index(&self) -> io::Result<Vec<PackageManifest>> {
        let config = PkgConfig::load();
        let repos = config.enabled_repos();
        let mut all_packages = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // Load from each repo in priority order
        for repo in &repos {
            let index_path = self.repo_index_path(&repo.name);
            if !index_path.exists() {
                // Fall back to legacy "index" file for backward compat
                continue;
            }
            let text = match fs::read_to_string(&index_path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            for chunk in text.split("\n\n") {
                let chunk = chunk.trim();
                if chunk.is_empty() {
                    continue;
                }
                if let Some(manifest) = PackageManifest::parse(chunk) {
                    // Higher-priority repos take precedence for same package name
                    let key = format!("{}:{}", manifest.name, manifest.version);
                    if !seen.contains(&key) {
                        seen.insert(key);
                        all_packages.push(manifest);
                    }
                }
            }
        }

        // Also check legacy "index" file if no per-repo indices found
        if all_packages.is_empty() {
            let legacy_path = self.repo_dir.join("index");
            if legacy_path.exists() {
                let text = fs::read_to_string(&legacy_path)?;
                for chunk in text.split("\n\n") {
                    let chunk = chunk.trim();
                    if chunk.is_empty() {
                        continue;
                    }
                    if let Some(manifest) = PackageManifest::parse(chunk) {
                        all_packages.push(manifest);
                    }
                }
            }
        }

        Ok(all_packages)
    }

    /// Path to a repo-specific cached index file.
    fn repo_index_path(&self, repo_name: &str) -> PathBuf {
        self.repo_dir.join(format!("{repo_name}.index"))
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
// Transaction log — audit trail for all package operations
// ============================================================================

/// Where the transaction log lives.
const TX_LOG_PATH: &str = "/var/pkg/transactions.log";

/// A transaction records a single package manager operation.
#[derive(Clone, Debug)]
struct Transaction {
    /// Monotonic sequence number.
    id: u64,
    /// Unix timestamp.
    timestamp: u64,
    /// What operation was performed.
    operation: TxOperation,
    /// Generation ID created by this operation.
    generation_id: u64,
    /// Previous generation ID.
    prev_generation_id: u64,
    /// Packages affected.
    packages: Vec<String>,
    /// Human-readable description.
    description: String,
}

#[derive(Clone, Debug)]
enum TxOperation {
    Install,
    InstallLocal,
    Remove,
    Upgrade,
    Rollback,
    GarbageCollect,
}

impl TxOperation {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::InstallLocal => "install-local",
            Self::Remove => "remove",
            Self::Upgrade => "upgrade",
            Self::Rollback => "rollback",
            Self::GarbageCollect => "gc",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "install" => Some(Self::Install),
            "install-local" => Some(Self::InstallLocal),
            "remove" => Some(Self::Remove),
            "upgrade" => Some(Self::Upgrade),
            "rollback" => Some(Self::Rollback),
            "gc" => Some(Self::GarbageCollect),
            _ => None,
        }
    }
}

/// Transaction log manager — append-only log of all package operations.
///
/// Each line is a JSON-lines record (text-based per OS design spec):
/// ```json
/// {"id":1,"ts":1700000000,"op":"install","gen":2,"prev":1,"pkgs":["foo","bar"],"desc":"install foo, bar"}
/// ```
struct TransactionLog;

impl TransactionLog {
    /// Append a transaction to the log.
    fn append(tx: &Transaction) -> io::Result<()> {
        let pkgs_json: Vec<String> = tx
            .packages
            .iter()
            .map(|p| format!("\"{}\"", p.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect();

        let line = format!(
            "{{\"id\":{},\"ts\":{},\"op\":\"{}\",\"gen\":{},\"prev\":{},\"pkgs\":[{}],\"desc\":\"{}\"}}\n",
            tx.id,
            tx.timestamp,
            tx.operation.as_str(),
            tx.generation_id,
            tx.prev_generation_id,
            pkgs_json.join(","),
            tx.description.replace('\\', "\\\\").replace('"', "\\\""),
        );

        // Ensure parent directory exists
        if let Some(parent) = Path::new(TX_LOG_PATH).parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Append to the log file
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(TX_LOG_PATH)?;
        io::Write::write_all(&mut file, line.as_bytes())?;
        Ok(())
    }

    /// Read the transaction log.
    fn read_all() -> io::Result<Vec<Transaction>> {
        let text = match fs::read_to_string(TX_LOG_PATH) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };

        let mut transactions = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(tx) = Self::parse_json_line(line) {
                transactions.push(tx);
            }
        }
        Ok(transactions)
    }

    /// Read the last N transactions.
    #[allow(dead_code)] // Will be used by future 'pkg log' enhancements
    fn read_last(n: usize) -> io::Result<Vec<Transaction>> {
        let all = Self::read_all()?;
        let start = all.len().saturating_sub(n);
        Ok(all[start..].to_vec())
    }

    /// Get the next transaction ID.
    fn next_id() -> u64 {
        Self::read_all()
            .ok()
            .and_then(|txs| txs.last().map(|t| t.id + 1))
            .unwrap_or(1)
    }

    /// Parse a JSON-lines record. Minimal parser — we know the exact format.
    fn parse_json_line(line: &str) -> Option<Transaction> {
        // Extract values from our known JSON format
        let id = Self::extract_u64(line, "\"id\":")?;
        let timestamp = Self::extract_u64(line, "\"ts\":")?;
        let op_str = Self::extract_string(line, "\"op\":\"")?;
        let operation = TxOperation::parse(&op_str)?;
        let generation_id = Self::extract_u64(line, "\"gen\":")?;
        let prev_generation_id = Self::extract_u64(line, "\"prev\":")?;
        let description = Self::extract_string(line, "\"desc\":\"").unwrap_or_default();

        // Extract packages array
        let packages = Self::extract_string_array(line, "\"pkgs\":");

        Some(Transaction {
            id,
            timestamp,
            operation,
            generation_id,
            prev_generation_id,
            packages,
            description,
        })
    }

    fn extract_u64(line: &str, key: &str) -> Option<u64> {
        let start = line.find(key)? + key.len();
        let rest = &line[start..];
        let end = rest.find(|c: char| !c.is_ascii_digit())?;
        rest[..end].parse().ok()
    }

    fn extract_string(line: &str, key: &str) -> Option<String> {
        let start = line.find(key)? + key.len();
        let rest = &line[start..];
        // Find the closing quote (handle escaped quotes)
        let mut end = 0;
        let bytes = rest.as_bytes();
        while end < bytes.len() {
            if bytes[end] == b'"' {
                if end == 0 || bytes[end - 1] != b'\\' {
                    break;
                }
            }
            end += 1;
        }
        Some(rest[..end].replace("\\\"", "\"").replace("\\\\", "\\"))
    }

    fn extract_string_array(line: &str, key: &str) -> Vec<String> {
        let start = match line.find(key) {
            Some(s) => s + key.len(),
            None => return Vec::new(),
        };
        let rest = &line[start..];
        let end = match rest.find(']') {
            Some(e) => e,
            None => return Vec::new(),
        };
        let array_content = &rest[1..end]; // skip '['
        let mut result = Vec::new();
        for item in array_content.split(',') {
            let item = item.trim().trim_matches('"');
            if !item.is_empty() {
                result.push(item.replace("\\\"", "\"").replace("\\\\", "\\"));
            }
        }
        result
    }
}

/// Helper: create and log a transaction for a package operation.
fn log_transaction(
    op: TxOperation,
    gen_id: u64,
    prev_gen_id: u64,
    packages: &[String],
    description: &str,
) {
    let tx = Transaction {
        id: TransactionLog::next_id(),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        operation: op,
        generation_id: gen_id,
        prev_generation_id: prev_gen_id,
        packages: packages.to_vec(),
        description: description.to_string(),
    };
    if let Err(e) = TransactionLog::append(&tx) {
        eprintln!("pkg: warning: failed to write transaction log: {e}");
    }
}

// ============================================================================
// Config file management
// ============================================================================

/// A file that should not be overwritten during upgrades if the user has
/// modified it.
///
/// Config files are tracked in the generation's installed package info
/// alongside regular file hashes. During upgrade, if a config file's
/// deployed copy differs from the original hash, we preserve the user's
/// version and save the new version as `<file>.pkg-new`.
struct ConfigFileTracker;

#[allow(dead_code)] // Infrastructure for upgrade config protection — wired in next
impl ConfigFileTracker {
    /// Check whether a deployed file has been modified by the user.
    ///
    /// Compares the current file on disk to its expected CAS hash.
    /// Returns true if the file exists and has been modified.
    fn is_user_modified(path: &Path, expected_hash: &str) -> bool {
        if expected_hash.is_empty() {
            return false;
        }
        match fs::read(path) {
            Ok(data) => sha256_hex(&data) != expected_hash,
            Err(_) => false, // file doesn't exist or unreadable — not "modified"
        }
    }

    /// Deploy a config file with user-modification protection.
    ///
    /// If the file already exists and has been modified by the user,
    /// save the new version as `<path>.pkg-new` and print a notice.
    /// Otherwise deploy normally.
    fn deploy_config(cas: &ContentStore, hash: &str, dst: &Path, old_hash: &str) -> io::Result<bool> {
        if dst.exists() && Self::is_user_modified(dst, old_hash) {
            // User modified the config — don't clobber it
            let new_path = PathBuf::from(format!("{}.pkg-new", dst.display()));
            cas.deploy_hardlink(hash, &new_path)?;
            eprintln!(
                "  notice: {} modified by user — new version saved as {}",
                dst.display(),
                new_path.display()
            );
            Ok(false) // not replaced
        } else {
            cas.deploy_hardlink(hash, dst)?;
            Ok(true) // replaced
        }
    }
}

// ============================================================================
// Package hooks
// ============================================================================

/// Package lifecycle hooks — shell commands run at specific points during
/// install/remove/upgrade operations.
///
/// Hooks are defined in the manifest:
/// ```
/// hook-pre-install: /usr/lib/mypackage/setup.sh
/// hook-post-install: /usr/lib/mypackage/configure.sh
/// hook-pre-remove: /usr/lib/mypackage/cleanup.sh
/// hook-post-remove: /usr/lib/mypackage/teardown.sh
/// ```
///
/// Hooks run with the package name and version as arguments.
/// A non-zero exit code from a pre-hook aborts the operation.
struct PackageHooks;

#[allow(dead_code)] // Infrastructure for package lifecycle hooks — wired in next
impl PackageHooks {
    /// Run a hook command if it exists.
    ///
    /// Returns Ok(true) if the hook ran and succeeded,
    /// Ok(false) if no hook was defined, or Err if it failed.
    fn run(command: &str, pkg_name: &str, pkg_version: &str) -> Result<bool, String> {
        if command.is_empty() {
            return Ok(false);
        }

        use std::process::Command;

        match Command::new(command)
            .arg(pkg_name)
            .arg(pkg_version)
            .status()
        {
            Ok(status) => {
                if status.success() {
                    Ok(true)
                } else {
                    Err(format!(
                        "hook '{}' failed with exit code {}",
                        command,
                        status.code().unwrap_or(-1)
                    ))
                }
            }
            Err(e) => Err(format!("failed to execute hook '{}': {e}", command)),
        }
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
        // Run pre-install hook if defined
        if !manifest.hook_pre_install.is_empty() {
            let ver_str = manifest.version.to_string();
            match PackageHooks::run(&manifest.hook_pre_install, &manifest.name, &ver_str) {
                Ok(true) => println!("  pre-install hook OK for {}", manifest.name),
                Ok(false) => {} // no hook
                Err(e) => {
                    eprintln!("pkg: {}: pre-install hook failed: {e}", manifest.name);
                    eprintln!("pkg: skipping {}", manifest.name);
                    continue;
                }
            }
        }

        // Ensure the package archive is available in CAS. If not, try to
        // download it from the repository.
        if !manifest.archive_hash.is_empty() && !db.cas.has(&manifest.archive_hash) {
            let version_str = manifest.version.to_string();
            match cmd_download(db, &manifest.name, &version_str) {
                Ok(hash) => {
                    if hash != manifest.archive_hash {
                        eprintln!(
                            "pkg: hash mismatch for {}: expected {}, got {}",
                            manifest.name, manifest.archive_hash, hash
                        );
                        continue;
                    }
                }
                Err(e) => {
                    eprintln!("pkg: failed to download {}: {e}", manifest.name);
                    eprintln!("pkg: continuing with manifest-only install");
                }
            }
        }

        // Store the manifest itself in CAS
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

        // Deploy files to the filesystem via hardlinks from CAS
        if !manifest.files.is_empty() && !manifest.archive_hash.is_empty() {
            match db.cas.deploy_package_files(manifest) {
                Ok(stats) => {
                    let dedup_info = if stats.dedup_bytes > 0 {
                        format!(" ({} saved via dedup)", format_size(stats.dedup_bytes))
                    } else {
                        String::new()
                    };
                    println!(
                        "  Installed {} {}: {} files, {}{}",
                        manifest.name,
                        manifest.version,
                        stats.deployed,
                        format_size(stats.total_bytes),
                        dedup_info
                    );
                    if stats.failed > 0 {
                        eprintln!("    {} file(s) failed to deploy", stats.failed);
                    }
                }
                Err(e) => {
                    eprintln!("  warning: file deployment error for {}: {e}", manifest.name);
                }
            }
        } else {
            println!("  Installed {} {} (metadata only)", manifest.name, manifest.version);
        }

        // Run post-install hook if defined
        if !manifest.hook_post_install.is_empty() {
            let ver_str = manifest.version.to_string();
            match PackageHooks::run(&manifest.hook_post_install, &manifest.name, &ver_str) {
                Ok(true) => println!("  post-install hook OK for {}", manifest.name),
                Ok(false) => {}
                Err(e) => {
                    eprintln!("  warning: {}: post-install hook failed: {e}", manifest.name);
                    // Post-install hook failure is not fatal — package is already installed
                }
            }
        }
    }

    // Commit generation atomically
    match db.save_generation(&new_gen) {
        Ok(()) => {
            if let Err(e) = db.set_current_generation(new_gen.id) {
                eprintln!("pkg: CRITICAL — saved generation but failed to update pointer: {e}");
                eprintln!("pkg: manually run: echo {} > {}/current", new_gen.id, GEN_DIR);
                process::exit(1);
            }
            log_transaction(
                TxOperation::Install,
                new_gen.id,
                current.id,
                &to_install,
                &desc,
            );
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
        // Run pre-remove hook if we can find the manifest in CAS
        if let Some(pkg) = current.packages.get(name) {
            if let Ok(manifest_data) = db.cas.get(&pkg.manifest_hash) {
                if let Ok(text) = String::from_utf8(manifest_data) {
                    if let Some(manifest) = PackageManifest::parse(&text) {
                        if !manifest.hook_pre_remove.is_empty() {
                            let ver_str = pkg.version.to_string();
                            match PackageHooks::run(&manifest.hook_pre_remove, name, &ver_str) {
                                Ok(true) => println!("  pre-remove hook OK for {name}"),
                                Ok(false) => {}
                                Err(e) => {
                                    eprintln!("  warning: {name}: pre-remove hook failed: {e}");
                                }
                            }
                        }
                    }
                }
            }
        }

        // Undeploy files from the filesystem before removing from generation
        if let Some(pkg) = current.packages.get(name) {
            let removed = db.cas.undeploy_package_files(&pkg.file_hashes);
            if removed > 0 {
                println!("  Removing {name}... ({removed} files removed)");
            } else {
                println!("  Removing {name}...");
            }
        }
        new_gen.packages.remove(name);

        // Run post-remove hook
        if let Some(pkg) = current.packages.get(name) {
            if let Ok(manifest_data) = db.cas.get(&pkg.manifest_hash) {
                if let Ok(text) = String::from_utf8(manifest_data) {
                    if let Some(manifest) = PackageManifest::parse(&text) {
                        if !manifest.hook_post_remove.is_empty() {
                            let ver_str = pkg.version.to_string();
                            match PackageHooks::run(&manifest.hook_post_remove, name, &ver_str) {
                                Ok(true) => println!("  post-remove hook OK for {name}"),
                                Ok(false) => {}
                                Err(e) => {
                                    eprintln!("  warning: {name}: post-remove hook failed: {e}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    match db.save_generation(&new_gen) {
        Ok(()) => {
            let _ = db.set_current_generation(new_gen.id);
            log_transaction(
                TxOperation::Remove,
                new_gen.id,
                current.id,
                packages,
                &desc,
            );
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
            log_transaction(
                TxOperation::Rollback,
                rollback_gen.id,
                current_id,
                &[],
                &desc,
            );
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

    let gc_desc = format!(
        "gc: removed {} gen(s), {} blob(s), {} freed",
        remove_ids.len(),
        orphaned,
        format_size(freed)
    );
    log_transaction(
        TxOperation::GarbageCollect,
        current_id,
        current_id,
        &[],
        &gc_desc,
    );

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

    let mut total_config_preserved = 0u64;

    for (name, _, new_ver) in &upgrades {
        let manifest = match available
            .iter()
            .find(|p| p.name == *name && &p.version == *new_ver)
        {
            Some(m) => m,
            None => continue,
        };

        // Run pre-install hook (used for upgrades too)
        if !manifest.hook_pre_install.is_empty() {
            let ver_str = manifest.version.to_string();
            match PackageHooks::run(&manifest.hook_pre_install, &manifest.name, &ver_str) {
                Ok(true) => println!("  pre-install hook OK for {}", manifest.name),
                Ok(false) => {}
                Err(e) => {
                    eprintln!("pkg: {}: pre-install hook failed: {e}", manifest.name);
                    eprintln!("pkg: skipping {}", manifest.name);
                    continue;
                }
            }
        }

        // Download archive if needed
        if !manifest.archive_hash.is_empty() && !db.cas.has(&manifest.archive_hash) {
            let version_str = manifest.version.to_string();
            match cmd_download(db, &manifest.name, &version_str) {
                Ok(hash) => {
                    if hash != manifest.archive_hash {
                        eprintln!(
                            "pkg: hash mismatch for {}: expected {}, got {}",
                            manifest.name, manifest.archive_hash, hash
                        );
                        continue;
                    }
                }
                Err(e) => {
                    eprintln!("pkg: failed to download {}: {e}", manifest.name);
                    eprintln!("pkg: continuing with manifest-only upgrade");
                }
            }
        }

        // Store the manifest itself in CAS
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

        // Get old file hashes for config-aware deployment
        let old_file_hashes: Vec<(String, String)> = current
            .packages
            .get(*name)
            .map(|p| p.file_hashes.clone())
            .unwrap_or_default();

        let new_file_hashes: Vec<(String, String)> = manifest
            .files
            .iter()
            .map(|f| (f.dst.clone(), f.hash.clone()))
            .collect();

        // Remove files from the old version that are not in the new version.
        // Build a set of new destination paths for quick lookup.
        let new_dsts: HashSet<&str> = manifest
            .files
            .iter()
            .map(|f| f.dst.as_str())
            .collect();

        let mut removed_stale = 0u64;
        for (old_dst, _) in &old_file_hashes {
            if !new_dsts.contains(old_dst.as_str()) {
                let dst = Path::new(old_dst);
                if dst.exists() {
                    if fs::remove_file(dst).is_ok() {
                        removed_stale += 1;
                    }
                    if let Some(parent) = dst.parent() {
                        let _ = fs::remove_dir(parent);
                    }
                }
            }
        }

        // Deploy new files with config-aware upgrade logic
        if !manifest.files.is_empty() && !manifest.archive_hash.is_empty() {
            match db.cas.deploy_package_files_upgrade(manifest, &old_file_hashes) {
                Ok(stats) => {
                    let mut extras = Vec::new();
                    if stats.dedup_bytes > 0 {
                        extras.push(format!("{} saved via dedup", format_size(stats.dedup_bytes)));
                    }
                    if stats.config_preserved > 0 {
                        extras.push(format!("{} config(s) preserved", stats.config_preserved));
                        total_config_preserved += stats.config_preserved;
                    }
                    if removed_stale > 0 {
                        extras.push(format!("{removed_stale} stale file(s) removed"));
                    }
                    let extra_str = if extras.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", extras.join(", "))
                    };
                    println!(
                        "  Upgraded {} to {}: {} files, {}{}",
                        manifest.name,
                        manifest.version,
                        stats.deployed,
                        format_size(stats.total_bytes),
                        extra_str
                    );
                    if stats.failed > 0 {
                        eprintln!("    {} file(s) failed to deploy", stats.failed);
                    }
                }
                Err(e) => {
                    eprintln!("  warning: file deployment error for {}: {e}", manifest.name);
                }
            }
        } else {
            if removed_stale > 0 {
                println!(
                    "  Upgraded {} to {} (metadata only, {removed_stale} stale file(s) removed)",
                    manifest.name, manifest.version
                );
            } else {
                println!(
                    "  Upgraded {} to {} (metadata only)",
                    manifest.name, manifest.version
                );
            }
        }

        new_gen.packages.insert(
            name.to_string(),
            InstalledPackage {
                version: manifest.version.clone(),
                manifest_hash,
                file_hashes: new_file_hashes,
                installed_at: now,
                explicit: old_explicit,
            },
        );

        // Run post-install hook (used for upgrades too)
        if !manifest.hook_post_install.is_empty() {
            let ver_str = manifest.version.to_string();
            match PackageHooks::run(&manifest.hook_post_install, &manifest.name, &ver_str) {
                Ok(true) => println!("  post-install hook OK for {}", manifest.name),
                Ok(false) => {}
                Err(e) => {
                    eprintln!("  warning: {}: post-install hook failed: {e}", manifest.name);
                }
            }
        }
    }

    match db.save_generation(&new_gen) {
        Ok(()) => {
            let _ = db.set_current_generation(new_gen.id);
            log_transaction(
                TxOperation::Upgrade,
                new_gen.id,
                current.id,
                &names,
                &desc,
            );
            println!("\nDone. Generation {} created.", new_gen.id);
            if total_config_preserved > 0 {
                println!(
                    "  {total_config_preserved} config file(s) preserved — \
                     new versions saved as .pkg-new"
                );
            }
        }
        Err(e) => {
            eprintln!("pkg: failed to save generation: {e}");
            process::exit(1);
        }
    }
}

fn cmd_update(db: &PackageDb) {
    db.ensure_dirs();

    let config = PkgConfig::load();
    let repos = config.enabled_repos();

    if repos.is_empty() {
        eprintln!("pkg: no repositories configured");
        eprintln!("pkg: add one with 'pkg repo add <name> <url>'");
        process::exit(1);
    }

    let mut total_entries = 0usize;
    let mut failures = 0u32;

    for repo in &repos {
        let index_url = format!("{}/index", repo.url);
        println!("Fetching {} ({})...", repo.name, index_url);

        match fetch_url(&index_url) {
            Ok(body) => {
                let index_path = db.repo_index_path(&repo.name);
                match fs::write(&index_path, &body) {
                    Ok(()) => {
                        let entry_count = count_index_entries(&index_path);
                        total_entries += entry_count;
                        println!(
                            "  {} updated: {} ({} packages)",
                            repo.name,
                            format_size(body.len() as u64),
                            entry_count
                        );
                    }
                    Err(e) => {
                        eprintln!("  {} write error: {e}", repo.name);
                        failures += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("  {} fetch failed: {e}", repo.name);
                // Check for cached data
                let index_path = db.repo_index_path(&repo.name);
                if index_path.exists() {
                    let cached_count = count_index_entries(&index_path);
                    total_entries += cached_count;
                    println!("  {} using cache ({} packages)", repo.name, cached_count);
                } else {
                    failures += 1;
                }
            }
        }
    }

    if failures == repos.len() as u32 {
        // All repos failed — try legacy index as last resort
        let legacy_path = db.repo_dir.join("index");
        if legacy_path.exists() {
            let legacy_count = count_index_entries(&legacy_path);
            println!(
                "\nAll repositories failed. Using legacy cached index ({} packages).",
                legacy_count
            );
        } else {
            eprintln!("\npkg: all repository fetches failed");
            process::exit(1);
        }
    } else {
        println!(
            "\nDone. {} repository(ies) updated, {} total packages available.",
            repos.len() as u32 - failures,
            total_entries
        );
        if failures > 0 {
            eprintln!("  ({failures} repository(ies) failed — using cached data where available)");
        }
    }
}

/// Manage repository configuration.
///
/// Subcommands:
///   pkg repo list              — show configured repositories
///   pkg repo add <name> <url>  — add a third-party repository
///   pkg repo remove <name>     — remove a repository
///   pkg repo enable <name>     — enable a disabled repository
///   pkg repo disable <name>    — disable a repository without removing it
fn cmd_repo(args: &[String]) {
    if args.is_empty() {
        cmd_repo_list();
        return;
    }

    match args[0].as_str() {
        "list" | "ls" => cmd_repo_list(),
        "add" => {
            if args.len() < 3 {
                eprintln!("Usage: pkg repo add <name> <url> [priority]");
                process::exit(1);
            }
            let name = &args[1];
            let url = &args[2];
            let priority = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(500);

            let mut config = PkgConfig::load();
            match config.add_repo(name, url, priority) {
                Ok(()) => {
                    if let Err(e) = config.save() {
                        eprintln!("pkg: failed to save config: {e}");
                        process::exit(1);
                    }
                    println!("Added repository '{name}' ({url}, priority {priority})");
                    println!("Run 'pkg update' to fetch the index.");
                }
                Err(e) => {
                    eprintln!("pkg: {e}");
                    process::exit(1);
                }
            }
        }
        "remove" | "rm" => {
            if args.len() < 2 {
                eprintln!("Usage: pkg repo remove <name>");
                process::exit(1);
            }
            let name = &args[1];

            let mut config = PkgConfig::load();
            match config.remove_repo(name) {
                Ok(()) => {
                    if let Err(e) = config.save() {
                        eprintln!("pkg: failed to save config: {e}");
                        process::exit(1);
                    }
                    println!("Removed repository '{name}'");
                    // Also remove cached index
                    let db = PackageDb::new();
                    let index_path = db.repo_index_path(name);
                    let _ = fs::remove_file(index_path);
                }
                Err(e) => {
                    eprintln!("pkg: {e}");
                    process::exit(1);
                }
            }
        }
        "enable" => {
            if args.len() < 2 {
                eprintln!("Usage: pkg repo enable <name>");
                process::exit(1);
            }
            let name = &args[1];
            let mut config = PkgConfig::load();
            match config.set_enabled(name, true) {
                Ok(()) => {
                    if let Err(e) = config.save() {
                        eprintln!("pkg: failed to save config: {e}");
                        process::exit(1);
                    }
                    println!("Enabled repository '{name}'");
                }
                Err(e) => {
                    eprintln!("pkg: {e}");
                    process::exit(1);
                }
            }
        }
        "disable" => {
            if args.len() < 2 {
                eprintln!("Usage: pkg repo disable <name>");
                process::exit(1);
            }
            let name = &args[1];
            let mut config = PkgConfig::load();
            match config.set_enabled(name, false) {
                Ok(()) => {
                    if let Err(e) = config.save() {
                        eprintln!("pkg: failed to save config: {e}");
                        process::exit(1);
                    }
                    println!("Disabled repository '{name}'");
                }
                Err(e) => {
                    eprintln!("pkg: {e}");
                    process::exit(1);
                }
            }
        }
        other => {
            eprintln!("pkg: unknown repo subcommand: {other}");
            eprintln!("Usage: pkg repo [list|add|remove|enable|disable]");
            process::exit(1);
        }
    }
}

fn cmd_repo_list() {
    let config = PkgConfig::load();

    if config.repos.is_empty() {
        println!("No repositories configured.");
        println!("Add one with: pkg repo add <name> <url>");
        return;
    }

    println!(
        "{:<20} {:<8} {:<8} {}",
        "REPOSITORY", "PRIO", "STATUS", "URL"
    );
    for repo in &config.repos {
        let status = if repo.enabled { "enabled" } else { "disabled" };
        println!(
            "{:<20} {:<8} {:<8} {}",
            repo.name, repo.priority, status, repo.url
        );
    }
}

/// Show the transaction log.
///
/// `pkg log` shows the last 20 transactions.
/// `pkg log N` shows the last N transactions.
/// `pkg log --all` shows all transactions.
fn cmd_log(args: &[String]) {
    let transactions = match TransactionLog::read_all() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("pkg: failed to read transaction log: {e}");
            process::exit(1);
        }
    };

    if transactions.is_empty() {
        println!("No transactions recorded.");
        return;
    }

    let show_all = args.iter().any(|a| a == "--all" || a == "-a");
    let count: usize = if show_all {
        transactions.len()
    } else {
        args.iter()
            .find(|a| a.parse::<usize>().is_ok())
            .and_then(|a| a.parse().ok())
            .unwrap_or(20)
    };

    let start = transactions.len().saturating_sub(count);
    let shown = &transactions[start..];

    println!(
        "{:<6} {:<20} {:<14} {:<6} {}",
        "TX", "DATE", "OPERATION", "GEN", "PACKAGES"
    );

    for tx in shown {
        let date = format_timestamp(tx.timestamp);
        let pkgs = if tx.packages.is_empty() {
            tx.description.clone()
        } else {
            tx.packages.join(", ")
        };
        println!(
            "{:<6} {:<20} {:<14} {:<6} {}",
            tx.id,
            date,
            tx.operation.as_str(),
            tx.generation_id,
            pkgs,
        );
    }

    if !show_all && start > 0 {
        println!(
            "\n({} older transactions not shown — use 'pkg log --all')",
            start
        );
    }
}

/// Download a package archive from repositories and store it in the CAS.
///
/// Tries each enabled repository in priority order until the download succeeds.
/// Returns the SHA-256 hash of the downloaded content. Verifies the hash against
/// the manifest's expected hash.
fn cmd_download(db: &PackageDb, name: &str, version: &str) -> Result<String, String> {
    db.ensure_dirs();

    let config = PkgConfig::load();
    let repos = config.enabled_repos();

    // Look up expected hash from the repository index
    let expected_hash = db
        .find_in_repo(name)
        .ok()
        .flatten()
        .and_then(|m| {
            if m.version.to_string() == version {
                Some(m.archive_hash.clone())
            } else {
                None
            }
        });

    // Try each repository in priority order
    let mut last_error = String::from("no repositories configured");
    for repo in &repos {
        let pkg_url = format!("{}/packages/{name}-{version}.pkg", repo.url);
        println!("Trying {name} {version} from {} ({pkg_url})...", repo.name);

        match fetch_url(&pkg_url) {
            Ok(body) => {
                // Store in CAS — this computes the SHA-256 as a side effect
                let hash = db
                    .cas
                    .put(&body)
                    .map_err(|e| format!("failed to store package in CAS: {e}"))?;

                // Verify hash against the index manifest if we have an expected hash
                if let Some(ref expected) = expected_hash {
                    if !expected.is_empty() && hash != *expected {
                        let _ = db.cas.remove(&hash);
                        // Don't give up — might be in another repo with the right content
                        last_error = format!(
                            "hash mismatch from {}: expected {expected}, got {hash}",
                            repo.name
                        );
                        continue;
                    }
                }

                println!(
                    "Downloaded {name} {version}: {} (sha256: {:.12}...)",
                    format_size(body.len() as u64),
                    hash
                );
                return Ok(hash);
            }
            Err(e) => {
                last_error = format!("{}: {e}", repo.name);
            }
        }
    }

    Err(format!("download failed from all repositories: {last_error}"))
}

/// Fetch a package and store it in CAS without installing.
///
/// `pkg fetch <PACKAGE>` downloads the latest version from the repository index
/// into the content-addressed store for later offline installation.
fn cmd_fetch(db: &PackageDb, packages: &[String]) {
    if packages.is_empty() {
        eprintln!("pkg: no packages specified");
        process::exit(1);
    }

    db.ensure_dirs();

    let repo_index = match db.load_repo_index() {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("pkg: failed to load repository index: {e}");
            eprintln!("pkg: try 'pkg update' first");
            process::exit(1);
        }
    };

    let mut failures = 0u32;
    for name in packages {
        let manifest = match repo_index.iter().find(|m| m.name == *name) {
            Some(m) => m,
            None => {
                eprintln!("pkg: package '{name}' not found in repository index");
                failures += 1;
                continue;
            }
        };

        let version_str = manifest.version.to_string();

        // Check if we already have it in CAS
        if !manifest.archive_hash.is_empty() && db.cas.has(&manifest.archive_hash) {
            println!("{name} {version_str}: already in CAS (sha256: {:.12}...)", manifest.archive_hash);
            continue;
        }

        match cmd_download(db, name, &version_str) {
            Ok(_hash) => {}
            Err(e) => {
                eprintln!("pkg: failed to fetch {name}: {e}");
                failures += 1;
            }
        }
    }

    if failures > 0 {
        eprintln!("\npkg: {failures} package(s) failed to fetch");
        process::exit(1);
    }
    println!("\nDone.");
}

/// Perform an HTTP GET request and return the response body.
///
/// Validates the response status is 200 OK. Maps HTTP and I/O errors into
/// a human-readable string.
fn fetch_url(url: &str) -> Result<Vec<u8>, String> {
    let client = HttpClient::new();
    let request = client
        .get(url)
        .map_err(|e| format!("invalid URL '{url}': {e}"))?
        .header("Accept", "*/*")
        .build();

    // Serialize the request — the actual sending happens via the OS network
    // stack. In a full implementation, this would open a TCP socket to
    // request.url.host:request.url.port, send the serialized bytes, and
    // read the response. For now we use the system's network facilities.
    let response_bytes = http_roundtrip(&request).map_err(|e| format!("{e}"))?;

    let response =
        httpclient::parse_response(&response_bytes, &request.url).map_err(|e| format!("{e}"))?;

    if !response.is_success() {
        return Err(format!(
            "HTTP {} {} from {url}",
            response.status, response.status_text
        ));
    }

    Ok(response.body)
}

/// Perform the actual network I/O for an HTTP request.
///
/// Opens a TCP connection to the target host, sends the serialized request,
/// and reads the full response. This bridges the httpclient library (which
/// handles serialization/parsing) to the OS network stack.
fn http_roundtrip(request: &httpclient::Request) -> io::Result<Vec<u8>> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let addr = format!("{}:{}", request.url.host, request.url.port);
    let mut stream = TcpStream::connect(&addr)?;

    // Apply timeout if set
    if request.timeout_ms > 0 {
        let timeout = std::time::Duration::from_millis(u64::from(request.timeout_ms));
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
    }

    let serialized = request.serialize();
    stream.write_all(&serialized)?;

    // Read the full response. We read in chunks until the connection is closed
    // or we detect the end of the HTTP response.
    let mut response_buf = Vec::with_capacity(8192);
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break, // Connection closed
            Ok(n) => {
                response_buf.extend_from_slice(&chunk[..n]);
                // Check if we have a complete response by looking for
                // the end of the body based on Content-Length or chunked encoding
                if response_is_complete(&response_buf) {
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                if response_buf.is_empty() {
                    return Err(e);
                }
                // We have partial data; try to use it
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if response_buf.is_empty() {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "request timed out"));
                }
                break;
            }
            Err(e) => return Err(e),
        }
    }

    if response_buf.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "empty response from server",
        ));
    }

    Ok(response_buf)
}

/// Check if a raw HTTP response buffer contains a complete response.
///
/// Looks for the header/body separator, then uses Content-Length or chunked
/// encoding markers to determine if the full body has been received.
fn response_is_complete(data: &[u8]) -> bool {
    // Find the header/body separator (\r\n\r\n)
    let header_end = match data.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(pos) => pos,
        None => return false, // Haven't received full headers yet
    };

    let header_bytes = &data[..header_end];
    let header_str = match core::str::from_utf8(header_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let body_start = header_end + 4;

    // Check for chunked encoding
    for line in header_str.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            // For chunked, look for the terminal "0\r\n\r\n"
            return data[body_start..].windows(5).any(|w| w == b"0\r\n\r\n");
        }
    }

    // Check for Content-Length
    for line in header_str.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            if let Ok(len) = rest.trim().parse::<usize>() {
                return data.len() >= body_start + len;
            }
        }
    }

    // No Content-Length and not chunked — we can't tell, so assume complete
    // once we have headers (the server will close the connection to signal end)
    true
}

fn count_index_entries(path: &Path) -> usize {
    fs::read_to_string(path)
        .unwrap_or_default()
        .split("\n\n")
        .filter(|chunk| !chunk.trim().is_empty())
        .count()
}

// ============================================================================
// .pkg file format — local package archives
// ============================================================================

/// A list of file blobs extracted from a .pkg archive: (hash, content).
type PkgFileBlobs = Vec<(String, Vec<u8>)>;

/// Magic bytes identifying our package archive format.
const PKG_MAGIC: &[u8] = b"PKG1\n";

/// Separator between the manifest section and file data section.
const PKG_FILES_SEP: &[u8] = b"\n---FILES---\n";

/// Parse a .pkg archive from raw bytes.
///
/// Format:
/// ```text
/// PKG1\n
/// <manifest text (UTF-8)>
/// \n---FILES---\n
/// <hash_hex> <size_decimal>\n
/// <raw_bytes of `size`>
/// <hash_hex> <size_decimal>\n
/// <raw_bytes of `size`>
/// ...
/// ```
///
/// Returns the manifest and an iterator-like vec of (hash, data) pairs.
fn parse_pkg_archive(data: &[u8]) -> Result<(PackageManifest, PkgFileBlobs), String> {
    // Verify magic
    if !data.starts_with(PKG_MAGIC) {
        return Err("not a valid .pkg file (missing PKG1 magic)".to_string());
    }

    let after_magic = &data[PKG_MAGIC.len()..];

    // Find the files separator
    let sep_pos = after_magic
        .windows(PKG_FILES_SEP.len())
        .position(|w| w == PKG_FILES_SEP)
        .ok_or_else(|| "invalid .pkg file (missing ---FILES--- separator)".to_string())?;

    // Parse manifest text
    let manifest_bytes = &after_magic[..sep_pos];
    let manifest_text = core::str::from_utf8(manifest_bytes)
        .map_err(|e| format!("invalid manifest UTF-8: {e}"))?;
    let manifest = PackageManifest::parse(manifest_text)
        .ok_or_else(|| "failed to parse manifest from .pkg file".to_string())?;

    // Parse file entries
    let files_section = &after_magic[sep_pos + PKG_FILES_SEP.len()..];
    let mut files = Vec::new();
    let mut pos = 0;

    while pos < files_section.len() {
        // Read the header line: "<hash> <size>\n"
        let line_end = files_section[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| "truncated file entry header in .pkg".to_string())?;

        let header_line = core::str::from_utf8(&files_section[pos..pos + line_end])
            .map_err(|e| format!("invalid file entry header: {e}"))?;
        pos += line_end + 1; // skip past the newline

        let (hash, size_str) = header_line
            .split_once(' ')
            .ok_or_else(|| format!("malformed file entry header: '{header_line}'"))?;

        let size: usize = size_str
            .parse()
            .map_err(|e| format!("invalid file size in entry '{header_line}': {e}"))?;

        // Read the file data
        if pos + size > files_section.len() {
            return Err(format!(
                "truncated file data for {hash}: expected {size} bytes, have {}",
                files_section.len() - pos
            ));
        }

        let file_data = files_section[pos..pos + size].to_vec();
        pos += size;

        files.push((hash.to_string(), file_data));
    }

    Ok((manifest, files))
}

/// Create a .pkg archive from a manifest and a directory of files.
///
/// Reads each file listed in the manifest from `base_dir`, computes its hash,
/// and packs everything into the PKG1 format.
fn create_pkg_archive(manifest: &PackageManifest, base_dir: &Path) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();

    // Write magic
    output.extend_from_slice(PKG_MAGIC);

    // Write manifest
    let manifest_text = manifest.serialize();
    output.extend_from_slice(manifest_text.as_bytes());

    // Write files separator
    output.extend_from_slice(PKG_FILES_SEP);

    // Write each file entry
    for file in &manifest.files {
        let src_path = base_dir.join(&file.src);
        let data = fs::read(&src_path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("failed to read {}: {e}", src_path.display()),
            )
        })?;

        let hash = sha256_hex(&data);
        let header = format!("{} {}\n", hash, data.len());
        output.extend_from_slice(header.as_bytes());
        output.extend_from_slice(&data);
    }

    Ok(output)
}

/// Install packages from local .pkg files.
///
/// Each path is opened, parsed as a PKG1 archive, its file blobs are stored
/// in CAS, and then deployed via hardlinks. A new generation is created
/// encompassing all installed packages.
fn cmd_install_local(db: &PackageDb, paths: &[PathBuf]) {
    db.ensure_dirs();

    let current = db.current_generation();

    // Parse all .pkg files first to validate them before making changes
    let mut parsed: Vec<(PathBuf, PackageManifest, PkgFileBlobs)> = Vec::new();
    for path in paths {
        let data = match fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("pkg: failed to read {}: {e}", path.display());
                process::exit(1);
            }
        };

        match parse_pkg_archive(&data) {
            Ok((manifest, files)) => {
                if current.packages.contains_key(&manifest.name) {
                    let existing = &current.packages[&manifest.name];
                    println!(
                        "{} {}: already installed (version {})",
                        manifest.name, manifest.version, existing.version
                    );
                    if manifest.version <= existing.version {
                        continue;
                    }
                    println!("  upgrading to {}", manifest.version);
                }
                parsed.push((path.clone(), manifest, files));
            }
            Err(e) => {
                eprintln!("pkg: {}: {e}", path.display());
                process::exit(1);
            }
        }
    }

    if parsed.is_empty() {
        println!("Nothing to install.");
        return;
    }

    // Show what will be installed
    println!("The following packages will be installed from local files:");
    for (path, manifest, files) in &parsed {
        println!(
            "  {} {} ({}, {} files)",
            manifest.name,
            manifest.version,
            path.display(),
            files.len()
        );
    }

    // Show capabilities
    let mut has_caps = false;
    for (_, manifest, _) in &parsed {
        if !manifest.capabilities.is_empty() {
            if !has_caps {
                println!("\nCapabilities requested:");
                has_caps = true;
            }
            let caps: Vec<&str> = manifest.capabilities.iter().map(|c| c.name.as_str()).collect();
            println!("  {}: {}", manifest.name, caps.join(", "));
        }
    }

    // Create new generation
    let names: Vec<&str> = parsed.iter().map(|(_, m, _)| m.name.as_str()).collect();
    let desc = format!("install (local) {}", names.join(", "));
    let mut new_gen = db.next_generation(&desc);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for (_path, manifest, files) in &parsed {
        // Store all file blobs in CAS
        let mut store_errors = 0u32;
        for (hash, data) in files {
            // Verify the hash matches the actual data
            let computed = sha256_hex(data);
            if computed != *hash {
                eprintln!(
                    "  warning: {}: file hash mismatch (archive says {hash}, data hashes to {computed})",
                    manifest.name
                );
                store_errors += 1;
                continue;
            }
            if let Err(e) = db.cas.put(data) {
                eprintln!("  warning: failed to store blob {hash}: {e}");
                store_errors += 1;
            }
        }

        if store_errors > 0 {
            eprintln!(
                "  warning: {} file(s) had errors for {}",
                store_errors, manifest.name
            );
        }

        // Store the manifest itself in CAS
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

        new_gen.packages.insert(
            manifest.name.clone(),
            InstalledPackage {
                version: manifest.version.clone(),
                manifest_hash,
                file_hashes,
                installed_at: now,
                explicit: true,
            },
        );

        // Deploy files via hardlinks
        if !manifest.files.is_empty() {
            match db.cas.deploy_package_files(manifest) {
                Ok(stats) => {
                    let dedup_info = if stats.dedup_bytes > 0 {
                        format!(" ({} saved via dedup)", format_size(stats.dedup_bytes))
                    } else {
                        String::new()
                    };
                    println!(
                        "  Installed {} {}: {} files, {}{}",
                        manifest.name,
                        manifest.version,
                        stats.deployed,
                        format_size(stats.total_bytes),
                        dedup_info
                    );
                    if stats.failed > 0 {
                        eprintln!("    {} file(s) failed to deploy", stats.failed);
                    }
                }
                Err(e) => {
                    eprintln!("  warning: file deployment error for {}: {e}", manifest.name);
                }
            }
        } else {
            println!(
                "  Installed {} {} (metadata only)",
                manifest.name, manifest.version
            );
        }
    }

    // Commit generation atomically
    let current_id = db.current_generation_id();
    match db.save_generation(&new_gen) {
        Ok(()) => {
            if let Err(e) = db.set_current_generation(new_gen.id) {
                eprintln!("pkg: CRITICAL — saved generation but failed to update pointer: {e}");
                eprintln!("pkg: manually run: echo {} > {}/current", new_gen.id, GEN_DIR);
                process::exit(1);
            }
            let pkg_names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
            log_transaction(
                TxOperation::InstallLocal,
                new_gen.id,
                current_id,
                &pkg_names,
                &desc,
            );
            println!("\nDone. Generation {} created.", new_gen.id);
        }
        Err(e) => {
            eprintln!("pkg: failed to save generation: {e}");
            process::exit(1);
        }
    }
}

/// Create a .pkg file from a manifest and source directory.
///
/// Usage: pkg pack <manifest-path> [--output <file.pkg>]
///
/// Reads the manifest, locates all referenced source files relative to the
/// manifest's parent directory, computes hashes, and creates the archive.
fn cmd_pack(manifest_path: &Path, output_path: Option<&Path>) {
    let manifest_text = match fs::read_to_string(manifest_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("pkg: failed to read manifest {}: {e}", manifest_path.display());
            process::exit(1);
        }
    };

    let mut manifest = match PackageManifest::parse(&manifest_text) {
        Some(m) => m,
        None => {
            eprintln!("pkg: failed to parse manifest {}", manifest_path.display());
            process::exit(1);
        }
    };

    let base_dir = manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."));

    // Compute hashes for all files and update the manifest entries
    let mut updated_files = Vec::new();
    for file in &manifest.files {
        let src_path = base_dir.join(&file.src);
        let data = match fs::read(&src_path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!(
                    "pkg: failed to read source file {}: {e}",
                    src_path.display()
                );
                process::exit(1);
            }
        };
        let hash = sha256_hex(&data);
        let size = data.len() as u64;
        updated_files.push(PackageFile {
            src: file.src.clone(),
            dst: file.dst.clone(),
            mode: file.mode,
            hash,
            size,
        });
    }
    manifest.files = updated_files;

    // Build the archive
    let archive = match create_pkg_archive(&manifest, base_dir) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("pkg: failed to create archive: {e}");
            process::exit(1);
        }
    };

    // Compute archive hash and update manifest
    let archive_hash = sha256_hex(&archive);
    let archive_size = archive.len() as u64;

    // Determine output path
    let default_name = format!("{}-{}.pkg", manifest.name, manifest.version);
    let out = output_path.unwrap_or_else(|| Path::new(&default_name));

    match fs::write(out, &archive) {
        Ok(()) => {
            println!(
                "Created {} ({}, sha256: {:.12}...)",
                out.display(),
                format_size(archive_size),
                archive_hash
            );
            println!("  {} {} — {} files packed",
                manifest.name,
                manifest.version,
                manifest.files.len()
            );
        }
        Err(e) => {
            eprintln!("pkg: failed to write {}: {e}", out.display());
            process::exit(1);
        }
    }
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
  install <pkg>...       Install packages (names from repo, or local .pkg files)
  remove <pkg>...        Remove packages
  update                 Refresh repository metadata
  upgrade [pkg...]       Upgrade packages (all if none specified)
  fetch <pkg>...         Download packages to CAS without installing
  pack <manifest>        Create a .pkg archive from a manifest and source files
  repo [subcommand]      Manage package repositories
  log [N|--all]          Show transaction history (default: last 20)
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
  --output, -o <file>    Output path for 'pack' command

Repository Management:
  pkg repo list                  List configured repositories
  pkg repo add <name> <url>      Add a third-party repository (priority 500)
  pkg repo add <name> <url> N    Add with custom priority (lower = preferred)
  pkg repo remove <name>         Remove a repository
  pkg repo enable <name>         Re-enable a disabled repository
  pkg repo disable <name>        Disable without removing

  Config file: {CONFIG_PATH}
  Repositories are tried in priority order when downloading packages.
  Official repos default to priority 100, user-added repos to 500.

Local Installation:
  pkg install ./path/to/package.pkg
  pkg install /abs/path/to/package.pkg

  Arguments ending in '.pkg' or pointing to existing files via relative/
  absolute paths are treated as local package archives. They are installed
  directly without needing a repository.

Creating Packages:
  pkg pack manifest.txt --output mypackage-1.0.0.pkg

  Reads a manifest file, locates all referenced source files relative to
  the manifest's directory, computes SHA-256 hashes, and produces a .pkg
  archive that can be installed with 'pkg install'.

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
        "install" => {
            // Detect if any argument is a local .pkg file path.
            // A local path is identified by: ending in ".pkg", starting with "/" or "./",
            // or containing a path separator and pointing to an existing file.
            let (local_paths, repo_names): (Vec<String>, Vec<String>) =
                rest_filtered.into_iter().partition(|arg| is_local_pkg_path(arg));

            if !local_paths.is_empty() {
                let paths: Vec<PathBuf> = local_paths.iter().map(PathBuf::from).collect();
                cmd_install_local(&db, &paths);
            }
            if !repo_names.is_empty() {
                cmd_install(&db, &repo_names, dry_run);
            }
            if local_paths.is_empty() && repo_names.is_empty() {
                eprintln!("pkg: no packages specified");
                process::exit(1);
            }
        }
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
        "fetch" | "download" => {
            if rest_filtered.is_empty() {
                eprintln!("pkg: fetch requires one or more package names");
                process::exit(1);
            }
            cmd_fetch(&db, &rest_filtered);
        }
        "pack" => {
            if rest_filtered.is_empty() {
                eprintln!("pkg: pack requires a manifest path");
                eprintln!("Usage: pkg pack <manifest-file> [--output <file.pkg>]");
                process::exit(1);
            }
            let manifest_path = Path::new(&rest_filtered[0]);
            let output = rest_filtered
                .iter()
                .position(|a| a == "--output" || a == "-o")
                .and_then(|i| rest_filtered.get(i + 1))
                .map(|s| Path::new(s.as_str()));
            cmd_pack(manifest_path, output);
        }
        "repo" | "repository" => cmd_repo(&rest_filtered),
        "log" | "history" => cmd_log(&rest_filtered),
        "help" | "--help" | "-h" => print_usage(),
        _ => {
            eprintln!("pkg: unknown command: {command}");
            eprintln!("Run 'pkg help' for usage.");
            process::exit(1);
        }
    }
}

/// Determine if an argument to `pkg install` is a local file path rather than
/// a repository package name.
///
/// Heuristic: it's a local path if it ends in `.pkg`, starts with `/` or `./`
/// or `../`, or contains a path separator and the file exists.
fn is_local_pkg_path(arg: &str) -> bool {
    if arg.ends_with(".pkg") {
        return true;
    }
    if arg.starts_with('/') || arg.starts_with("./") || arg.starts_with("../") {
        return Path::new(arg).exists();
    }
    if arg.contains('/') {
        return Path::new(arg).exists();
    }
    false
}
