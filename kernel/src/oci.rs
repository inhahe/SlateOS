//! OCI Image Format parser — container image loading.
//!
//! Implements the OCI Image Specification v1.0 for parsing and loading
//! container images from disk (e.g., images pulled with `skopeo` or
//! exported with `docker save --format=oci`).
//!
//! ## OCI Image Layout
//!
//! An OCI image on disk is a directory with this structure:
//!
//! ```text
//! <image-root>/
//! ├── oci-layout           # {"imageLayoutVersion": "1.0.0"}
//! ├── index.json           # Image index → points to manifest(s)
//! └── blobs/
//!     └── sha256/
//!         ├── <manifest>   # Image manifest (JSON)
//!         ├── <config>     # Image config (JSON)
//!         └── <layer>...   # Filesystem layers (tar+gzip)
//! ```
//!
//! ## Key types
//!
//! - **Image Index** (`index.json`): references one or more manifests
//!   (multi-platform images have one per architecture).
//! - **Image Manifest**: references the config and an ordered list of
//!   layers, each identified by content-addressable digest.
//! - **Image Config**: metadata (OS, architecture, env vars, entrypoint,
//!   cmd, labels, layer diff-ids).
//! - **Layer**: a `.tar.gz` filesystem diff.  Layers are stacked via
//!   overlayfs to form the container's root filesystem.
//!
//! ## Workflow
//!
//! 1. Read `oci-layout` to verify format version.
//! 2. Parse `index.json` to find the manifest for our platform
//!    (`linux/amd64`).
//! 3. Parse the manifest to get config digest and layer digests.
//! 4. Parse the config for runtime metadata (env, cmd, entrypoint).
//! 5. For each layer: verify digest, gunzip, extract tar into the
//!    overlay filesystem.
//!
//! ## Security
//!
//! All blobs are verified against their SHA-256 content digest before
//! use.  A mismatch means the image is corrupt or tampered with.
//!
//! ## References
//!
//! - OCI Image Spec: <https://github.com/opencontainers/image-spec>
//! - OCI Runtime Spec: <https://github.com/opencontainers/runtime-spec>
//! - Docker image format: compatible subset of OCI

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::json::{self, JsonValue};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Expected OCI image layout version.
const OCI_LAYOUT_VERSION: &str = "1.0.0";

/// OCI media type for image index.
pub const MEDIA_TYPE_INDEX: &str = "application/vnd.oci.image.index.v1+json";

/// OCI media type for image manifest.
pub const MEDIA_TYPE_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

/// OCI media type for image config.
pub const MEDIA_TYPE_CONFIG: &str = "application/vnd.oci.image.config.v1+json";

/// OCI media type for tar+gzip layers.
pub const MEDIA_TYPE_LAYER_GZIP: &str =
    "application/vnd.oci.image.layer.v1.tar+gzip";

/// OCI media type for plain tar layers.
pub const MEDIA_TYPE_LAYER_TAR: &str =
    "application/vnd.oci.image.layer.v1.tar";

/// Docker manifest v2 media type (compatibility).
pub const MEDIA_TYPE_DOCKER_MANIFEST: &str =
    "application/vnd.docker.distribution.manifest.v2+json";

/// Docker config media type (compatibility).
pub const MEDIA_TYPE_DOCKER_CONFIG: &str =
    "application/vnd.docker.container.image.v1+json";

/// Docker layer media type (compatibility).
pub const MEDIA_TYPE_DOCKER_LAYER: &str =
    "application/vnd.docker.image.rootfs.diff.tar.gzip";

/// Maximum number of layers per image.
const MAX_LAYERS: usize = 128;

/// Maximum config blob size (1 MiB — configs are small JSON).
const MAX_CONFIG_SIZE: usize = 1024 * 1024;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A content-addressable descriptor referencing a blob.
///
/// All OCI blobs are identified by their digest (typically `sha256:hex`).
#[derive(Debug, Clone)]
pub struct Descriptor {
    /// Media type of the referenced content.
    pub media_type: String,
    /// Content-addressable digest (e.g., `sha256:abcdef...`).
    pub digest: String,
    /// Size of the blob in bytes.
    pub size: u64,
}

impl Descriptor {
    /// Parse a descriptor from a JSON object.
    fn from_json(value: &JsonValue) -> KernelResult<Self> {
        let media_type = value
            .get_str("mediaType")
            .unwrap_or("")
            .into();
        let digest = value
            .get_str("digest")
            .ok_or(KernelError::InvalidArgument)?
            .into();
        let size = value
            .get_i64("size")
            .ok_or(KernelError::InvalidArgument)? as u64;

        Ok(Self {
            media_type,
            digest,
            size,
        })
    }

    /// Extract the algorithm and hex digest from the digest string.
    ///
    /// E.g., `"sha256:abcdef..."` → `("sha256", "abcdef...")`.
    #[must_use]
    pub fn split_digest(&self) -> Option<(&str, &str)> {
        self.digest.split_once(':')
    }

    /// Get the path to this blob in the OCI layout.
    ///
    /// E.g., `"sha256:abc..."` → `"blobs/sha256/abc..."`.
    #[must_use]
    pub fn blob_path(&self) -> Option<String> {
        let (algo, hex) = self.split_digest()?;
        Some(format!("blobs/{algo}/{hex}"))
    }
}

/// A platform specification for multi-platform images.
#[derive(Debug, Clone)]
pub struct Platform {
    /// Operating system (e.g., "linux").
    pub os: String,
    /// CPU architecture (e.g., "amd64").
    pub architecture: String,
    /// Variant (e.g., "v8" for arm64).
    pub variant: String,
}

impl Platform {
    /// Parse platform from JSON.
    fn from_json(value: &JsonValue) -> Self {
        Self {
            os: value.get_str("os").unwrap_or("").into(),
            architecture: value.get_str("architecture").unwrap_or("").into(),
            variant: value.get_str("variant").unwrap_or("").into(),
        }
    }

    /// Check if this platform matches our target (linux/amd64).
    #[must_use]
    pub fn matches_host(&self) -> bool {
        (self.os.is_empty() || self.os == "linux")
            && (self.architecture.is_empty() || self.architecture == "amd64")
    }
}

/// An entry in the OCI image index.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    /// Descriptor pointing to the manifest blob.
    pub descriptor: Descriptor,
    /// Platform this manifest targets (if specified).
    pub platform: Option<Platform>,
}

/// Parsed OCI image index (`index.json`).
#[derive(Debug, Clone)]
pub struct ImageIndex {
    /// Schema version (should be 2).
    pub schema_version: i64,
    /// Manifest entries (one per platform).
    pub manifests: Vec<IndexEntry>,
}

impl ImageIndex {
    /// Parse an image index from JSON bytes.
    pub fn parse(data: &[u8]) -> KernelResult<Self> {
        let root = json::parse(data)?;

        let schema_version = root.get_i64("schemaVersion").unwrap_or(2);

        let manifests_json = root
            .get_array("manifests")
            .ok_or(KernelError::InvalidArgument)?;

        let mut manifests = Vec::with_capacity(manifests_json.len());
        for entry_val in manifests_json {
            let descriptor = Descriptor::from_json(entry_val)?;
            let platform = entry_val
                .get("platform")
                .map(Platform::from_json);
            manifests.push(IndexEntry {
                descriptor,
                platform,
            });
        }

        Ok(Self {
            schema_version,
            manifests,
        })
    }

    /// Find the manifest descriptor for our host platform (linux/amd64).
    ///
    /// If there's only one manifest and no platform is specified, returns
    /// that one (common for single-platform images).
    #[must_use]
    pub fn find_manifest_for_host(&self) -> Option<&Descriptor> {
        // Single manifest without platform → assume it's for us.
        if self.manifests.len() == 1 {
            if self.manifests[0].platform.is_none()
                || self.manifests[0]
                    .platform
                    .as_ref()
                    .is_some_and(Platform::matches_host)
            {
                return Some(&self.manifests[0].descriptor);
            }
        }

        // Multi-platform: find linux/amd64.
        for entry in &self.manifests {
            if let Some(ref plat) = entry.platform {
                if plat.matches_host() {
                    return Some(&entry.descriptor);
                }
            }
        }

        // Fallback: first manifest.
        self.manifests.first().map(|e| &e.descriptor)
    }
}

/// Parsed OCI image manifest.
#[derive(Debug, Clone)]
pub struct ImageManifest {
    /// Schema version (should be 2).
    pub schema_version: i64,
    /// Media type of this manifest.
    pub media_type: String,
    /// Descriptor pointing to the image config blob.
    pub config: Descriptor,
    /// Ordered list of layer descriptors (bottom → top).
    pub layers: Vec<Descriptor>,
}

impl ImageManifest {
    /// Parse an image manifest from JSON bytes.
    pub fn parse(data: &[u8]) -> KernelResult<Self> {
        let root = json::parse(data)?;

        let schema_version = root.get_i64("schemaVersion").unwrap_or(2);
        let media_type = root
            .get_str("mediaType")
            .unwrap_or(MEDIA_TYPE_MANIFEST)
            .into();

        let config_val = root
            .get("config")
            .ok_or(KernelError::InvalidArgument)?;
        let config = Descriptor::from_json(config_val)?;

        let layers_json = root
            .get_array("layers")
            .ok_or(KernelError::InvalidArgument)?;

        if layers_json.len() > MAX_LAYERS {
            serial_println!(
                "[oci] Too many layers ({}, max {})",
                layers_json.len(),
                MAX_LAYERS
            );
            return Err(KernelError::InvalidArgument);
        }

        let mut layers = Vec::with_capacity(layers_json.len());
        for layer_val in layers_json {
            layers.push(Descriptor::from_json(layer_val)?);
        }

        Ok(Self {
            schema_version,
            media_type,
            config,
            layers,
        })
    }
}

/// Parsed OCI/Docker healthcheck configuration (`config.Healthcheck`).
///
/// Mirrors Docker's `HealthConfig`: a probe command plus timing/retry
/// parameters.  `test[0]` is the *type* token:
/// - `"NONE"` — explicitly disable any inherited healthcheck.
/// - `"CMD"` — `test[1..]` is the argv to exec directly.
/// - `"CMD-SHELL"` — `test[1]` is a single command line run via a shell.
///
/// Zero durations / retries mean "use the Docker default" (30s / 3), which the
/// `effective_*` accessors apply.  Durations are nanoseconds, matching the OCI
/// JSON encoding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HealthcheckConfig {
    /// The raw `Test` array, including the type token at index 0.
    pub test: Vec<String>,
    /// Probe interval (ns).  0 → default (30s).
    pub interval_ns: u64,
    /// Per-probe timeout (ns).  0 → default (30s).
    pub timeout_ns: u64,
    /// Grace period after container start before failures count (ns).  0 → none.
    pub start_period_ns: u64,
    /// Consecutive failures before the container is marked unhealthy.  0 →
    /// default (3).
    pub retries: u32,
}

impl HealthcheckConfig {
    /// Docker default probe interval / timeout when unset: 30 seconds.
    pub const DEFAULT_INTERVAL_NS: u64 = 30_000_000_000;
    /// Docker default retry count when unset.
    pub const DEFAULT_RETRIES: u32 = 3;

    /// `true` if this healthcheck is explicitly disabled (`Test: ["NONE"]`).
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        matches!(self.test.first().map(String::as_str), Some("NONE"))
    }

    /// `true` if the probe is a shell command (`Test: ["CMD-SHELL", ...]`)
    /// rather than a direct exec (`Test: ["CMD", ...]`).
    #[must_use]
    pub fn is_shell(&self) -> bool {
        matches!(self.test.first().map(String::as_str), Some("CMD-SHELL"))
    }

    /// The probe arguments (everything after the type token).  For `CMD` this
    /// is the argv; for `CMD-SHELL` it is a single-element slice holding the
    /// shell command line.  Empty when disabled or malformed.
    #[must_use]
    pub fn probe_args(&self) -> &[String] {
        self.test.get(1..).unwrap_or(&[])
    }

    /// `true` if a runnable probe command is present (a `CMD`/`CMD-SHELL` with
    /// at least one argument).  Disabled or empty healthchecks return `false`.
    #[must_use]
    pub fn is_runnable(&self) -> bool {
        !self.is_disabled() && !self.probe_args().is_empty()
    }

    /// Effective interval, applying the Docker default for an unset value.
    #[must_use]
    pub fn effective_interval_ns(&self) -> u64 {
        if self.interval_ns == 0 {
            Self::DEFAULT_INTERVAL_NS
        } else {
            self.interval_ns
        }
    }

    /// Effective timeout, applying the Docker default for an unset value.
    #[must_use]
    pub fn effective_timeout_ns(&self) -> u64 {
        if self.timeout_ns == 0 {
            Self::DEFAULT_INTERVAL_NS
        } else {
            self.timeout_ns
        }
    }

    /// Effective retry count, applying the Docker default for an unset value.
    #[must_use]
    pub fn effective_retries(&self) -> u32 {
        if self.retries == 0 {
            Self::DEFAULT_RETRIES
        } else {
            self.retries
        }
    }
}

/// Parsed OCI image configuration.
#[derive(Debug, Clone)]
pub struct ImageConfig {
    /// Architecture (e.g., "amd64").
    pub architecture: String,
    /// Operating system (e.g., "linux").
    pub os: String,
    /// Environment variables (`KEY=VALUE` strings).
    pub env: Vec<String>,
    /// Default command to run.
    pub cmd: Vec<String>,
    /// Entrypoint (prepended to cmd).
    pub entrypoint: Vec<String>,
    /// Working directory.
    pub working_dir: String,
    /// Exposed ports (keys from `ExposedPorts` object).
    pub exposed_ports: Vec<String>,
    /// User to run as.
    pub user: String,
    /// Labels (key-value metadata).
    pub labels: Vec<(String, String)>,
    /// Volume mount points (keys from `Volumes`).
    pub volumes: Vec<String>,
    /// Stop signal (`StopSignal`), e.g. `"SIGTERM"`.
    pub stop_signal: String,
    /// Shell prefix for shell-form commands (`Shell`).
    pub shell: Vec<String>,
    /// Deferred `ONBUILD` triggers (`OnBuild`).
    pub onbuild: Vec<String>,
    /// Layer diff-ids (sha256 of uncompressed tar, in layer order).
    pub diff_ids: Vec<String>,
    /// Build history (OCI config `history[]`), oldest first.
    pub history: Vec<HistoryEntry>,
    /// Container healthcheck (`config.Healthcheck`), if present.
    pub healthcheck: Option<HealthcheckConfig>,
}

impl ImageConfig {
    /// Parse an image config from JSON bytes.
    pub fn parse(data: &[u8]) -> KernelResult<Self> {
        if data.len() > MAX_CONFIG_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let root = json::parse(data)?;

        let architecture = root
            .get_str("architecture")
            .unwrap_or("amd64")
            .into();
        let os = root.get_str("os").unwrap_or("linux").into();

        // Runtime config is nested under "config" key.
        let cfg = root.get("config");

        let env = Self::parse_string_array(
            cfg.and_then(|c| c.get_array("Env")),
        );
        let cmd = Self::parse_string_array(
            cfg.and_then(|c| c.get_array("Cmd")),
        );
        let entrypoint = Self::parse_string_array(
            cfg.and_then(|c| c.get_array("Entrypoint")),
        );
        let working_dir = cfg
            .and_then(|c| c.get_str("WorkingDir"))
            .unwrap_or("")
            .into();
        let user = cfg
            .and_then(|c| c.get_str("User"))
            .unwrap_or("")
            .into();

        // Exposed ports: object keys like "8080/tcp".
        let exposed_ports = match cfg.and_then(|c| c.get("ExposedPorts")) {
            Some(JsonValue::Object(entries)) => {
                entries.iter().map(|(k, _)| k.clone()).collect()
            }
            _ => Vec::new(),
        };

        // Labels.
        let labels = match cfg.and_then(|c| c.get("Labels")) {
            Some(JsonValue::Object(entries)) => {
                entries
                    .iter()
                    .filter_map(|(k, v)| {
                        v.as_str().map(|s| (k.clone(), String::from(s)))
                    })
                    .collect()
            }
            _ => Vec::new(),
        };

        // Volume mount points (object keys, like ExposedPorts).
        let volumes = match cfg.and_then(|c| c.get("Volumes")) {
            Some(JsonValue::Object(entries)) => {
                entries.iter().map(|(k, _)| k.clone()).collect()
            }
            _ => Vec::new(),
        };

        // Stop signal, shell prefix, and ONBUILD triggers.
        let stop_signal = cfg
            .and_then(|c| c.get_str("StopSignal"))
            .unwrap_or("")
            .into();
        let shell = Self::parse_string_array(cfg.and_then(|c| c.get_array("Shell")));
        let onbuild = Self::parse_string_array(cfg.and_then(|c| c.get_array("OnBuild")));

        // Healthcheck (`config.Healthcheck`): a probe command plus timing.
        // Present-but-empty is preserved (a `Test: ["NONE"]` disable is
        // meaningful); absent → None.
        let healthcheck = cfg.and_then(|c| c.get("Healthcheck")).map(|hc| {
            let ns = |key: &str| -> u64 {
                hc.get(key)
                    .and_then(JsonValue::as_i64)
                    .and_then(|n| u64::try_from(n).ok())
                    .unwrap_or(0)
            };
            let retries = hc
                .get("Retries")
                .and_then(JsonValue::as_i64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0);
            HealthcheckConfig {
                test: Self::parse_string_array(hc.get_array("Test")),
                interval_ns: ns("Interval"),
                timeout_ns: ns("Timeout"),
                start_period_ns: ns("StartPeriod"),
                retries,
            }
        });

        // Rootfs diff-ids.
        let rootfs = root.get("rootfs");
        let diff_ids = Self::parse_string_array(
            rootfs.and_then(|r| r.get_array("diff_ids")),
        );

        // Build history (top-level `history[]`); optional.
        let history = match root.get_array("history") {
            Some(items) => items
                .iter()
                .map(|e| HistoryEntry {
                    created_by: e.get_str("created_by").unwrap_or("").into(),
                    empty_layer: matches!(e.get("empty_layer"), Some(JsonValue::Bool(true))),
                })
                .collect(),
            None => Vec::new(),
        };

        Ok(Self {
            architecture,
            os,
            env,
            cmd,
            entrypoint,
            working_dir,
            exposed_ports,
            user,
            labels,
            volumes,
            stop_signal,
            shell,
            onbuild,
            diff_ids,
            history,
            healthcheck,
        })
    }

    /// Helper: parse a JSON array of strings.
    fn parse_string_array(arr: Option<&[JsonValue]>) -> Vec<String> {
        match arr {
            Some(items) => items
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Build the full command line: entrypoint + cmd.
    #[must_use]
    pub fn command(&self) -> Vec<String> {
        let mut args = self.entrypoint.clone();
        args.extend(self.cmd.iter().cloned());
        args
    }

    /// Build the effective command line applying Docker's ENTRYPOINT/CMD
    /// override rules, returning `ENTRYPOINT ++ CMD`.
    ///
    /// * `entrypoint_override`:
    ///   - `None` keeps the image's ENTRYPOINT.
    ///   - `Some("")` clears the ENTRYPOINT (`docker run --entrypoint ""`).
    ///   - `Some(exe)` replaces the ENTRYPOINT with a single token.
    ///
    ///   In addition, supplying *any* `--entrypoint` (even an empty one) drops
    ///   the image's default CMD — only `cmd_override` can then supply a CMD,
    ///   matching Docker.
    /// * `cmd_override`:
    ///   - empty keeps the image's CMD (unless cleared by `--entrypoint`).
    ///   - non-empty replaces the CMD entirely (the trailing `IMAGE CMD...`
    ///     tokens), while the ENTRYPOINT is preserved.
    #[must_use]
    pub fn effective_command(
        &self,
        entrypoint_override: Option<&str>,
        cmd_override: &[&str],
    ) -> Vec<String> {
        let mut command: Vec<String> = match entrypoint_override {
            Some("") => Vec::new(),
            Some(ep) => alloc::vec![String::from(ep)],
            None => self.entrypoint.clone(),
        };
        if !cmd_override.is_empty() {
            command.extend(cmd_override.iter().map(|s| String::from(*s)));
        } else if entrypoint_override.is_none() {
            command.extend(self.cmd.iter().cloned());
        }
        command
    }
}

/// Extract the key (bytes before the first `=`) of an environment entry.
///
/// Returns the whole entry if there is no `=`. Used to deduplicate
/// environment variables by key when merging override sources.
#[must_use]
pub fn env_entry_key(entry: &[u8]) -> &[u8] {
    match entry.iter().position(|&b| b == b'=') {
        Some(eq) => entry.get(..eq).unwrap_or(entry),
        None => entry,
    }
}

/// Outcome of parsing a Docker-style `--env-file`.
pub struct EnvFileParse {
    /// Valid `KEY=value` entries, in file order, as raw bytes (env values
    /// may contain non-UTF-8 data, which must not be corrupted).
    pub entries: Vec<Vec<u8>>,
    /// 1-based line numbers that were rejected (no `=` or empty key), so
    /// the caller can report them. Blank and `#`-comment lines are *not*
    /// reported — they are silently skipped per Docker semantics.
    pub rejected_lines: Vec<usize>,
}

/// Parse the contents of a Docker-style `--env-file` into environment
/// entries.
///
/// Each line is treated as raw bytes (`KEY=value`). Blank lines and lines
/// whose first non-whitespace byte is `#` are ignored. A line without `=`
/// or with an empty key is rejected (its 1-based line number is recorded
/// in [`EnvFileParse::rejected_lines`]) — unlike Docker, a bare `KEY`
/// cannot inherit a value because a container has no host environment.
/// Surrounding ASCII whitespace is trimmed from each line.
#[must_use]
pub fn parse_env_file(bytes: &[u8]) -> EnvFileParse {
    let mut entries: Vec<Vec<u8>> = Vec::new();
    let mut rejected_lines: Vec<usize> = Vec::new();
    for (n, raw) in bytes.split(|&b| b == b'\n').enumerate() {
        let line = raw.trim_ascii();
        if line.is_empty() || line.first() == Some(&b'#') {
            continue;
        }
        match line.iter().position(|&b| b == b'=') {
            Some(eq) if !line.get(..eq).unwrap_or(&[]).trim_ascii().is_empty() => {
                entries.push(line.to_vec());
            }
            _ => rejected_lines.push(n.saturating_add(1)),
        }
    }
    EnvFileParse {
        entries,
        rejected_lines,
    }
}

// ---------------------------------------------------------------------------
// Digest verification
// ---------------------------------------------------------------------------

/// Verify a blob's content against its expected SHA-256 digest.
///
/// The digest should be in the format `sha256:<hex>`.
///
/// # Errors
///
/// - `InvalidArgument` if the digest format is unrecognised.
/// - `PermissionDenied` if the hash does not match (data tampering).
pub fn verify_digest(data: &[u8], expected_digest: &str) -> KernelResult<()> {
    let (algo, expected_hex) = expected_digest
        .split_once(':')
        .ok_or(KernelError::InvalidArgument)?;

    if algo != "sha256" {
        serial_println!("[oci] Unsupported digest algorithm: {}", algo);
        return Err(KernelError::NotSupported);
    }

    let hash = crate::crypto::sha256(data);

    // Convert hash to hex and compare.
    let mut hex_buf = [0u8; 64];
    for (i, &byte) in hash.iter().enumerate() {
        let hi = byte >> 4;
        let lo = byte & 0x0F;
        if let Some(slot) = hex_buf.get_mut(i.wrapping_mul(2)) {
            *slot = if hi < 10 { b'0' + hi } else { b'a' + hi - 10 };
        }
        if let Some(slot) = hex_buf.get_mut(i.wrapping_mul(2).wrapping_add(1)) {
            *slot = if lo < 10 { b'0' + lo } else { b'a' + lo - 10 };
        }
    }

    let computed_hex = core::str::from_utf8(&hex_buf)
        .map_err(|_| KernelError::InternalError)?;

    if computed_hex != expected_hex {
        serial_println!(
            "[oci] Digest mismatch: expected {}, got sha256:{}",
            expected_digest, computed_hex
        );
        return Err(KernelError::PermissionDenied);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Image loading — high-level API
// ---------------------------------------------------------------------------

/// Parsed OCI image ready for container creation.
///
/// Contains all the metadata needed to set up a container: the runtime
/// config (env, cmd, entrypoint) and the list of layer blobs to extract.
#[derive(Debug, Clone)]
pub struct OciImage {
    /// Image manifest.
    pub manifest: ImageManifest,
    /// Image config (runtime metadata).
    pub config: ImageConfig,
}

/// Load an OCI image from a directory on the VFS.
///
/// Reads `oci-layout`, `index.json`, the manifest, and the config.
/// Does NOT extract layers — that is done separately per the overlay
/// filesystem setup.
///
/// # Arguments
///
/// - `image_dir`: path to the OCI image directory (e.g., `/images/alpine`)
///
/// # Errors
///
/// - `NotFound` if required files are missing
/// - `InvalidArgument` if JSON is malformed
/// - `PermissionDenied` if digest verification fails
pub fn load_image(image_dir: &str) -> KernelResult<OciImage> {
    // Step 1: Verify oci-layout version.
    let layout_path = format!("{image_dir}/oci-layout");
    let layout_data = crate::fs::Vfs::read_file(&layout_path)?;
    let layout_json = json::parse(&layout_data)?;
    let version = layout_json
        .get_str("imageLayoutVersion")
        .ok_or(KernelError::InvalidArgument)?;
    if version != OCI_LAYOUT_VERSION {
        serial_println!(
            "[oci] Unsupported layout version: {} (expected {})",
            version, OCI_LAYOUT_VERSION
        );
        return Err(KernelError::NotSupported);
    }

    // Step 2: Parse index.json to find the manifest for our platform.
    let index_path = format!("{image_dir}/index.json");
    let index_data = crate::fs::Vfs::read_file(&index_path)?;
    let index = ImageIndex::parse(&index_data)?;

    let manifest_desc = index
        .find_manifest_for_host()
        .ok_or_else(|| {
            serial_println!("[oci] No manifest found for linux/amd64");
            KernelError::NotFound
        })?;

    // Step 3: Read and verify the manifest blob.
    let manifest_blob_path = manifest_desc.blob_path()
        .ok_or(KernelError::InvalidArgument)?;
    let manifest_path = format!("{image_dir}/{manifest_blob_path}");
    let manifest_data = crate::fs::Vfs::read_file(&manifest_path)?;

    verify_digest(&manifest_data, &manifest_desc.digest)?;

    let manifest = ImageManifest::parse(&manifest_data)?;

    // Step 4: Read and verify the config blob.
    let config_blob_path = manifest.config.blob_path()
        .ok_or(KernelError::InvalidArgument)?;
    let config_path = format!("{image_dir}/{config_blob_path}");
    let config_data = crate::fs::Vfs::read_file(&config_path)?;

    verify_digest(&config_data, &manifest.config.digest)?;

    let config = ImageConfig::parse(&config_data)?;

    serial_println!(
        "[oci] Loaded image: {} layers, arch={}, os={}, cmd={:?}",
        manifest.layers.len(),
        config.architecture,
        config.os,
        config.command(),
    );

    Ok(OciImage { manifest, config })
}

/// Extract a single layer from an OCI image into a target directory.
///
/// Reads the blob, verifies its digest, decompresses (if gzip), and
/// extracts the tar archive into `target_dir`.
///
/// # Arguments
///
/// - `image_dir`: path to the OCI image root
/// - `layer`: the layer descriptor from the manifest
/// - `target_dir`: VFS path to extract into (e.g., `/containers/xyz/layer0`)
pub fn extract_layer(
    image_dir: &str,
    layer: &Descriptor,
    target_dir: &str,
) -> KernelResult<u64> {
    let blob_path = layer.blob_path()
        .ok_or(KernelError::InvalidArgument)?;
    let full_path = format!("{image_dir}/{blob_path}");

    serial_println!(
        "[oci]   Extracting layer {} ({} bytes)...",
        layer.digest, layer.size
    );

    let blob_data = crate::fs::Vfs::read_file(&full_path)?;

    // Verify digest.
    verify_digest(&blob_data, &layer.digest)?;

    // Decompress if gzip.
    let tar_data = if layer.media_type == MEDIA_TYPE_LAYER_GZIP
        || layer.media_type == MEDIA_TYPE_DOCKER_LAYER
    {
        crate::fs::compress::gunzip(&blob_data)?
    } else {
        blob_data
    };

    // Extract tar into target_dir.
    let entries = crate::fs::tar::parse(&tar_data)?;
    let mut extracted: u64 = 0;

    for entry in &entries {
        let dest = format!("{target_dir}/{}", entry.name.trim_start_matches('/'));

        match entry.kind {
            crate::fs::tar::EntryKind::Directory => {
                // Create directory (ignore already-exists).
                let _ = crate::fs::Vfs::mkdir(&dest);
            }
            crate::fs::tar::EntryKind::File => {
                // Extract file data.
                let data_end = entry.data_offset.saturating_add(entry.size as usize);
                if data_end <= tar_data.len() {
                    let file_data = &tar_data[entry.data_offset..data_end];
                    crate::fs::Vfs::write_file(&dest, file_data)?;
                    extracted = extracted.saturating_add(1);
                }
            }
            crate::fs::tar::EntryKind::Symlink => {
                // Create symlink.
                let _ = crate::fs::Vfs::symlink(&dest, &entry.link_target);
            }
            crate::fs::tar::EntryKind::Other(_) => {
                // Skip unsupported entry types (devices, etc.).
            }
        }
    }

    serial_println!(
        "[oci]   Layer extracted: {} files, {} tar entries",
        extracted, entries.len()
    );

    Ok(extracted)
}

// ---------------------------------------------------------------------------
// Image authoring (writer) — the ungated foundation for `docker build`
// ---------------------------------------------------------------------------

/// A single file to place into a built image layer.
///
/// `path` is the archive-relative path (no leading `/`); parent directories are
/// synthesised automatically.  `mode` is the POSIX permission bits.
pub struct LayerFile {
    /// Archive-relative path, e.g. `"bin/hello"`.
    pub path: String,
    /// File contents.
    pub data: Vec<u8>,
    /// POSIX permission bits (e.g. `0o755`).
    pub mode: u32,
    /// Owning user id (tar `uid`; `COPY --chown`).
    pub uid: u32,
    /// Owning group id (tar `gid`; `COPY --chown`).
    pub gid: u32,
}

/// An explicit directory to create in a built image layer (e.g. Dockerfile
/// `WORKDIR`).  Parent directories are synthesised automatically by
/// [`build_layer_tar`], so only the leaf need be listed.
pub struct LayerDir {
    /// Archive-relative path, e.g. `"srv/app"` (no leading `/`).
    pub path: String,
    /// POSIX permission bits (Docker's `WORKDIR` uses `0o755`).
    pub mode: u32,
}

/// One layer of a built image — an ordered set of directories and files.
pub struct BuildLayer {
    /// Explicit directories created by this layer (e.g. `WORKDIR`).  These are
    /// emitted with their own mode; parent prefixes are synthesised at `0o755`.
    pub dirs: Vec<LayerDir>,
    /// Files contained in this layer (bottom-to-top layer order is the order
    /// of layers in [`ImageSpec::layers`]).
    pub files: Vec<LayerFile>,
}

/// One entry in an image's build history (OCI config `history[]`).
///
/// The OCI spec requires the number of `history` entries with
/// `empty_layer == false` to equal the number of layers, in order; layer-less
/// instructions (metadata-only, e.g. `ENV`/`LABEL`/`WORKDIR`) set
/// `empty_layer = true`.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The build step, conventionally the Dockerfile instruction text
    /// (e.g. `"COPY app /srv/app"`), stored as OCI `created_by`.
    pub created_by: String,
    /// `true` for metadata-only steps that produce no filesystem layer.
    pub empty_layer: bool,
}

/// A complete specification for authoring an OCI image on disk.
///
/// Mirrors the fields [`ImageConfig`] parses back, so an image written with
/// [`write_image`] round-trips through [`load_image`] with byte-identical
/// metadata.  This is the output target of a Dockerfile builder (`docker
/// build`): each instruction mutates the config or appends a layer.
pub struct ImageSpec {
    /// CPU architecture (e.g. `"amd64"`).
    pub architecture: String,
    /// Operating system (e.g. `"linux"`).
    pub os: String,
    /// `KEY=VALUE` environment entries (Dockerfile `ENV`).
    pub env: Vec<String>,
    /// Default command (Dockerfile `CMD`).
    pub cmd: Vec<String>,
    /// Entrypoint (Dockerfile `ENTRYPOINT`).
    pub entrypoint: Vec<String>,
    /// Working directory (Dockerfile `WORKDIR`).
    pub working_dir: String,
    /// User (Dockerfile `USER`).
    pub user: String,
    /// Exposed ports like `"8080/tcp"` (Dockerfile `EXPOSE`).
    pub exposed_ports: Vec<String>,
    /// Labels (Dockerfile `LABEL`).
    pub labels: Vec<(String, String)>,
    /// Mount-point volumes like `"/data"` (Dockerfile `VOLUME`).
    pub volumes: Vec<String>,
    /// Stop signal, e.g. `"SIGTERM"` (Dockerfile `STOPSIGNAL`).
    pub stop_signal: String,
    /// Shell prefix for shell-form commands (Dockerfile `SHELL`); empty means
    /// the default `["/bin/sh","-c"]`.
    pub shell: Vec<String>,
    /// Deferred `ONBUILD` trigger instructions (stored verbatim).
    pub onbuild: Vec<String>,
    /// Container healthcheck (Dockerfile `HEALTHCHECK`), if set.  `None` = no
    /// healthcheck; `Some` with `Test: ["NONE"]` = an explicit disable.
    pub healthcheck: Option<HealthcheckConfig>,
    /// Layers, bottom-to-top (Dockerfile `COPY`/`ADD` produce these).
    pub layers: Vec<BuildLayer>,
    /// Build history (OCI config `history[]`), oldest first.  Entries with
    /// `empty_layer == false` correspond 1:1, in order, to `layers`.
    pub history: Vec<HistoryEntry>,
}

impl ImageSpec {
    /// A minimal linux/amd64 spec with no config and no layers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            architecture: String::from("amd64"),
            os: String::from("linux"),
            env: Vec::new(),
            cmd: Vec::new(),
            entrypoint: Vec::new(),
            working_dir: String::new(),
            user: String::new(),
            exposed_ports: Vec::new(),
            labels: Vec::new(),
            volumes: Vec::new(),
            stop_signal: String::new(),
            shell: Vec::new(),
            onbuild: Vec::new(),
            healthcheck: None,
            layers: Vec::new(),
            history: Vec::new(),
        }
    }
}

impl Default for ImageSpec {
    fn default() -> Self {
        Self::new()
    }
}

/// Lowercase-hex encode a byte slice.
fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len().saturating_mul(2));
    for &b in bytes {
        let hi = b >> 4;
        let lo = b & 0x0F;
        s.push(char::from(if hi < 10 { b'0' + hi } else { b'a' + hi - 10 }));
        s.push(char::from(if lo < 10 { b'0' + lo } else { b'a' + lo - 10 }));
    }
    s
}

/// Compute the `sha256:<hex>` digest string of `data`.
fn sha256_digest(data: &[u8]) -> String {
    format!("sha256:{}", hex_lower(&crate::crypto::sha256(data)))
}

/// Append `s` as a JSON string literal (with surrounding quotes and escaping)
/// to `out`.  Escapes `"`, `\`, and the JSON-mandatory control characters; other
/// bytes are emitted as-is (our parser is byte-oriented, so non-ASCII passes
/// through unchanged).
fn push_json_string(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Render a `["a","b",...]` JSON array of strings.
fn json_string_array(items: &[String]) -> String {
    let mut out = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i != 0 {
            out.push(',');
        }
        push_json_string(&mut out, item);
    }
    out.push(']');
    out
}

/// All unique parent-directory prefixes of `path` (archive-relative), shallow to
/// deep — e.g. `"a/b/c"` → `["a", "a/b"]`.
fn parent_prefixes(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut acc = String::new();
    let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    // Every component except the last is a directory prefix.
    let dir_count = comps.len().saturating_sub(1);
    for comp in comps.into_iter().take(dir_count) {
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(comp);
        out.push(acc.clone());
    }
    out
}

/// Build one layer's *uncompressed* tar bytes from its files, synthesising the
/// directory entries every file's parents need (deduplicated, shallow-first).
fn build_layer_tar(layer: &BuildLayer) -> Vec<u8> {
    use crate::fs::tar::{EntryKind, TarWriteEntry};
    use alloc::collections::BTreeSet;

    // Collect all directory prefixes across all files and explicit dirs,
    // deduplicated and ordered shallow→deep (BTreeSet gives lexicographic
    // order; a parent always sorts before its children because it is a prefix
    // ending at a `/` boundary).  Explicit dirs (e.g. `WORKDIR`) carry their
    // own mode; synthesised parents default to `0o755`.
    let mut explicit: alloc::collections::BTreeMap<String, u32> =
        alloc::collections::BTreeMap::new();
    for d in &layer.dirs {
        let name = d.path.trim_matches('/');
        if name.is_empty() {
            continue;
        }
        explicit.insert(String::from(name), d.mode);
    }
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    for f in &layer.files {
        for d in parent_prefixes(&f.path) {
            dirs.insert(d);
        }
    }
    for name in explicit.keys() {
        for d in parent_prefixes(&format!("{name}/x")) {
            dirs.insert(d);
        }
        dirs.insert(name.clone());
    }

    let mut entries: Vec<TarWriteEntry> = Vec::new();
    for d in dirs {
        let mode = explicit.get(&d).copied().unwrap_or(0o755);
        entries.push(TarWriteEntry {
            name: format!("{d}/"),
            data: Vec::new(),
            kind: EntryKind::Directory,
            link_target: String::new(),
            mode,
            uid: 0,
            gid: 0,
            mtime: 0,
        });
    }
    for f in &layer.files {
        entries.push(TarWriteEntry {
            name: f.path.clone(),
            data: f.data.clone(),
            kind: EntryKind::File,
            link_target: String::new(),
            mode: f.mode,
            uid: f.uid,
            gid: f.gid,
            mtime: 0,
        });
    }
    crate::fs::tar::create(&entries)
}

/// Serialise the image config JSON (the blob [`ImageConfig::parse`] reads back).
fn build_config_json(spec: &ImageSpec, diff_ids: &[String]) -> String {
    let mut out = String::from("{");
    out.push_str("\"architecture\":");
    push_json_string(&mut out, &spec.architecture);
    out.push_str(",\"os\":");
    push_json_string(&mut out, &spec.os);
    out.push_str(",\"config\":{");
    out.push_str("\"Env\":");
    out.push_str(&json_string_array(&spec.env));
    out.push_str(",\"Cmd\":");
    out.push_str(&json_string_array(&spec.cmd));
    out.push_str(",\"Entrypoint\":");
    out.push_str(&json_string_array(&spec.entrypoint));
    out.push_str(",\"WorkingDir\":");
    push_json_string(&mut out, &spec.working_dir);
    out.push_str(",\"User\":");
    push_json_string(&mut out, &spec.user);
    out.push_str(",\"ExposedPorts\":{");
    for (i, p) in spec.exposed_ports.iter().enumerate() {
        if i != 0 {
            out.push(',');
        }
        push_json_string(&mut out, p);
        out.push_str(":{}");
    }
    out.push_str("},\"Labels\":{");
    for (i, (k, v)) in spec.labels.iter().enumerate() {
        if i != 0 {
            out.push(',');
        }
        push_json_string(&mut out, k);
        out.push(':');
        push_json_string(&mut out, v);
    }
    out.push('}'); // close Labels
    // VOLUME → Volumes object (keyed by mount path, empty-object values).
    if !spec.volumes.is_empty() {
        out.push_str(",\"Volumes\":{");
        for (i, v) in spec.volumes.iter().enumerate() {
            if i != 0 {
                out.push(',');
            }
            push_json_string(&mut out, v);
            out.push_str(":{}");
        }
        out.push('}');
    }
    // STOPSIGNAL → StopSignal string.
    if !spec.stop_signal.is_empty() {
        out.push_str(",\"StopSignal\":");
        push_json_string(&mut out, &spec.stop_signal);
    }
    // SHELL → Shell array.
    if !spec.shell.is_empty() {
        out.push_str(",\"Shell\":");
        out.push_str(&json_string_array(&spec.shell));
    }
    // ONBUILD → OnBuild array.
    if !spec.onbuild.is_empty() {
        out.push_str(",\"OnBuild\":");
        out.push_str(&json_string_array(&spec.onbuild));
    }
    // HEALTHCHECK → Healthcheck object (Test array + ns durations + retries).
    // Durations/retries are emitted only when non-zero (0 means "Docker
    // default", which the loader's `effective_*` accessors reapply).
    if let Some(hc) = &spec.healthcheck {
        out.push_str(",\"Healthcheck\":{\"Test\":");
        out.push_str(&json_string_array(&hc.test));
        if hc.interval_ns != 0 {
            out.push_str(&format!(",\"Interval\":{}", hc.interval_ns));
        }
        if hc.timeout_ns != 0 {
            out.push_str(&format!(",\"Timeout\":{}", hc.timeout_ns));
        }
        if hc.start_period_ns != 0 {
            out.push_str(&format!(",\"StartPeriod\":{}", hc.start_period_ns));
        }
        if hc.retries != 0 {
            out.push_str(&format!(",\"Retries\":{}", hc.retries));
        }
        out.push('}');
    }
    out.push('}'); // close config
    out.push_str(",\"rootfs\":{\"type\":\"layers\",\"diff_ids\":");
    out.push_str(&json_string_array(diff_ids));
    out.push('}');
    // Build history (optional; emitted only when the builder recorded steps).
    if !spec.history.is_empty() {
        out.push_str(",\"history\":[");
        for (i, h) in spec.history.iter().enumerate() {
            if i != 0 {
                out.push(',');
            }
            out.push_str("{\"created_by\":");
            push_json_string(&mut out, &h.created_by);
            if h.empty_layer {
                out.push_str(",\"empty_layer\":true");
            }
            out.push('}');
        }
        out.push(']');
    }
    out.push('}');
    out
}

/// Render a blob descriptor object `{"mediaType":..,"digest":..,"size":N}`.
fn descriptor_json(media_type: &str, digest: &str, size: u64) -> String {
    let mut out = String::from("{\"mediaType\":");
    push_json_string(&mut out, media_type);
    out.push_str(",\"digest\":");
    push_json_string(&mut out, digest);
    out.push_str(",\"size\":");
    out.push_str(&format!("{size}"));
    out.push('}');
    out
}

/// Write a blob into `image_dir/blobs/sha256/<hex>`; returns its `Descriptor`.
fn write_blob(image_dir: &str, media_type: &str, data: &[u8]) -> KernelResult<Descriptor> {
    let digest = sha256_digest(data);
    let (_, hex) = digest.split_once(':').ok_or(KernelError::InternalError)?;
    let path = format!("{image_dir}/blobs/sha256/{hex}");
    crate::fs::Vfs::write_file(&path, data)?;
    Ok(Descriptor {
        media_type: String::from(media_type),
        digest,
        size: data.len() as u64,
    })
}

/// Author a complete OCI image directory from `spec`.
///
/// Writes uncompressed→gzipped layer blobs (with correct content digests and
/// uncompressed-tar `diff_id`s), the image config, the manifest, `index.json`
/// and `oci-layout`, all in standard OCI layout under `dest_dir`.  The result is
/// immediately loadable by [`load_image`] and runnable by the container runtime,
/// so this is the build target for a Dockerfile builder.
///
/// Returns the manifest [`Descriptor`] (as referenced by `index.json`).
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if `dest_dir` is empty or contains NUL.
/// - Any VFS error while creating directories or writing blobs.
pub fn write_image(dest_dir: &str, spec: &ImageSpec) -> KernelResult<Descriptor> {
    if dest_dir.is_empty() || dest_dir.contains('\0') {
        return Err(KernelError::InvalidArgument);
    }
    let dest = dest_dir.trim_end_matches('/');
    if dest.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if spec.layers.len() > MAX_LAYERS {
        return Err(KernelError::InvalidArgument);
    }

    create_layout_skeleton(dest);

    // Layers: tar → diff_id (uncompressed) → gzip → blob digest.
    let mut layer_descs: Vec<Descriptor> = Vec::with_capacity(spec.layers.len());
    let mut diff_ids: Vec<String> = Vec::with_capacity(spec.layers.len());
    for layer in &spec.layers {
        let tar = build_layer_tar(layer);
        diff_ids.push(sha256_digest(&tar));
        let gz = crate::fs::compress::gzip(&tar);
        layer_descs.push(write_blob(dest, MEDIA_TYPE_LAYER_GZIP, &gz)?);
    }

    finish_image(dest, spec, &layer_descs, &diff_ids)
}

/// Create the standard OCI layout skeleton (`blobs/sha256`) under `dest`.
/// Idempotent — pre-existing directories are fine.
fn create_layout_skeleton(dest: &str) {
    use crate::fs::Vfs;
    let _ = Vfs::mkdir(dest);
    let _ = Vfs::mkdir(&format!("{dest}/blobs"));
    let _ = Vfs::mkdir(&format!("{dest}/blobs/sha256"));
}

/// Assemble config + manifest + `index.json` + `oci-layout` from
/// already-written layer blobs.  `layer_descs` and `diff_ids` are parallel,
/// bottom-to-top.  Shared by [`write_image`] and [`build_image`].
///
/// Returns the manifest [`Descriptor`] (as referenced by `index.json`).
fn finish_image(
    dest: &str,
    spec: &ImageSpec,
    layer_descs: &[Descriptor],
    diff_ids: &[String],
) -> KernelResult<Descriptor> {
    use crate::fs::Vfs;

    // Config blob.
    let config_json = build_config_json(spec, diff_ids);
    let config_desc = write_blob(dest, MEDIA_TYPE_CONFIG, config_json.as_bytes())?;

    // Manifest blob.
    let mut manifest = String::from("{\"schemaVersion\":2,\"mediaType\":");
    push_json_string(&mut manifest, MEDIA_TYPE_MANIFEST);
    manifest.push_str(",\"config\":");
    manifest.push_str(&descriptor_json(
        &config_desc.media_type,
        &config_desc.digest,
        config_desc.size,
    ));
    manifest.push_str(",\"layers\":[");
    for (i, d) in layer_descs.iter().enumerate() {
        if i != 0 {
            manifest.push(',');
        }
        manifest.push_str(&descriptor_json(&d.media_type, &d.digest, d.size));
    }
    manifest.push_str("]}");
    let manifest_desc = write_blob(dest, MEDIA_TYPE_MANIFEST, manifest.as_bytes())?;

    // index.json (references the manifest, with a platform).
    let mut index = String::from("{\"schemaVersion\":2,\"mediaType\":");
    push_json_string(&mut index, MEDIA_TYPE_INDEX);
    index.push_str(",\"manifests\":[{\"mediaType\":");
    push_json_string(&mut index, &manifest_desc.media_type);
    index.push_str(",\"digest\":");
    push_json_string(&mut index, &manifest_desc.digest);
    index.push_str(",\"size\":");
    index.push_str(&format!("{}", manifest_desc.size));
    index.push_str(",\"platform\":{\"architecture\":");
    push_json_string(&mut index, &spec.architecture);
    index.push_str(",\"os\":");
    push_json_string(&mut index, &spec.os);
    index.push_str("}}]}");
    Vfs::write_file(&format!("{dest}/index.json"), index.as_bytes())?;

    // oci-layout marker.
    Vfs::write_file(
        &format!("{dest}/oci-layout"),
        format!("{{\"imageLayoutVersion\":\"{OCI_LAYOUT_VERSION}\"}}").as_bytes(),
    )?;

    Ok(manifest_desc)
}

// ---------------------------------------------------------------------------
// Named image store (`docker images` / `tag` / `rmi`)
// ---------------------------------------------------------------------------

/// Root of the local named-image store — a single OCI layout whose
/// `index.json` holds one annotated manifest descriptor per `name:tag`
/// (`org.opencontainers.image.ref.name`).  Mirrors Docker's local image store
/// so `build -t name:tag`, `images`, `tag`, and `rmi` work by name rather than
/// by on-disk directory path.
pub const STORE_DIR: &str = "/var/lib/images";

/// OCI annotation key that carries an image's `name:tag` reference.
const ANNOTATION_REF_NAME: &str = "org.opencontainers.image.ref.name";

/// One tagged image in the store (a row of `docker images`).
pub struct StoredImage {
    /// The `name:tag` reference.
    pub reference: String,
    /// The manifest digest (`sha256:…`).
    pub digest: String,
    /// The manifest blob size in bytes.
    pub size: u64,
}

/// A parsed store index entry: a manifest descriptor plus its `ref.name`.
struct StoreEntry {
    media_type: String,
    digest: String,
    size: u64,
    os: String,
    architecture: String,
    reference: String,
}

/// Normalise an image reference to `name:tag`, defaulting the tag to `latest`.
/// A digest-form reference (`name@sha256:…`) is returned unchanged; a `:` that
/// is part of a `registry:port` host (i.e. followed by a `/`) is not mistaken
/// for a tag separator.
fn normalize_ref(reference: &str) -> String {
    let reference = reference.trim();
    if reference.contains('@') {
        return String::from(reference);
    }
    match reference.rsplit_once(':') {
        Some((_, tag)) if !tag.is_empty() && !tag.contains('/') => String::from(reference),
        _ => format!("{reference}:latest"),
    }
}

/// Read and parse the store `index.json` into entries (empty if absent).
fn store_read_index() -> KernelResult<Vec<StoreEntry>> {
    read_index_at(STORE_DIR)
}

/// Read and parse the `index.json` of an arbitrary OCI layout `dir` into
/// [`StoreEntry`] rows (each manifest descriptor + its `ref.name` annotation).
/// Returns an empty vec if the index is absent.
fn read_index_at(dir: &str) -> KernelResult<Vec<StoreEntry>> {
    use crate::fs::Vfs;
    let data = match Vfs::read_file(&format!("{}/index.json", dir.trim_end_matches('/'))) {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };
    let root = json::parse(&data)?;
    let Some(manifests) = root.get_array("manifests") else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for e in manifests {
        let Some(digest) = e.get_str("digest") else {
            continue;
        };
        let media_type = e.get_str("mediaType").unwrap_or(MEDIA_TYPE_MANIFEST);
        let size = e.get_i64("size").unwrap_or(0).max(0) as u64;
        let (os, architecture) = match e.get("platform") {
            Some(p) => (
                String::from(p.get_str("os").unwrap_or("linux")),
                String::from(p.get_str("architecture").unwrap_or("amd64")),
            ),
            None => (String::from("linux"), String::from("amd64")),
        };
        let reference = e
            .get("annotations")
            .and_then(|a| a.get_str(ANNOTATION_REF_NAME))
            .map(String::from)
            .unwrap_or_default();
        out.push(StoreEntry {
            media_type: String::from(media_type),
            digest: String::from(digest),
            size,
            os,
            architecture,
            reference,
        });
    }
    Ok(out)
}

/// Serialise `entries` to an OCI multi-manifest `index.json` string (each with
/// its platform + `ref.name` annotation).
fn serialize_index(entries: &[StoreEntry]) -> String {
    let mut index = String::from("{\"schemaVersion\":2,\"mediaType\":");
    push_json_string(&mut index, MEDIA_TYPE_INDEX);
    index.push_str(",\"manifests\":[");
    for (i, e) in entries.iter().enumerate() {
        if i != 0 {
            index.push(',');
        }
        index.push_str("{\"mediaType\":");
        push_json_string(&mut index, &e.media_type);
        index.push_str(",\"digest\":");
        push_json_string(&mut index, &e.digest);
        index.push_str(",\"size\":");
        index.push_str(&format!("{}", e.size));
        index.push_str(",\"platform\":{\"architecture\":");
        push_json_string(&mut index, &e.architecture);
        index.push_str(",\"os\":");
        push_json_string(&mut index, &e.os);
        index.push_str("},\"annotations\":{");
        push_json_string(&mut index, ANNOTATION_REF_NAME);
        index.push(':');
        push_json_string(&mut index, &e.reference);
        index.push_str("}}");
    }
    index.push_str("]}");
    index
}

/// Write `entries` as the `index.json` of the OCI layout at `dir` (and refresh
/// its `oci-layout` marker).
fn write_index_at(dir: &str, entries: &[StoreEntry]) -> KernelResult<()> {
    use crate::fs::Vfs;
    let dir = dir.trim_end_matches('/');
    Vfs::write_file(&format!("{dir}/index.json"), serialize_index(entries).as_bytes())?;
    Vfs::write_file(
        &format!("{dir}/oci-layout"),
        b"{\"imageLayoutVersion\":\"1.0.0\"}",
    )?;
    Ok(())
}

/// Serialise `entries` to the store `index.json` (and refresh the `oci-layout`
/// marker).
fn store_write_index(entries: &[StoreEntry]) -> KernelResult<()> {
    write_index_at(STORE_DIR, entries)
}

/// Copy every blob under `src/blobs/sha256` into `dst/blobs/sha256`
/// (content-addressed, so identical names are simply re-written).
fn copy_all_blobs(src: &str, dst: &str) -> KernelResult<()> {
    use crate::fs::vfs::{EntryType, Vfs};
    let src_blobs = format!("{}/blobs/sha256", src.trim_end_matches('/'));
    let dst_blobs = format!("{}/blobs/sha256", dst.trim_end_matches('/'));
    for de in Vfs::readdir(&src_blobs)? {
        if de.name == "." || de.name == ".." || de.entry_type != EntryType::File {
            continue;
        }
        let data = Vfs::read_file(&format!("{src_blobs}/{}", de.name))?;
        Vfs::write_file(&format!("{dst_blobs}/{}", de.name), &data)?;
    }
    Ok(())
}

/// Copy the image at `src_image_dir` into the store and tag it `reference`
/// (`name`, defaulting the tag to `latest`).  Any prior tag with the same
/// reference is replaced.  Returns the tagged manifest digest.
///
/// # Errors
/// The source directory must be a valid OCI layout; propagates VFS errors.
pub fn store_tag_from_dir(src_image_dir: &str, reference: &str) -> KernelResult<String> {
    use crate::fs::Vfs;
    let reference = normalize_ref(reference);
    create_layout_skeleton(STORE_DIR);

    let src = src_image_dir.trim_end_matches('/');
    let src_index = Vfs::read_file(&format!("{src}/index.json"))?;
    let idx = ImageIndex::parse(&src_index)?;
    let man = idx.find_manifest_for_host().ok_or(KernelError::NotFound)?;
    let media_type = if man.media_type.is_empty() {
        String::from(MEDIA_TYPE_MANIFEST)
    } else {
        man.media_type.clone()
    };
    let digest = man.digest.clone();
    let size = man.size;

    copy_all_blobs(src, STORE_DIR)?;

    let mut entries = store_read_index()?;
    entries.retain(|e| e.reference != reference);
    entries.push(StoreEntry {
        media_type,
        digest: digest.clone(),
        size,
        os: String::from("linux"),
        architecture: String::from("amd64"),
        reference,
    });
    store_write_index(&entries)?;
    Ok(digest)
}

/// Add a second `reference` pointing at the same manifest as an existing
/// `source` reference (`docker tag src dst`), without recopying blobs.
///
/// # Errors
/// `NotFound` if `source` is not a tagged image in the store.
pub fn store_add_tag(source: &str, reference: &str) -> KernelResult<()> {
    let source = normalize_ref(source);
    let reference = normalize_ref(reference);
    let mut entries = store_read_index()?;
    let src = entries
        .iter()
        .find(|e| e.reference == source)
        .ok_or(KernelError::NotFound)?;
    let new = StoreEntry {
        media_type: src.media_type.clone(),
        digest: src.digest.clone(),
        size: src.size,
        os: src.os.clone(),
        architecture: src.architecture.clone(),
        reference: reference.clone(),
    };
    entries.retain(|e| e.reference != reference);
    entries.push(new);
    store_write_index(&entries)
}

/// Resolve `reference` (`name:tag`, default tag `latest`) to its manifest
/// digest in the store.
///
/// # Errors
/// `NotFound` if no tag matches.
pub fn store_resolve(reference: &str) -> KernelResult<String> {
    let reference = normalize_ref(reference);
    store_read_index()?
        .into_iter()
        .find(|e| e.reference == reference)
        .map(|e| e.digest)
        .ok_or(KernelError::NotFound)
}

/// List all tagged images in the store (rows of `docker images`).
///
/// # Errors
/// Propagates a malformed-index parse error.
pub fn store_list() -> KernelResult<Vec<StoredImage>> {
    Ok(store_read_index()?
        .into_iter()
        .map(|e| StoredImage {
            reference: e.reference,
            digest: e.digest,
            size: e.size,
        })
        .collect())
}

/// Remove the `reference` tag from the store, garbage-collecting any blob no
/// longer reachable from a remaining manifest.
///
/// # Errors
/// `NotFound` if the tag is absent.
pub fn store_remove(reference: &str) -> KernelResult<()> {
    use crate::fs::vfs::{EntryType, Vfs};
    let reference = normalize_ref(reference);
    let mut entries = store_read_index()?;
    let before = entries.len();
    entries.retain(|e| e.reference != reference);
    if entries.len() == before {
        return Err(KernelError::NotFound);
    }
    store_write_index(&entries)?;

    // GC: keep every blob reachable from a surviving manifest (the manifest
    // blob itself + its config + layers); delete the rest.
    let mut keep: alloc::collections::BTreeSet<String> = alloc::collections::BTreeSet::new();
    for e in &entries {
        let Some((_, hex)) = e.digest.split_once(':') else {
            continue;
        };
        keep.insert(String::from(hex));
        if let Ok(data) = Vfs::read_file(&format!("{STORE_DIR}/blobs/sha256/{hex}")) {
            collect_manifest_blob_hexes(&data, &mut keep);
        }
    }
    let blobs = format!("{STORE_DIR}/blobs/sha256");
    if let Ok(list) = Vfs::readdir(&blobs) {
        for de in list {
            if de.name == "." || de.name == ".." || de.entry_type != EntryType::File {
                continue;
            }
            if !keep.contains(&de.name) {
                let _ = Vfs::remove(&format!("{blobs}/{}", de.name));
            }
        }
    }
    Ok(())
}

/// Add the config + layer blob hex digests referenced by a manifest JSON blob
/// to `keep` (used by the store GC).
fn collect_manifest_blob_hexes(
    manifest_json: &[u8],
    keep: &mut alloc::collections::BTreeSet<String>,
) {
    let Ok(root) = json::parse(manifest_json) else {
        return;
    };
    if let Some(d) = root.get("config").and_then(|c| c.get_str("digest")) {
        if let Some((_, hex)) = d.split_once(':') {
            keep.insert(String::from(hex));
        }
    }
    if let Some(layers) = root.get_array("layers") {
        for l in layers {
            if let Some((_, hex)) = l.get_str("digest").and_then(|d| d.split_once(':')) {
                keep.insert(String::from(hex));
            }
        }
    }
}

/// Copy the blob identified by `digest` (`algo:hex`) from the `src` OCI layout
/// into the `dst` layout's content-addressed blob pool.
fn copy_blob_by_digest(src: &str, dst: &str, digest: &str) -> KernelResult<()> {
    use crate::fs::Vfs;
    let (_, hex) = digest.split_once(':').ok_or(KernelError::InvalidArgument)?;
    let data = Vfs::read_file(&format!("{}/blobs/sha256/{hex}", src.trim_end_matches('/')))?;
    Vfs::write_file(&format!("{}/blobs/sha256/{hex}", dst.trim_end_matches('/')), &data)?;
    Ok(())
}

/// Export a single stored image `reference` into a standalone **single-manifest**
/// OCI layout at `dest_dir` — the form `oci save` / `docker save` bundles into a
/// tar.  Only that image's manifest, config, and layer blobs are copied (not the
/// whole shared store), and the dest `index.json` carries exactly one manifest
/// with the `ref.name` annotation preserved.
///
/// # Errors
/// `NotFound` if `reference` is not in the store; propagates VFS/parse errors.
pub fn store_export_ref(reference: &str, dest_dir: &str) -> KernelResult<()> {
    use crate::fs::Vfs;
    let reference = normalize_ref(reference);
    let entries = store_read_index()?;
    let entry = entries
        .iter()
        .find(|e| e.reference == reference)
        .ok_or(KernelError::NotFound)?;

    create_layout_skeleton(dest_dir);
    // Manifest blob, then the config + layer blobs it references.
    copy_blob_by_digest(STORE_DIR, dest_dir, &entry.digest)?;
    let (_, hex) = entry.digest.split_once(':').ok_or(KernelError::InvalidArgument)?;
    let manifest_data = Vfs::read_file(&format!("{STORE_DIR}/blobs/sha256/{hex}"))?;
    let manifest = ImageManifest::parse(&manifest_data)?;
    copy_blob_by_digest(STORE_DIR, dest_dir, &manifest.config.digest)?;
    for l in &manifest.layers {
        copy_blob_by_digest(STORE_DIR, dest_dir, &l.digest)?;
    }
    write_index_at(dest_dir, core::slice::from_ref(entry))
}

/// Import every annotated image from a standalone OCI layout `src_dir` into the
/// store (the inverse of [`store_export_ref`]; used by `oci load` / `docker
/// load`).  Blobs are copied into the shared pool and each `ref.name`-annotated
/// manifest becomes/refreshes a store tag.  Returns the tags added.
///
/// # Errors
/// Propagates VFS/parse errors.  A layout with no `ref.name` annotations yields
/// an empty list (nothing to name).
pub fn store_import_dir(src_dir: &str) -> KernelResult<Vec<String>> {
    create_layout_skeleton(STORE_DIR);
    let src = src_dir.trim_end_matches('/');
    let src_entries = read_index_at(src)?;
    copy_all_blobs(src, STORE_DIR)?;

    let mut store = store_read_index()?;
    let mut added = Vec::new();
    for e in src_entries {
        if e.reference.is_empty() {
            continue;
        }
        store.retain(|s| s.reference != e.reference);
        added.push(e.reference.clone());
        store.push(e);
    }
    store_write_index(&store)?;
    Ok(added)
}

/// Load an image from an OCI layout `dir` by an explicit manifest digest,
/// rather than by host-platform selection.  Used to pick one specific tagged
/// image out of the store's shared multi-manifest layout.
///
/// # Errors
/// Propagates VFS/parse errors; `InvalidArgument` if the digest is malformed.
fn load_manifest_by_digest(dir: &str, manifest_digest: &str) -> KernelResult<OciImage> {
    let (_, hex) = manifest_digest
        .split_once(':')
        .ok_or(KernelError::InvalidArgument)?;
    let manifest_data = crate::fs::Vfs::read_file(&format!("{dir}/blobs/sha256/{hex}"))?;
    verify_digest(&manifest_data, manifest_digest)?;
    let manifest = ImageManifest::parse(&manifest_data)?;

    let config_blob_path = manifest.config.blob_path().ok_or(KernelError::InvalidArgument)?;
    let config_data = crate::fs::Vfs::read_file(&format!("{dir}/{config_blob_path}"))?;
    verify_digest(&config_data, &manifest.config.digest)?;
    let config = ImageConfig::parse(&config_data)?;

    Ok(OciImage { manifest, config })
}

/// Resolve an image argument that is either an on-disk OCI-layout **directory**
/// or a named-store **reference** (`name:tag`, defaulting to `:latest`) into
/// the blob-source directory to extract from and the loaded image.
///
/// A path that exists as a valid OCI layout (has an `oci-layout` marker) is
/// treated as a directory; otherwise the argument is looked up in the named
/// store at [`STORE_DIR`], and the returned blob-source directory *is*
/// `STORE_DIR` (all store images share its content-addressed blob pool).
///
/// # Errors
/// `NotFound` if the argument is neither a valid layout directory nor a known
/// store reference; propagates VFS/parse errors otherwise.
pub fn resolve_image_source(arg: &str) -> KernelResult<(String, OciImage)> {
    let dir = arg.trim_end_matches('/');
    // A valid on-disk OCI layout is marked by its `oci-layout` file.
    if crate::fs::Vfs::metadata(&format!("{dir}/oci-layout")).is_ok() {
        let img = load_image(dir)?;
        return Ok((String::from(dir), img));
    }
    // Otherwise treat the argument as a store reference.
    let digest = store_resolve(arg)?;
    let img = load_manifest_by_digest(STORE_DIR, &digest)?;
    Ok((String::from(STORE_DIR), img))
}

/// Recursively collect the files and directories under `upper_dir` (a
/// container's overlay scratch layer) into a [`BuildLayer`], appending OCI
/// `.wh.`-prefixed whiteout markers for each deleted `whiteouts` path.  This is
/// the new layer captured by `commit` — the container's filesystem changes
/// relative to its read-only base image.
fn overlay_to_build_layer(upper_dir: &str, whiteouts: &[String]) -> KernelResult<BuildLayer> {
    use crate::fs::vfs::{EntryType, Vfs};
    let root = upper_dir.trim_end_matches('/');
    let mut dirs: Vec<LayerDir> = Vec::new();
    let mut files: Vec<LayerFile> = Vec::new();

    // Iterative DFS over the upper tree; `rel` is archive-relative (no slash).
    let mut stack: Vec<String> = alloc::vec![String::new()];
    while let Some(rel) = stack.pop() {
        let abs = if rel.is_empty() {
            String::from(root)
        } else {
            format!("{root}/{rel}")
        };
        for de in Vfs::readdir(&abs)? {
            if de.name == "." || de.name == ".." {
                continue;
            }
            let child_rel = if rel.is_empty() {
                de.name.clone()
            } else {
                format!("{rel}/{}", de.name)
            };
            let child_abs = format!("{abs}/{}", de.name);
            match de.entry_type {
                EntryType::Directory => {
                    let mode = Vfs::metadata(&child_abs)
                        .map(|m| u32::from(m.permissions))
                        .unwrap_or(0o755);
                    dirs.push(LayerDir { path: child_rel.clone(), mode });
                    stack.push(child_rel);
                }
                EntryType::File => {
                    let meta = Vfs::metadata(&child_abs)?;
                    let data = Vfs::read_file(&child_abs)?;
                    files.push(LayerFile {
                        path: child_rel,
                        data,
                        mode: u32::from(meta.permissions),
                        uid: 0,
                        gid: 0,
                    });
                }
                // Symlinks/other kinds are skipped (a documented `commit`
                // limitation, mirroring the COPY/ADD symlink handling).
                _ => {}
            }
        }
    }

    // OCI whiteouts: a deleted path `a/b/c` is recorded as an empty file
    // `a/b/.wh.c` so a consumer hides the corresponding lower-layer entry.
    for w in whiteouts {
        let norm = w.trim_start_matches('/');
        if norm.is_empty() {
            continue;
        }
        let (parent, base) = match norm.rsplit_once('/') {
            Some((p, b)) => (p, b),
            None => ("", norm),
        };
        let wh_path = if parent.is_empty() {
            format!(".wh.{base}")
        } else {
            format!("{parent}/.wh.{base}")
        };
        files.push(LayerFile {
            path: wh_path,
            data: Vec::new(),
            mode: 0,
            uid: 0,
            gid: 0,
        });
    }

    Ok(BuildLayer { dirs, files })
}

/// Author a new image = the `base` image (resolved from `base_source`, a dir or
/// store reference) plus one new layer capturing a container's filesystem
/// changes from its overlay `upper_dir` (added/changed files) and `whiteouts`
/// (deletions).  The base config (Env/Cmd/Entrypoint/…) and layers are carried
/// forward verbatim and a `commit`-style history entry is appended.  Written as
/// a standalone OCI layout at `dest_dir`; returns the manifest descriptor.
///
/// This is the engine behind `oci commit` / `docker commit` — **image
/// production** (a new image from a container's writes), distinct from the
/// native `container commit`, which clones a container.
///
/// # Errors
/// Propagates base-resolution and VFS/parse errors; `InvalidArgument` on a
/// base layer/diff_id mismatch or if the layer count exceeds the OCI cap.
pub fn commit_image(
    base_source: &str,
    upper_dir: &str,
    whiteouts: &[String],
    dest_dir: &str,
) -> KernelResult<Descriptor> {
    let (base_blob_dir, base) = resolve_image_source(base_source)?;

    // Reconstruct the build spec from the base image's config so the committed
    // image keeps the base's runtime metadata (Env/Cmd/Entrypoint/etc.).
    let mut spec = ImageSpec::new();
    spec.architecture = base.config.architecture.clone();
    spec.os = base.config.os.clone();
    spec.env = base.config.env.clone();
    spec.cmd = base.config.cmd.clone();
    spec.entrypoint = base.config.entrypoint.clone();
    spec.working_dir = base.config.working_dir.clone();
    spec.user = base.config.user.clone();
    spec.exposed_ports = base.config.exposed_ports.clone();
    spec.labels = base.config.labels.clone();
    spec.volumes = base.config.volumes.clone();
    spec.stop_signal = base.config.stop_signal.clone();
    spec.shell = base.config.shell.clone();
    spec.onbuild = base.config.onbuild.clone();
    spec.healthcheck = base.config.healthcheck.clone();
    spec.history = base.config.history.clone();

    create_layout_skeleton(dest_dir);

    // Carry base layers forward verbatim (copy blobs, reuse descriptors+diff_ids).
    let mut layer_descs = base.manifest.layers.clone();
    let mut diff_ids = base.config.diff_ids.clone();
    if layer_descs.len() != diff_ids.len() {
        return Err(KernelError::InvalidArgument);
    }
    for d in &layer_descs {
        copy_blob_by_digest(&base_blob_dir, dest_dir, &d.digest)?;
    }

    // Append the commit layer (the container's filesystem changes).
    let layer = overlay_to_build_layer(upper_dir, whiteouts)?;
    let tar = build_layer_tar(&layer);
    diff_ids.push(sha256_digest(&tar));
    let gz = crate::fs::compress::gzip(&tar);
    layer_descs.push(write_blob(dest_dir, MEDIA_TYPE_LAYER_GZIP, &gz)?);

    spec.history.push(HistoryEntry {
        created_by: String::from("/bin/sh -c #(nop) COMMIT"),
        empty_layer: false,
    });

    if layer_descs.len() > MAX_LAYERS {
        return Err(KernelError::InvalidArgument);
    }
    finish_image(dest_dir, &spec, &layer_descs, &diff_ids)
}

// ---------------------------------------------------------------------------
// Dockerfile builder (`docker build` / `oci build`)
// ---------------------------------------------------------------------------

/// Upper bound on instructions processed from one Dockerfile.
const MAX_BUILD_INSTRUCTIONS: usize = 4096;
/// Upper bound on files collected by a single COPY/ADD (runaway-recursion guard).
const MAX_COPY_FILES: usize = 100_000;
/// Upper bound on build stages (`FROM …`) in one multi-stage Dockerfile.
const MAX_BUILD_STAGES: usize = 128;

/// Failure modes of [`build_image`].
///
/// Kept distinct from a bare [`KernelError`] so the shell can print a precise,
/// Docker-style diagnostic — in particular, a `RUN` instruction executes the
/// real in-container exec (design-decisions.md §58), so its two failure modes
/// (could-not-launch vs. non-zero exit) are surfaced distinctly.
pub enum BuildError {
    /// A `RUN` command could not be launched at 1-based source `line` — e.g. the
    /// executable (or the shell for a shell-form `RUN`) is absent from the
    /// in-progress rootfs (a `FROM scratch` image has no `/bin/sh`), or the
    /// spawn failed. `msg` gives the specific cause.
    RunLaunch { line: usize, msg: String },
    /// A `RUN` command executed but exited non-zero (Docker aborts the build).
    RunFailed { line: usize, code: i32 },
    /// Malformed or unsupported instruction at 1-based source `line`.
    Parse { line: usize, msg: String },
    /// The first build instruction (after any leading `ARG`s) was not `FROM`.
    MissingFrom,
    /// A COPY/ADD source path did not exist in the build context.
    CopySourceMissing { src: String },
    /// Underlying kernel/VFS error.
    Kernel(KernelError),
}

impl BuildError {
    /// A human-readable, single-line description for the shell.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            BuildError::RunLaunch { line, msg } => {
                format!("line {line}: RUN could not launch: {msg}")
            }
            BuildError::RunFailed { line, code } => {
                format!("line {line}: RUN exited with code {code}")
            }
            BuildError::Parse { line, msg } => format!("line {line}: {msg}"),
            BuildError::MissingFrom => {
                String::from("Dockerfile must start with a FROM instruction")
            }
            BuildError::CopySourceMissing { src } => {
                format!("COPY/ADD source not found in build context: {src}")
            }
            BuildError::Kernel(e) => format!("i/o error: {e:?}"),
        }
    }
}

impl From<KernelError> for BuildError {
    fn from(e: KernelError) -> Self {
        BuildError::Kernel(e)
    }
}

/// Split a Dockerfile into `(1-based-start-line, logical-line)` pairs, honouring
/// `#` comments, blank lines, and `\`-continuation (including comment lines that
/// appear *inside* a continuation, which Docker skips).
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut pieces: Vec<String> = Vec::new();
    let mut start = 0usize;
    for (idx, raw) in text.split('\n').enumerate() {
        let lineno = idx.saturating_add(1);
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let continuing = !pieces.is_empty();
        if continuing {
            // Comment-only lines inside a continuation are ignored by Docker.
            if line.trim_start().starts_with('#') {
                continue;
            }
        } else {
            let lead = line.trim_start();
            if lead.is_empty() || lead.starts_with('#') {
                continue;
            }
            start = lineno;
        }
        let trimmed = line.trim();
        if let Some(prefix) = trimmed.strip_suffix('\\') {
            pieces.push(String::from(prefix.trim()));
            // remain in continuation
        } else {
            pieces.push(String::from(trimmed));
            let joined = pieces.join(" ");
            pieces.clear();
            let joined = String::from(joined.trim());
            if !joined.is_empty() {
                out.push((start, joined));
            }
        }
    }
    if !pieces.is_empty() {
        let joined = String::from(pieces.join(" ").trim());
        if !joined.is_empty() {
            out.push((start, joined));
        }
    }
    out
}

/// Quote-aware whitespace tokenizer (single/double quotes + backslash escapes).
fn tokenize(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut has = false;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if !in_single => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    has = true;
                }
            }
            '\'' if !in_double => {
                in_single = !in_single;
                has = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                has = true;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if has {
                    out.push(core::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        out.push(cur);
    }
    out
}

/// Look up a build variable (ARG/ENV), last definition wins.
fn var_value<'a>(name: &str, vars: &'a [(String, String)]) -> Option<&'a str> {
    vars.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// Expand `$VAR`, `${VAR}` and `${VAR:-default}` references in `input` using
/// `vars` (Docker performs this in FROM/COPY/ADD/ENV/LABEL/EXPOSE/WORKDIR/USER,
/// but *not* in CMD/ENTRYPOINT).  A backslash escapes the next character.
fn expand_vars(input: &str, vars: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            } else {
                out.push('\\');
            }
            continue;
        }
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('{') => {
                chars.next(); // consume '{'
                let mut body = String::new();
                for n in chars.by_ref() {
                    if n == '}' {
                        break;
                    }
                    body.push(n);
                }
                // Support ${name:-default}.
                if let Some((name, default)) = body.split_once(":-") {
                    match var_value(name, vars) {
                        Some(v) if !v.is_empty() => out.push_str(v),
                        _ => out.push_str(default),
                    }
                } else if let Some(v) = var_value(&body, vars) {
                    out.push_str(v);
                }
            }
            Some(c2) if c2.is_ascii_alphabetic() || *c2 == '_' => {
                let mut name = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphanumeric() || n == '_' {
                        name.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if let Some(v) = var_value(&name, vars) {
                    out.push_str(v);
                }
            }
            _ => out.push('$'),
        }
    }
    out
}

/// Parse a CMD/ENTRYPOINT argument: JSON exec-form `["a","b"]` verbatim, else
/// shell-form wrapped as `<shell> "<rest>"` (matching Docker). `shell` is the
/// active `SHELL` prefix (empty → the default `["/bin/sh","-c"]`).
fn parse_cmd_form(rest: &str, shell: &[String]) -> Vec<String> {
    let t = rest.trim();
    if t.starts_with('[') {
        if let Ok(v) = json::parse_str(t) {
            if let Some(arr) = v.as_array() {
                let mut items = Vec::with_capacity(arr.len());
                let mut ok = true;
                for e in arr {
                    match e.as_str() {
                        Some(s) => items.push(String::from(s)),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    return items;
                }
            }
        }
        // Malformed JSON → fall through to shell form.
    }
    if t.is_empty() {
        Vec::new()
    } else {
        let mut v: Vec<String> = if shell.is_empty() {
            alloc::vec![String::from("/bin/sh"), String::from("-c")]
        } else {
            shell.to_vec()
        };
        v.push(String::from(t));
        v
    }
}

/// Parse a JSON string-array argument (`SHELL`, exec-form only). Returns `None`
/// if the value is not a well-formed `["a","b",...]` array of strings.
fn parse_json_str_array(rest: &str) -> Option<Vec<String>> {
    let t = rest.trim();
    if !t.starts_with('[') {
        return None;
    }
    let v = json::parse_str(t).ok()?;
    let arr = v.as_array()?;
    let mut items = Vec::with_capacity(arr.len());
    for e in arr {
        items.push(String::from(e.as_str()?));
    }
    Some(items)
}

/// Parse a Go-style duration string (as Docker's `HEALTHCHECK --interval` etc.
/// accept) into nanoseconds. Supports a sequence of `<number><unit>` terms with
/// units `ns`, `us`/`µs`, `ms`, `s`, `m`, `h` and fractional numbers
/// (e.g. `"1h30m"`, `"1.5s"`, `"100ms"`). Returns `None` on any malformed input.
fn parse_go_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // A bare "0" means zero duration.
    if s == "0" {
        return Some(0);
    }
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut total_ns: u64 = 0;
    while i < bytes.len() {
        // Parse the numeric part (digits with an optional single '.').
        let num_start = i;
        let mut seen_dot = false;
        while let Some(&c) = bytes.get(i) {
            if c.is_ascii_digit() {
                i = i.checked_add(1)?;
            } else if c == b'.' && !seen_dot {
                seen_dot = true;
                i = i.checked_add(1)?;
            } else {
                break;
            }
        }
        if i == num_start {
            return None; // a unit with no preceding number
        }
        let num_str = s.get(num_start..i)?;
        let value: f64 = num_str.parse().ok()?;
        // Parse the unit.
        let unit_start = i;
        while let Some(&c) = bytes.get(i) {
            if c.is_ascii_digit() || c == b'.' {
                break;
            }
            i = i.checked_add(1)?;
        }
        let unit = s.get(unit_start..i)?;
        let unit_ns: f64 = match unit {
            "ns" => 1.0,
            "us" | "µs" | "μs" => 1_000.0,
            "ms" => 1_000_000.0,
            "s" => 1_000_000_000.0,
            "m" => 60_000_000_000.0,
            "h" => 3_600_000_000_000.0,
            _ => return None,
        };
        let term = value * unit_ns;
        if !term.is_finite() || term < 0.0 {
            return None;
        }
        total_ns = total_ns.checked_add(term as u64)?;
    }
    Some(total_ns)
}

/// Parse a Dockerfile `HEALTHCHECK` instruction body into a [`HealthcheckConfig`].
///
/// Accepts `HEALTHCHECK NONE` (disable) and
/// `HEALTHCHECK [--interval=D] [--timeout=D] [--start-period=D] [--retries=N]
/// CMD <command>`, where `<command>` is exec-form (`["exe","arg"]` → `CMD` +
/// argv) or shell-form (`cmd arg` → `CMD-SHELL` + the whole line). Returns the
/// 1-based-line-agnostic error message on malformed input.
fn parse_healthcheck(rest: &str) -> Result<HealthcheckConfig, String> {
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        return Err(String::from("HEALTHCHECK requires NONE or CMD"));
    }
    if trimmed.eq_ignore_ascii_case("NONE") {
        return Ok(HealthcheckConfig {
            test: alloc::vec![String::from("NONE")],
            ..HealthcheckConfig::default()
        });
    }

    let mut hc = HealthcheckConfig::default();
    // Consume leading `--flag=value` options until the CMD token.
    let mut remainder = trimmed;
    loop {
        let tok_end = remainder.find(char::is_whitespace).unwrap_or(remainder.len());
        let tok = &remainder[..tok_end];
        if let Some(opt) = tok.strip_prefix("--") {
            let (key, val) = opt.split_once('=').ok_or_else(|| {
                format!("HEALTHCHECK option '{tok}' needs a value (--key=value)")
            })?;
            match key {
                "interval" => {
                    hc.interval_ns =
                        parse_go_duration(val).ok_or_else(|| format!("bad --interval '{val}'"))?;
                }
                "timeout" => {
                    hc.timeout_ns =
                        parse_go_duration(val).ok_or_else(|| format!("bad --timeout '{val}'"))?;
                }
                "start-period" => {
                    hc.start_period_ns = parse_go_duration(val)
                        .ok_or_else(|| format!("bad --start-period '{val}'"))?;
                }
                "retries" => {
                    hc.retries =
                        val.parse().map_err(|_| format!("bad --retries '{val}'"))?;
                }
                other => return Err(format!("unknown HEALTHCHECK option '--{other}'")),
            }
            remainder = remainder[tok_end..].trim_start();
        } else {
            break;
        }
    }

    // The verb must be CMD (Docker only supports CMD after the options).
    let verb_end = remainder.find(char::is_whitespace).unwrap_or(remainder.len());
    let verb = &remainder[..verb_end];
    if !verb.eq_ignore_ascii_case("CMD") {
        return Err(format!("HEALTHCHECK expects CMD (or NONE), found '{verb}'"));
    }
    let cmd_rest = remainder[verb_end..].trim();
    if cmd_rest.is_empty() {
        return Err(String::from("HEALTHCHECK CMD requires a command"));
    }
    // Exec-form → CMD + argv; shell-form → CMD-SHELL + the whole line.
    if let Some(argv) = parse_json_str_array(cmd_rest) {
        hc.test = core::iter::once(String::from("CMD")).chain(argv).collect();
    } else {
        hc.test = alloc::vec![String::from("CMD-SHELL"), String::from(cmd_rest)];
    }
    Ok(hc)
}

/// Normalise a Dockerfile destination/path into an archive-relative path
/// (no leading `/`, no trailing `/`, no empty/`.`/`..` components).
fn archive_norm(path: &str) -> String {
    let mut comps: Vec<&str> = Vec::new();
    for c in path.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                comps.pop();
            }
            other => comps.push(other),
        }
    }
    comps.join("/")
}

/// Effective permission bits for a COPY'd file (fall back to 0o644).
fn file_mode(meta: &crate::fs::vfs::FileMeta) -> u32 {
    let m = u32::from(meta.permissions);
    if m == 0 { 0o644 } else { m }
}

/// Parse a `.dockerignore` file into ordered `(negated, pattern)` rules.
/// Blank lines and `#` comments are skipped; a leading `!` negates (re-includes).
fn parse_dockerignore(bytes: &[u8]) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    let text = core::str::from_utf8(bytes).unwrap_or("");
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (neg, pat) = match line.strip_prefix('!') {
            Some(rest) => (true, rest.trim()),
            None => (false, line),
        };
        let norm = archive_norm(pat);
        if !norm.is_empty() {
            out.push((neg, norm));
        }
    }
    out
}

/// Glob match with Docker/`filepath.Match` semantics extended with `**`:
/// `*` matches any run of non-`/`, `**` matches any run including `/`, `?`
/// matches a single non-`/` char, everything else is literal.
fn glob_match(pattern: &str, path: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = path.chars().collect();
    glob_rec(&p, &t)
}

fn glob_rec(p: &[char], t: &[char]) -> bool {
    let Some((&c, prest)) = p.split_first() else {
        return t.is_empty();
    };
    match c {
        '*' => {
            if prest.first() == Some(&'*') {
                // `**` — match any run, including `/`.
                let prest2 = prest.get(1..).unwrap_or(&[]);
                let mut i = 0usize;
                loop {
                    if glob_rec(prest2, t.get(i..).unwrap_or(&[])) {
                        return true;
                    }
                    if i >= t.len() {
                        return false;
                    }
                    i = i.saturating_add(1);
                }
            } else {
                // `*` — match any run of non-`/`.
                let mut i = 0usize;
                loop {
                    if glob_rec(prest, t.get(i..).unwrap_or(&[])) {
                        return true;
                    }
                    match t.get(i) {
                        None | Some('/') => return false,
                        Some(_) => {}
                    }
                    i = i.saturating_add(1);
                }
            }
        }
        '?' => match t.split_first() {
            Some((&tc, trest)) if tc != '/' => glob_rec(prest, trest),
            _ => false,
        },
        lit => match t.split_first() {
            Some((&tc, trest)) if tc == lit => glob_rec(prest, trest),
            _ => false,
        },
    }
}

/// Whether `path` (context-relative) is excluded by `.dockerignore` `patterns`.
/// A pattern matching any ancestor of `path` excludes it; rules apply in file
/// order (last match wins), so a later `!rule` can re-include.
fn path_ignored(patterns: &[(bool, String)], path: &str) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let mut candidates = parent_prefixes(path);
    candidates.push(String::from(path));
    let mut ignored = false;
    for (neg, pat) in patterns {
        if candidates.iter().any(|c| glob_match(pat, c)) {
            ignored = !neg;
        }
    }
    ignored
}

/// The path of `full` relative to `context_dir` (archive-normalised).
fn ctx_rel(context_dir: &str, full: &str) -> String {
    let prefix = format!("{}/", context_dir.trim_end_matches('/'));
    archive_norm(full.strip_prefix(&prefix).unwrap_or(full))
}

/// Whether a COPY/ADD source token contains a glob metacharacter (`*`, `?`,
/// or `[`) and therefore needs wildcard expansion against the source tree.
fn has_glob_meta(s: &str) -> bool {
    s.bytes().any(|b| matches!(b, b'*' | b'?' | b'['))
}

/// Expand a wildcard COPY/ADD source pattern against `src_dir`, returning the
/// matching entries as paths relative to `src_dir`, sorted for deterministic
/// layer output.  Matching is component-by-component per Docker's
/// `filepath.Match` semantics (`*`/`?` never cross `/`); a literal component
/// must exist.  Returns an empty vec when nothing matches.
fn expand_glob(src_dir: &str, pattern: &str) -> Vec<String> {
    use crate::fs::vfs::Vfs;
    let base_dir = src_dir.trim_end_matches('/');
    // Each candidate is a path relative to `src_dir`; "" denotes `src_dir`.
    let mut current: Vec<String> = alloc::vec![String::new()];
    for comp in pattern.split('/').filter(|c| !c.is_empty()) {
        let mut next: Vec<String> = Vec::new();
        for rel in &current {
            let dir_abs = if rel.is_empty() {
                String::from(base_dir)
            } else {
                format!("{base_dir}/{rel}")
            };
            if has_glob_meta(comp) {
                let Ok(entries) = Vfs::readdir(&dir_abs) else {
                    continue;
                };
                for de in entries {
                    if de.name == "." || de.name == ".." {
                        continue;
                    }
                    if glob_match(comp, &de.name) {
                        next.push(if rel.is_empty() {
                            de.name.clone()
                        } else {
                            format!("{rel}/{}", de.name)
                        });
                    }
                }
            } else {
                // Literal component — accept only if the path exists.
                let child = if rel.is_empty() {
                    String::from(comp)
                } else {
                    format!("{rel}/{comp}")
                };
                if Vfs::metadata(&format!("{base_dir}/{child}")).is_ok() {
                    next.push(child);
                }
            }
        }
        current = next;
    }
    current.sort();
    current.dedup();
    current
}

/// Collect files for a single COPY/ADD source into `files`, computing each
/// entry's archive-relative destination path per Docker's COPY semantics and
/// skipping context files excluded by `.dockerignore` (`ignore`).
#[allow(clippy::too_many_arguments)]
fn collect_copy_src(
    context_dir: &str,
    src: &str,
    dest: &str,
    single_source: bool,
    ignore: &[(bool, String)],
    chmod: Option<u32>,
    chown: Option<(u32, u32)>,
    files: &mut Vec<LayerFile>,
    line: usize,
) -> Result<(), BuildError> {
    use crate::fs::vfs::{normalize_path, EntryType, Vfs};
    let (cuid, cgid) = chown.unwrap_or((0, 0));
    // Normalise the joined path so `.`/`..`/double-slash in a COPY source
    // (notably `COPY . /dest`) resolve to a real context path.
    let ctx = normalize_path(&format!(
        "{}/{}",
        context_dir.trim_end_matches('/'),
        src.trim_start_matches('/')
    ));
    let meta = match Vfs::metadata(&ctx) {
        Ok(m) => m,
        Err(_) => return Err(BuildError::CopySourceMissing { src: String::from(src) }),
    };
    let dest_is_dir = dest.ends_with('/') || dest.is_empty() || dest == "/";
    let dest_norm = archive_norm(dest);

    match meta.entry_type {
        EntryType::File => {
            // A `.dockerignore`-excluded explicit source contributes nothing
            // (Docker removes it from the build context entirely).
            if path_ignored(ignore, &ctx_rel(context_dir, &ctx)) {
                return Ok(());
            }
            let basename = src.rsplit('/').next().unwrap_or(src);
            let target = if dest_is_dir || !single_source {
                if dest_norm.is_empty() {
                    String::from(basename)
                } else {
                    format!("{dest_norm}/{basename}")
                }
            } else {
                dest_norm.clone()
            };
            let target = archive_norm(&target);
            if target.is_empty() {
                return Err(BuildError::Parse {
                    line,
                    msg: format!("COPY/ADD destination resolves to empty path for {src}"),
                });
            }
            let data = Vfs::read_file(&ctx)?;
            files.push(LayerFile {
                path: target,
                data,
                mode: chmod.unwrap_or_else(|| file_mode(&meta)),
                uid: cuid,
                gid: cgid,
            });
        }
        EntryType::Directory => {
            // Docker copies the *contents* of a directory source into dest.
            let mut stack: Vec<(String, String)> =
                alloc::vec![(ctx.clone(), dest_norm.clone())];
            while let Some((cur, cur_dest)) = stack.pop() {
                let entries = Vfs::readdir(&cur)?;
                for de in entries {
                    if de.name == "." || de.name == ".." {
                        continue;
                    }
                    let child = format!("{cur}/{}", de.name);
                    let child_dest = if cur_dest.is_empty() {
                        de.name.clone()
                    } else {
                        format!("{cur_dest}/{}", de.name)
                    };
                    match de.entry_type {
                        // Always descend (a `!rule` can re-include a file under
                        // an otherwise-ignored directory), filtering per file.
                        EntryType::Directory => stack.push((child, child_dest)),
                        EntryType::File => {
                            if path_ignored(ignore, &ctx_rel(context_dir, &child)) {
                                continue;
                            }
                            if files.len() >= MAX_COPY_FILES {
                                return Err(BuildError::Parse {
                                    line,
                                    msg: String::from("COPY/ADD collected too many files"),
                                });
                            }
                            let cmeta = Vfs::metadata(&child)?;
                            let data = Vfs::read_file(&child)?;
                            files.push(LayerFile {
                                path: archive_norm(&child_dest),
                                data,
                                mode: chmod.unwrap_or_else(|| file_mode(&cmeta)),
                                uid: cuid,
                                gid: cgid,
                            });
                        }
                        // Symlinks/other kinds are skipped (documented limitation).
                        _ => {}
                    }
                }
            }
        }
        _ => {
            return Err(BuildError::Parse {
                line,
                msg: format!("COPY/ADD source is not a regular file or directory: {src}"),
            });
        }
    }
    Ok(())
}

/// Join a directory and a (possibly `.`/`..`-laden) relative path, normalising
/// the result — the same rule [`collect_copy_src`] applies to a COPY source.
fn normalize_path_join(dir: &str, rel: &str) -> String {
    crate::fs::vfs::normalize_path(&format!(
        "{}/{}",
        dir.trim_end_matches('/'),
        rel.trim_start_matches('/')
    ))
}

/// If `data` is a tar archive (plain, or gzip-compressed), return the
/// uncompressed tar bytes; otherwise `None`.  Used by `ADD` auto-extraction.
fn as_tar_bytes(data: &[u8]) -> Option<Vec<u8>> {
    // gzip magic 0x1f 0x8b.
    let is_gzip = data.first() == Some(&0x1f) && data.get(1) == Some(&0x8b);
    let tar = if is_gzip {
        crate::fs::compress::gunzip(data).ok()?
    } else {
        data.to_vec()
    };
    // POSIX ustar magic sits at offset 257.
    if tar.get(257..262) == Some(b"ustar".as_ref()) {
        Some(tar)
    } else {
        None
    }
}

/// Extract every regular file of a tar archive into `dest` (a directory), as
/// layer files.  Mirrors Docker's `ADD <local-tar>` unpacking.  `chmod`/`chown`
/// override the archive's own mode/owner when supplied; otherwise the archive's
/// metadata is preserved.
fn add_tar_into(
    tar: &[u8],
    dest: &str,
    chmod: Option<u32>,
    chown: Option<(u32, u32)>,
    files: &mut Vec<LayerFile>,
    line: usize,
) -> Result<(), BuildError> {
    let dest_norm = archive_norm(dest);
    let entries = crate::fs::tar::parse(tar).map_err(BuildError::Kernel)?;
    for e in &entries {
        // Only regular files carry data; directories are synthesised by
        // build_layer_tar and symlinks are a documented copy limitation.
        if !matches!(e.kind, crate::fs::tar::EntryKind::File) {
            continue;
        }
        let name = e.name.trim_start_matches('/');
        if name.is_empty() {
            continue;
        }
        let target = if dest_norm.is_empty() {
            archive_norm(name)
        } else {
            archive_norm(&format!("{dest_norm}/{name}"))
        };
        if target.is_empty() {
            continue;
        }
        if files.len() >= MAX_COPY_FILES {
            return Err(BuildError::Parse {
                line,
                msg: String::from("ADD tar extracted too many files"),
            });
        }
        let data_end = e.data_offset.saturating_add(e.size as usize);
        let data = tar.get(e.data_offset..data_end).unwrap_or(&[]).to_vec();
        let (uid, gid) = chown.unwrap_or((e.uid, e.gid));
        files.push(LayerFile {
            path: target,
            data,
            mode: chmod.unwrap_or(e.mode),
            uid,
            gid,
        });
    }
    Ok(())
}

/// Build an OCI image from a Dockerfile (no `--build-arg` overrides).
///
/// Convenience wrapper over [`build_image_with_args`] with an empty override
/// set.  See that function for the full instruction and error documentation.
///
/// # Errors
/// Returns [`BuildError`] on a malformed/unsupported instruction, a missing
/// COPY source, a `RUN`, or an underlying VFS failure.
pub fn build_image(
    dockerfile: &[u8],
    context_dir: &str,
    dest_dir: &str,
) -> Result<Descriptor, BuildError> {
    build_image_with_args(dockerfile, context_dir, dest_dir, &[])
}

/// Build an OCI image from a Dockerfile, honouring `--build-arg` overrides.
///
/// Supports every Dockerfile instruction except `RUN` (which needs the
/// operator-gated in-container exec — see open-questions.md Q17): `FROM`
/// (`scratch` or a local OCI image directory, with base-layer + config
/// inheritance), `COPY`/`ADD` (from `context_dir`), `ENV`, `CMD`, `ENTRYPOINT`,
/// `WORKDIR`, `USER`, `EXPOSE`, `LABEL`, and `ARG` (with `${VAR}` / `$VAR` /
/// `${VAR:-default}` expansion).  The result is written to `dest_dir` as a
/// standard OCI image loadable by [`load_image`].
///
/// `build_args` are `(name, value)` pairs from `--build-arg`.  As in Docker,
/// an override only takes effect for a `name` that the Dockerfile declares via
/// `ARG` (undeclared overrides are ignored), and it supersedes that `ARG`'s
/// default.
///
/// # Errors
/// Returns [`BuildError`] on a malformed/unsupported instruction, a missing
/// COPY source, a `RUN`, or an underlying VFS failure.
pub fn build_image_with_args(
    dockerfile: &[u8],
    context_dir: &str,
    dest_dir: &str,
    build_args: &[(String, String)],
) -> Result<Descriptor, BuildError> {
    build_image_targeted(dockerfile, context_dir, dest_dir, build_args, None)
}

/// Build an OCI image from a Dockerfile, optionally stopping at a named stage.
///
/// Identical to [`build_image_with_args`] but, when `target` is `Some`, builds
/// only the stages up to and including the stage named (`FROM … AS <name>`) or
/// indexed by `target`, and writes *that* stage's image to `dest_dir` — the
/// `docker build --target <stage>` behaviour.  Stages after the target are not
/// built.
///
/// # Errors
/// Returns [`BuildError`] on a malformed/unsupported instruction, a missing
/// COPY source, a `RUN`, an unknown `--target`, or an underlying VFS failure.
pub fn build_image_targeted(
    dockerfile: &[u8],
    context_dir: &str,
    dest_dir: &str,
    build_args: &[(String, String)],
    target: Option<&str>,
) -> Result<Descriptor, BuildError> {
    use crate::fs::Vfs;

    if dest_dir.is_empty() || dest_dir.contains('\0') || context_dir.contains('\0') {
        return Err(BuildError::Kernel(KernelError::InvalidArgument));
    }
    let dest = dest_dir.trim_end_matches('/');
    if dest.is_empty() {
        return Err(BuildError::Kernel(KernelError::InvalidArgument));
    }
    let text = core::str::from_utf8(dockerfile).map_err(|_| BuildError::Parse {
        line: 0,
        msg: String::from("Dockerfile is not valid UTF-8"),
    })?;

    let instrs = logical_lines(text);
    if instrs.len() > MAX_BUILD_INSTRUCTIONS {
        return Err(BuildError::Parse {
            line: 0,
            msg: String::from("Dockerfile has too many instructions"),
        });
    }

    // Read `.dockerignore` from the build-context root (best-effort; absence is
    // not an error — an empty rule set excludes nothing).
    let ignore = {
        let path = format!("{}/.dockerignore", context_dir.trim_end_matches('/'));
        match Vfs::read_file(&path) {
            Ok(bytes) => parse_dockerignore(&bytes),
            Err(_) => Vec::new(),
        }
    };

    // Split the Dockerfile into stages at `FROM` boundaries and collect the
    // "global" ARGs declared before the first FROM.
    let StageSplit { global_args, stages } = split_stages(&instrs, build_args)?;
    if stages.len() > MAX_BUILD_STAGES {
        return Err(BuildError::Parse {
            line: 0,
            msg: String::from("Dockerfile has too many build stages"),
        });
    }

    let stage_count = stages.len();

    // `--target` selects the output stage: build only 0..=target.  Resolve it
    // by `AS` name first, then by 0-based index.
    let last_idx = match target {
        Some(t) => {
            let by_name = stages.iter().position(|s| s.name.as_deref() == Some(t));
            match by_name.or_else(|| t.parse::<usize>().ok().filter(|&n| n < stage_count)) {
                Some(n) => n,
                None => {
                    return Err(BuildError::Parse {
                        line: 0,
                        msg: format!("build target stage not found: {t}"),
                    });
                }
            }
        }
        None => stage_count.saturating_sub(1),
    };

    // Build each stage in order up to the target.  Every stage but the target
    // is materialised to a temporary OCI image directory so that a later
    // `FROM <stage>` or `COPY --from=<stage>` can consume it; the target stage
    // writes to `dest`.
    let mut built: Vec<StageBuilt> = Vec::new();
    let mut temp_stage_dirs: Vec<String> = Vec::new();
    let mut final_desc: Option<Descriptor> = None;

    for i in 0..=last_idx {
        let Some(stage) = stages.get(i) else { break };
        let is_last = i == last_idx;
        let stage_dest = if is_last {
            String::from(dest)
        } else {
            format!("{dest}.stage{i}")
        };
        match build_one_stage(
            stage,
            context_dir,
            &ignore,
            build_args,
            &global_args,
            &built,
            &stage_dest,
        ) {
            Ok(d) => {
                if is_last {
                    final_desc = Some(d);
                } else {
                    temp_stage_dirs.push(stage_dest.clone());
                }
                built.push(StageBuilt {
                    name: stage.name.clone(),
                    image_dir: stage_dest,
                });
            }
            Err(e) => {
                for d in &temp_stage_dirs {
                    let _ = Vfs::remove_recursive(d);
                }
                return Err(e);
            }
        }
    }

    // Remove intermediate stage images (the final image stays in `dest`).
    for d in &temp_stage_dirs {
        let _ = Vfs::remove_recursive(d);
    }
    final_desc.ok_or(BuildError::MissingFrom)
}

/// One parsed build stage: its optional `AS <name>` and its instruction slice
/// (including the leading `FROM` line as element 0).
struct StageSpec<'a> {
    name: Option<String>,
    instrs: &'a [(usize, String)],
}

/// The result of splitting a Dockerfile into stages: the pre-FROM "global"
/// ARGs plus the ordered per-stage instruction slices.
struct StageSplit<'a> {
    global_args: Vec<(String, String)>,
    stages: Vec<StageSpec<'a>>,
}

/// A completed stage, addressable by name or by index for `FROM <stage>` /
/// `COPY --from=<stage>` resolution.
struct StageBuilt {
    name: Option<String>,
    image_dir: String,
}

/// Apply an `ARG` instruction to the running variable set, honouring any
/// matching `--build-arg` override (which only takes effect for a declared
/// name, superseding the `ARG` default).
fn apply_arg(rest_raw: &str, vars: &mut Vec<(String, String)>, build_args: &[(String, String)]) {
    let expanded = expand_vars(rest_raw, vars);
    let (name, default) = match expanded.split_once('=') {
        Some((n, v)) => (String::from(n.trim()), String::from(v)),
        None => (String::from(expanded.trim()), String::new()),
    };
    if !name.is_empty() {
        let value = build_args
            .iter()
            .rev()
            .find(|(k, _)| *k == name)
            .map_or(default, |(_, v)| v.clone());
        vars.push((name, value));
    }
}

/// Extract the `AS <name>` label from a `FROM <ref> [AS <name>]` line, if any.
fn parse_stage_name(from_line: &str) -> Option<String> {
    let toks: Vec<&str> = from_line.split_whitespace().collect();
    let mut it = toks.iter();
    while let Some(t) = it.next() {
        if t.eq_ignore_ascii_case("AS") {
            return it.next().map(|s| String::from(*s));
        }
    }
    None
}

/// Split logical Dockerfile lines into stages at each `FROM`, returning the
/// pre-FROM global ARGs and the ordered stage slices.
///
/// Any non-`ARG` instruction before the first `FROM` is a hard error
/// ([`BuildError::MissingFrom`]), matching Docker's rule that a build must
/// begin (globals aside) with `FROM`.
fn split_stages<'a>(
    instrs: &'a [(usize, String)],
    build_args: &[(String, String)],
) -> Result<StageSplit<'a>, BuildError> {
    let mut starts: Vec<usize> = Vec::new();
    for (idx, (_, logical)) in instrs.iter().enumerate() {
        let first = logical.split_whitespace().next().unwrap_or("");
        if first.eq_ignore_ascii_case("FROM") {
            starts.push(idx);
        }
    }
    let first_from = match starts.first() {
        Some(&s) => s,
        None => return Err(BuildError::MissingFrom),
    };

    // Pre-FROM preamble: only ARG is allowed there.
    let mut global_args: Vec<(String, String)> = Vec::new();
    for (_, logical) in instrs.get(..first_from).unwrap_or(&[]) {
        let (instr, rest_raw) = match logical.split_once(char::is_whitespace) {
            Some((a, b)) => (a, b.trim()),
            None => (logical.as_str(), ""),
        };
        if instr.eq_ignore_ascii_case("ARG") {
            apply_arg(rest_raw, &mut global_args, build_args);
        } else {
            return Err(BuildError::MissingFrom);
        }
    }

    let mut stages: Vec<StageSpec<'a>> = Vec::new();
    for (si, &start) in starts.iter().enumerate() {
        let end = starts.get(si.saturating_add(1)).copied().unwrap_or(instrs.len());
        let slice = instrs.get(start..end).unwrap_or(&[]);
        let name = slice.first().and_then(|(_, l)| parse_stage_name(l));
        stages.push(StageSpec { name, instrs: slice });
    }
    Ok(StageSplit { global_args, stages })
}

/// Resolve a `FROM <ref>` / `COPY --from=<ref>` reference to a prior stage's
/// image directory (by `AS` name, else by 0-based index).  Returns `None` if
/// the reference is not a prior stage (caller treats it as an external image).
fn resolve_stage_dir<'a>(reference: &str, prior: &'a [StageBuilt]) -> Option<&'a str> {
    if let Some(s) = prior.iter().find(|s| s.name.as_deref() == Some(reference)) {
        return Some(&s.image_dir);
    }
    if let Ok(idx) = reference.parse::<usize>() {
        if let Some(s) = prior.get(idx) {
            return Some(&s.image_dir);
        }
    }
    None
}

/// Extract every layer of an OCI image (bottom-to-top) into `out_dir`, yielding
/// that stage's assembled rootfs — the source tree for a `COPY --from`.
fn materialize_rootfs(image_dir: &str, out_dir: &str) -> Result<(), BuildError> {
    let img = load_image(image_dir).map_err(BuildError::Kernel)?;
    for layer in &img.manifest.layers {
        extract_layer(image_dir, layer, out_dir).map_err(BuildError::Kernel)?;
    }
    Ok(())
}

/// Resolve a `COPY --from=<ref>` reference to a rootfs directory to copy from,
/// materialising (and memoising) the referenced image's filesystem.
fn resolve_from_rootfs(
    reference: &str,
    prior: &[StageBuilt],
    cache: &mut Vec<(String, String)>,
    scratch: &mut Vec<String>,
    dest: &str,
) -> Result<String, BuildError> {
    let image_dir = match resolve_stage_dir(reference, prior) {
        Some(d) => String::from(d),
        None => String::from(reference),
    };
    if let Some((_, rd)) = cache.iter().find(|(id, _)| *id == image_dir) {
        return Ok(rd.clone());
    }
    let rootfs_dir = format!("{dest}.from{}", cache.len());
    crate::fs::Vfs::mkdir_all(&rootfs_dir).map_err(BuildError::Kernel)?;
    materialize_rootfs(&image_dir, &rootfs_dir)?;
    scratch.push(rootfs_dir.clone());
    cache.push((image_dir, rootfs_dir.clone()));
    Ok(rootfs_dir)
}

/// If `path` is an OCI whiteout marker (`<dir>/.wh.<name>` or `.wh.<name>`),
/// return the archive-relative path it deletes (`<dir>/<name>` or `<name>`).
fn whiteout_target_path(path: &str) -> Option<String> {
    let (parent, base) = match path.rsplit_once('/') {
        Some((p, b)) => (p, b),
        None => ("", path),
    };
    // An opaque-directory marker (`.wh..wh..opq`) has no single target; skip it
    // (our overlay never emits one, but be defensive).
    let name = base.strip_prefix(".wh.")?;
    if name.starts_with(".wh.") {
        return None;
    }
    if parent.is_empty() {
        Some(String::from(name))
    } else {
        Some(format!("{parent}/{name}"))
    }
}

/// Apply one accumulated [`BuildLayer`] onto a scratch rootfs directory `dir`,
/// reconstructing the in-progress image filesystem for a `RUN`'s overlay lower.
/// Directories and files are written verbatim; whiteout markers (`.wh.<name>`)
/// delete the corresponding lower path so a later `RUN` sees the deletion.
fn apply_build_layer_to_dir(layer: &BuildLayer, dir: &str) -> Result<(), BuildError> {
    use crate::fs::Vfs;
    let root = dir.trim_end_matches('/');
    for d in &layer.dirs {
        let p = format!("{root}/{}", d.path);
        Vfs::mkdir_all(&p).map_err(BuildError::Kernel)?;
        // Best-effort mode (POSIX perm bits are the low 12 of the mode word).
        let _ = Vfs::set_permissions(&p, (d.mode & 0o7777) as u16);
    }
    for f in &layer.files {
        if let Some(target) = whiteout_target_path(&f.path) {
            let _ = Vfs::remove_recursive(&format!("{root}/{target}"));
            continue;
        }
        // COPY/RUN layers list only leaf files; synthesise parent dirs.
        for pre in parent_prefixes(&f.path) {
            let _ = Vfs::mkdir_all(&format!("{root}/{pre}"));
        }
        let p = format!("{root}/{}", f.path);
        Vfs::write_file(&p, &f.data).map_err(BuildError::Kernel)?;
        let _ = Vfs::set_permissions(&p, (f.mode & 0o7777) as u16);
    }
    Ok(())
}

/// Reconstruct the in-progress image filesystem (base image layers, then the
/// COPY/RUN layers accumulated so far this stage) into `out_dir`, to serve as
/// the read-only lower for a `RUN`'s copy-on-write overlay.
fn materialize_current_rootfs(
    out_dir: &str,
    base_dir: Option<&str>,
    base_layer_descs: &[Descriptor],
    layers: &[BuildLayer],
) -> Result<(), BuildError> {
    crate::fs::Vfs::mkdir_all(out_dir).map_err(BuildError::Kernel)?;
    if let Some(bdir) = base_dir {
        for d in base_layer_descs {
            extract_layer(bdir, d, out_dir).map_err(BuildError::Kernel)?;
        }
    }
    for layer in layers {
        apply_build_layer_to_dir(layer, out_dir)?;
    }
    Ok(())
}

/// Execute a Dockerfile `RUN` (design-decisions.md §58 / Q17: the real
/// in-container exec) and capture its filesystem changes as a [`BuildLayer`].
///
/// The in-progress rootfs is materialised as an overlay lower; `argv[0]` runs in
/// an ephemeral container over a copy-on-write upper with the image's `env`; on a
/// zero exit the upper (added/changed files) plus whiteouts (deletions) become
/// the returned layer. All scratch dirs, the overlay, its VFS mount, and the
/// container are torn down on every exit path.
///
/// A non-zero exit aborts the build ([`BuildError::RunFailed`], matching Docker);
/// an un-launchable command (e.g. a `FROM scratch` image with no `/bin/sh` for a
/// shell-form `RUN`) yields [`BuildError::RunLaunch`].
#[allow(clippy::too_many_arguments)]
fn exec_build_run(
    run_no: usize,
    line: usize,
    dest: &str,
    argv: &[String],
    env: &[String],
    working_dir: &str,
    base_dir: Option<&str>,
    base_layer_descs: &[Descriptor],
    layers: &[BuildLayer],
) -> Result<BuildLayer, BuildError> {
    use crate::fs::Vfs;
    let lower = format!("{dest}.run{run_no}.lower");
    let upper = format!("{dest}.run{run_no}.upper");
    let merge = format!("{dest}.run{run_no}.merge");

    let mut ct: Option<crate::container::ContainerId> = None;
    let mut ov: Option<crate::fs::overlay::OverlayId> = None;
    let mut mounted = false;

    let res = exec_build_run_inner(
        line, &lower, &upper, &merge, argv, env, working_dir, base_dir, base_layer_descs,
        layers, &mut ct, &mut ov, &mut mounted,
    );

    // Teardown (best-effort), reverse creation order. The container recorded no
    // rootfs mount, so `force_delete` won't touch our overlay mount; we unmount
    // and destroy it explicitly.
    if let Some(id) = ct {
        let _ = crate::container::force_delete(id);
    }
    if mounted {
        let _ = Vfs::unmount(&merge);
    }
    if let Some(id) = ov {
        let _ = crate::fs::overlay::destroy(id);
    }
    let _ = Vfs::remove_recursive(&lower);
    let _ = Vfs::remove_recursive(&upper);
    let _ = Vfs::remove_recursive(&merge);
    res
}

#[allow(clippy::too_many_arguments)]
fn exec_build_run_inner(
    line: usize,
    lower: &str,
    upper: &str,
    merge: &str,
    argv: &[String],
    env: &[String],
    working_dir: &str,
    base_dir: Option<&str>,
    base_layer_descs: &[Descriptor],
    layers: &[BuildLayer],
    ct_out: &mut Option<crate::container::ContainerId>,
    ov_out: &mut Option<crate::fs::overlay::OverlayId>,
    mounted_out: &mut bool,
) -> Result<BuildLayer, BuildError> {
    use crate::fs::Vfs;

    let Some(program) = argv.first() else {
        return Err(BuildError::RunLaunch { line, msg: String::from("empty command") });
    };

    // 1. Reconstruct the in-progress rootfs as the overlay lower.
    materialize_current_rootfs(lower, base_dir, base_layer_descs, layers)?;
    Vfs::mkdir_all(upper).map_err(BuildError::Kernel)?;

    // 2. Create + mount the copy-on-write overlay (writes land in `upper`).
    let ov_id = crate::fs::overlay::create("oci-build-run", lower, upper)
        .map_err(BuildError::Kernel)?;
    *ov_out = Some(ov_id);
    Vfs::mkdir_all(merge).map_err(BuildError::Kernel)?;
    let ovfs = crate::fs::overlay::OverlayFs::new(ov_id).map_err(BuildError::Kernel)?;
    Vfs::mount(merge, alloc::boxed::Box::new(ovfs)).map_err(BuildError::Kernel)?;
    *mounted_out = true;

    // 3. Ephemeral container jailed at the merged view.
    let cfg = crate::container::ContainerConfig::new("oci-build-run");
    let ct_id = crate::container::create(&cfg).map_err(BuildError::Kernel)?;
    *ct_out = Some(ct_id);
    crate::container::set_root_path(ct_id, merge).map_err(BuildError::Kernel)?;
    crate::container::start(ct_id).map_err(BuildError::Kernel)?;

    // 4. Launch argv[0] with the image ENV, at the image WORKDIR; wait for exit.
    //    Passing the working directory makes a RUN with a relative path (e.g.
    //    `RUN ./configure`) resolve against WORKDIR as in Docker. A `RUN` before
    //    any WORKDIR (empty working_dir) keeps the spawn default of `/`. The
    //    directory is guaranteed to exist because WORKDIR materialises it as a
    //    layer, which `materialize_current_rootfs` reconstructs into the lower.
    let argv_bytes: Vec<Vec<u8>> = argv.iter().map(|s| s.as_bytes().to_vec()).collect();
    let argv_refs: Vec<&[u8]> = argv_bytes.iter().map(Vec::as_slice).collect();
    let env_bytes: Vec<Vec<u8>> = env.iter().map(|s| s.as_bytes().to_vec()).collect();
    let env_refs: Vec<&[u8]> = env_bytes.iter().map(Vec::as_slice).collect();
    let cwd: Option<&[u8]> =
        if working_dir.is_empty() { None } else { Some(working_dir.as_bytes()) };

    let spawn =
        crate::container::exec_path_env(ct_id, program.as_bytes(), &argv_refs, &env_refs, cwd)
            .map_err(|e| BuildError::RunLaunch { line, msg: format!("{e:?}") })?;

    let code = crate::container::wait_process(spawn.pid)
        .map_err(|e| BuildError::RunLaunch { line, msg: format!("wait failed: {e:?}") })?;
    let _ = crate::container::remove_process_task(ct_id, spawn.pid, spawn.task_id);
    if code != 0 {
        return Err(BuildError::RunFailed { line, code });
    }

    // 5. Capture the upper (added/changed files) + whiteouts (deletions).
    let whiteouts = crate::fs::overlay::list_whiteouts(ov_id).map_err(BuildError::Kernel)?;
    let upper_path = crate::fs::overlay::upper_path(ov_id).map_err(BuildError::Kernel)?;
    overlay_to_build_layer(&upper_path, &whiteouts).map_err(BuildError::Kernel)
}

/// Build a single stage, cleaning up any transient `COPY --from` rootfs
/// extractions before returning (success or failure).
fn build_one_stage(
    stage: &StageSpec,
    context_dir: &str,
    ignore: &[(bool, String)],
    build_args: &[(String, String)],
    global_args: &[(String, String)],
    prior: &[StageBuilt],
    dest: &str,
) -> Result<Descriptor, BuildError> {
    let mut scratch: Vec<String> = Vec::new();
    let res = build_one_stage_inner(
        stage,
        context_dir,
        ignore,
        build_args,
        global_args,
        prior,
        dest,
        &mut scratch,
    );
    for d in &scratch {
        let _ = crate::fs::Vfs::remove_recursive(d);
    }
    res
}

#[allow(clippy::too_many_arguments)]
fn build_one_stage_inner(
    stage: &StageSpec,
    context_dir: &str,
    ignore: &[(bool, String)],
    build_args: &[(String, String)],
    global_args: &[(String, String)],
    prior: &[StageBuilt],
    dest: &str,
    scratch: &mut Vec<String>,
) -> Result<Descriptor, BuildError> {
    use crate::fs::Vfs;

    let mut spec = ImageSpec::new();
    // Build-time variables: global ARGs seed each stage, then ARG/ENV extend.
    let mut vars: Vec<(String, String)> = global_args.to_vec();
    // Base-image layer blobs carried forward verbatim (FROM <dir>/<stage>).
    let mut base_layer_descs: Vec<Descriptor> = Vec::new();
    let mut base_diff_ids: Vec<String> = Vec::new();
    let mut base_dir: Option<String> = None;
    let mut from_seen = false;
    // Memoised (image_dir -> extracted rootfs dir) for `COPY --from`.
    let mut rootfs_cache: Vec<(String, String)> = Vec::new();
    // Monotone index for `RUN` scratch dirs (`{dest}.run{N}.{lower,upper,merge}`).
    let mut run_no: usize = 0;

    for (line, logical) in stage.instrs {
        let line = *line;
        let (instr, rest_raw) = match logical.split_once(char::is_whitespace) {
            Some((a, b)) => (a, b.trim()),
            None => (logical.as_str(), ""),
        };
        let instr_up = instr.to_ascii_uppercase();

        // ARG may legally precede FROM (a "global" build arg).
        if instr_up == "ARG" {
            apply_arg(rest_raw, &mut vars, build_args);
            continue;
        }

        if !from_seen && instr_up != "FROM" {
            return Err(BuildError::MissingFrom);
        }

        // Number of layers before this instruction: an instruction "produced a
        // layer" iff it grew this count (COPY/ADD always do; WORKDIR does
        // unless it resolves to `/`).  This keeps the OCI `history[]`
        // `empty_layer` flags exactly in step with the real layer count.
        let layers_before = spec.layers.len();

        match instr_up.as_str() {
            "FROM" => {
                let expanded = expand_vars(rest_raw, &vars);
                let base_ref = expanded.split_whitespace().next().unwrap_or("");
                if base_ref.is_empty() {
                    return Err(BuildError::Parse {
                        line,
                        msg: String::from("FROM requires an image reference"),
                    });
                }
                if base_ref != "scratch" {
                    // A prior-stage reference resolves to that stage's built
                    // image dir; otherwise resolve as either an on-disk OCI
                    // layout directory or a named-store reference (`name:tag`).
                    let (base_path, base) = match resolve_stage_dir(base_ref, prior) {
                        Some(d) => (String::from(d), load_image(d).map_err(BuildError::Kernel)?),
                        None => resolve_image_source(base_ref).map_err(BuildError::Kernel)?,
                    };
                    spec.architecture = base.config.architecture.clone();
                    spec.os = base.config.os.clone();
                    spec.env = base.config.env.clone();
                    spec.cmd = base.config.cmd.clone();
                    spec.entrypoint = base.config.entrypoint.clone();
                    spec.working_dir = base.config.working_dir.clone();
                    spec.user = base.config.user.clone();
                    spec.exposed_ports = base.config.exposed_ports.clone();
                    spec.labels = base.config.labels.clone();
                    // Volumes/StopSignal/Shell are inherited config; ONBUILD
                    // triggers are NOT (Docker fires + clears them on build).
                    spec.volumes = base.config.volumes.clone();
                    spec.stop_signal = base.config.stop_signal.clone();
                    spec.shell = base.config.shell.clone();
                    // Healthcheck is inherited config (a child may override it
                    // with its own HEALTHCHECK or disable via HEALTHCHECK NONE).
                    spec.healthcheck = base.config.healthcheck.clone();
                    // Seed vars with the inherited ENV so `${VAR}` sees them.
                    for e in &base.config.env {
                        if let Some((k, v)) = e.split_once('=') {
                            vars.push((String::from(k), String::from(v)));
                        }
                    }
                    base_layer_descs = base.manifest.layers.clone();
                    base_diff_ids = base.config.diff_ids.clone();
                    // Carry the base image's build history forward so the
                    // non-empty entries stay 1:1 with the inherited layers.
                    spec.history = base.config.history.clone();
                    if base_layer_descs.len() != base_diff_ids.len() {
                        return Err(BuildError::Parse {
                            line,
                            msg: String::from("base image layer/diff_id count mismatch"),
                        });
                    }
                    base_dir = Some(base_path);
                }
                from_seen = true;
            }
            "RUN" => {
                // Docker `RUN` runs a command inside the in-progress image and
                // commits its filesystem changes as a new layer (§58/Q17).
                let expanded = expand_vars(rest_raw, &vars);
                let argv = parse_cmd_form(&expanded, &spec.shell);
                if argv.is_empty() {
                    return Err(BuildError::Parse {
                        line,
                        msg: String::from("RUN requires a command"),
                    });
                }
                let layer = exec_build_run(
                    run_no,
                    line,
                    dest,
                    &argv,
                    &spec.env,
                    &spec.working_dir,
                    base_dir.as_deref(),
                    &base_layer_descs,
                    &spec.layers,
                )?;
                run_no = run_no.saturating_add(1);
                // A `RUN` always produces a layer (even an empty one), matching
                // Docker — this keeps the `history[]` empty_layer flags in step.
                spec.layers.push(layer);
            }
            "CMD" => {
                spec.cmd = parse_cmd_form(rest_raw, &spec.shell);
            }
            "ENTRYPOINT" => {
                spec.entrypoint = parse_cmd_form(rest_raw, &spec.shell);
            }
            "ENV" => {
                let expanded = expand_vars(rest_raw, &vars);
                let toks = tokenize(&expanded);
                if toks.first().is_some_and(|t| t.contains('=')) {
                    // key=value [key2=value2 ...]
                    for tok in &toks {
                        if let Some((k, v)) = tok.split_once('=') {
                            set_env(&mut spec.env, k, v);
                            vars.push((String::from(k), String::from(v)));
                        }
                    }
                } else if let Some(key) = toks.first() {
                    // ENV KEY the rest of the line
                    let value = rest_after_first_token(&expanded);
                    set_env(&mut spec.env, key, &value);
                    vars.push((key.clone(), value));
                } else {
                    return Err(BuildError::Parse {
                        line,
                        msg: String::from("ENV requires at least a key"),
                    });
                }
            }
            "LABEL" => {
                let expanded = expand_vars(rest_raw, &vars);
                let toks = tokenize(&expanded);
                if toks.is_empty() {
                    return Err(BuildError::Parse {
                        line,
                        msg: String::from("LABEL requires at least one key=value"),
                    });
                }
                for tok in &toks {
                    if let Some((k, v)) = tok.split_once('=') {
                        set_label(&mut spec.labels, k, v);
                    } else {
                        return Err(BuildError::Parse {
                            line,
                            msg: format!("LABEL entry is not key=value: {tok}"),
                        });
                    }
                }
            }
            "WORKDIR" => {
                let expanded = expand_vars(rest_raw, &vars);
                let w = expanded.trim();
                if w.starts_with('/') {
                    spec.working_dir = String::from(w);
                } else if spec.working_dir.is_empty() {
                    spec.working_dir = format!("/{w}");
                } else {
                    spec.working_dir =
                        format!("{}/{}", spec.working_dir.trim_end_matches('/'), w);
                }
                // Docker creates the working directory in the image filesystem
                // (mode 0o755, root-owned) if it does not already exist.  Emit a
                // layer that materialises it; overlay semantics make this a
                // no-op when the directory already exists in a lower layer.
                let dir_rel = archive_norm(&spec.working_dir);
                if !dir_rel.is_empty() {
                    spec.layers.push(BuildLayer {
                        dirs: alloc::vec![LayerDir {
                            path: dir_rel,
                            mode: 0o755,
                        }],
                        files: Vec::new(),
                    });
                }
            }
            "USER" => {
                let expanded = expand_vars(rest_raw, &vars);
                spec.user = String::from(expanded.trim());
            }
            "EXPOSE" => {
                let expanded = expand_vars(rest_raw, &vars);
                for tok in expanded.split_whitespace() {
                    let port = if tok.contains('/') {
                        String::from(tok)
                    } else {
                        format!("{tok}/tcp")
                    };
                    if !spec.exposed_ports.contains(&port) {
                        spec.exposed_ports.push(port);
                    }
                }
            }
            "COPY" | "ADD" => {
                let expanded = expand_vars(rest_raw, &vars);
                let mut toks = tokenize(&expanded);
                // Detect `--from=<ref>` *before* dropping flag tokens: it
                // switches the copy source from the build context to a prior
                // stage's (or an external image's) assembled rootfs.
                let from_ref: Option<String> = toks
                    .iter()
                    .find_map(|t| t.strip_prefix("--from=").map(String::from));
                // `--chmod=<octal>` overrides the copied files' permission bits.
                let chmod: Option<u32> = match toks.iter().find_map(|t| t.strip_prefix("--chmod=")) {
                    Some(m) => match u32::from_str_radix(m.trim_start_matches("0o"), 8) {
                        Ok(v) => Some(v),
                        Err(_) => {
                            return Err(BuildError::Parse {
                                line,
                                msg: format!("COPY/ADD --chmod is not a valid octal mode: {m}"),
                            });
                        }
                    },
                    None => None,
                };
                // `--chown=<uid>[:<gid>]` sets the owner (numeric only — name
                // resolution needs the stage's /etc/passwd, unsupported).  A
                // bare uid uses that value for the gid too, matching Docker.
                let chown: Option<(u32, u32)> = match toks.iter().find_map(|t| t.strip_prefix("--chown=")) {
                    Some(spec) => {
                        let (us, gs) = match spec.split_once(':') {
                            Some((u, g)) => (u, g),
                            None => (spec, spec),
                        };
                        match (us.parse::<u32>(), gs.parse::<u32>()) {
                            (Ok(u), Ok(g)) => Some((u, g)),
                            _ => {
                                return Err(BuildError::Parse {
                                    line,
                                    msg: format!(
                                        "COPY/ADD --chown must be numeric uid[:gid] (name resolution unsupported): {spec}"
                                    ),
                                });
                            }
                        }
                    }
                    None => None,
                };
                // Drop leading flag tokens (e.g. --chown=, --chmod=, --from=).
                toks.retain(|t| !t.starts_with("--"));
                if toks.len() < 2 {
                    return Err(BuildError::Parse {
                        line,
                        msg: String::from("COPY/ADD needs at least one source and a destination"),
                    });
                }
                // A destination that does not start with `/` is interpreted
                // relative to the current WORKDIR (Docker semantics); an unset
                // WORKDIR defaults to root.  The trailing slash (dir marker) is
                // preserved by the join.
                let dest_raw = toks.last().cloned().unwrap_or_default();
                let dest_path = if dest_raw.starts_with('/') {
                    dest_raw
                } else {
                    let wd = if spec.working_dir.is_empty() {
                        "/"
                    } else {
                        spec.working_dir.as_str()
                    };
                    format!("{}/{}", wd.trim_end_matches('/'), dest_raw)
                };
                let src_count = toks.len().saturating_sub(1);
                let mut files: Vec<LayerFile> = Vec::new();
                // For `--from`, copy from the referenced rootfs with no
                // `.dockerignore` filtering (that only applies to the context).
                let empty_ignore: [(bool, String); 0] = [];
                let (src_dir, eff_ignore): (String, &[(bool, String)]) = match &from_ref {
                    Some(r) => (
                        resolve_from_rootfs(r, prior, &mut rootfs_cache, scratch, dest)?,
                        &empty_ignore,
                    ),
                    None => (String::from(context_dir), ignore),
                };
                // Expand any wildcard sources against the source tree (Docker
                // `filepath.Match`); a literal source passes through unchanged.
                // A wildcard that matches nothing is an error (missing source).
                let mut effective_srcs: Vec<String> = Vec::new();
                for src in toks.iter().take(src_count) {
                    if has_glob_meta(src) {
                        let matches = expand_glob(&src_dir, src.trim_start_matches('/'));
                        if matches.is_empty() {
                            return Err(BuildError::CopySourceMissing { src: src.clone() });
                        }
                        effective_srcs.extend(matches);
                    } else {
                        effective_srcs.push(src.clone());
                    }
                }
                // `single` (rename-to-dest semantics) is keyed off the *expanded*
                // source count: a wildcard matching several files forces the
                // dest to be treated as a directory.
                let single = effective_srcs.len() == 1;
                // ADD (but not COPY) auto-extracts a local tar archive (plain or
                // gzip) into the destination directory — a `--from` reference
                // disables this (Docker treats it as a plain copy).
                let add_extract = instr_up == "ADD" && from_ref.is_none();
                for src in &effective_srcs {
                    if add_extract {
                        let full = normalize_path_join(&src_dir, src);
                        if let Ok(bytes) = Vfs::read_file(&full) {
                            if let Some(tar) = as_tar_bytes(&bytes) {
                                add_tar_into(&tar, &dest_path, chmod, chown, &mut files, line)?;
                                continue;
                            }
                        }
                    }
                    collect_copy_src(&src_dir, src, &dest_path, single, eff_ignore, chmod, chown, &mut files, line)?;
                }
                spec.layers.push(BuildLayer { dirs: Vec::new(), files });
            }
            "VOLUME" => {
                let expanded = expand_vars(rest_raw, &vars);
                // Accept both the JSON exec form `["/a","/b"]` and the
                // whitespace-separated shell form `VOLUME /a /b`.
                let paths = parse_json_str_array(&expanded)
                    .unwrap_or_else(|| tokenize(&expanded));
                if paths.is_empty() {
                    return Err(BuildError::Parse {
                        line,
                        msg: String::from("VOLUME requires at least one path"),
                    });
                }
                for p in paths {
                    if !spec.volumes.contains(&p) {
                        spec.volumes.push(p);
                    }
                }
            }
            "STOPSIGNAL" => {
                let expanded = expand_vars(rest_raw, &vars);
                let sig = expanded.trim();
                if sig.is_empty() {
                    return Err(BuildError::Parse {
                        line,
                        msg: String::from("STOPSIGNAL requires a signal"),
                    });
                }
                spec.stop_signal = String::from(sig);
            }
            "SHELL" => {
                // Docker requires SHELL in JSON exec form.
                let expanded = expand_vars(rest_raw, &vars);
                match parse_json_str_array(&expanded) {
                    Some(sh) if !sh.is_empty() => spec.shell = sh,
                    _ => {
                        return Err(BuildError::Parse {
                            line,
                            msg: String::from("SHELL requires a JSON array, e.g. [\"/bin/sh\",\"-c\"]"),
                        });
                    }
                }
            }
            "ONBUILD" => {
                // Store the trigger instruction verbatim (executed by a later
                // build that uses this image as a base — not run now).
                let trigger = rest_raw.trim();
                if trigger.is_empty() {
                    return Err(BuildError::Parse {
                        line,
                        msg: String::from("ONBUILD requires an instruction"),
                    });
                }
                spec.onbuild.push(String::from(trigger));
            }
            "HEALTHCHECK" => {
                // Container liveness probe (Docker `HEALTHCHECK`). Stored in the
                // image config so the runtime health monitor (container::
                // start_health_monitor / health_tick) picks it up when the image
                // is run. Var-expanded like the other config instructions.
                let expanded = expand_vars(rest_raw, &vars);
                let hc = parse_healthcheck(&expanded)
                    .map_err(|msg| BuildError::Parse { line, msg })?;
                spec.healthcheck = Some(hc);
            }
            "MAINTAINER" => {
                // Deprecated; record as the conventional label.
                let expanded = expand_vars(rest_raw, &vars);
                set_label(&mut spec.labels, "maintainer", expanded.trim());
            }
            other => {
                return Err(BuildError::Parse {
                    line,
                    msg: format!("unsupported instruction: {other}"),
                });
            }
        }

        // Record the build step. FROM contributes no entry of its own (it
        // carries the base image's history); COPY/ADD produce a filesystem
        // layer (non-empty), everything else is a metadata-only empty layer.
        if instr_up != "FROM" {
            spec.history.push(HistoryEntry {
                created_by: logical.clone(),
                empty_layer: spec.layers.len() == layers_before,
            });
        }
    }

    if !from_seen {
        return Err(BuildError::MissingFrom);
    }

    // Assemble: base layers (carried verbatim) + new COPY/ADD layers.
    create_layout_skeleton(dest);
    let mut layer_descs = base_layer_descs;
    let mut diff_ids = base_diff_ids;
    if let Some(bdir) = &base_dir {
        for d in &layer_descs {
            let bp = d.blob_path().ok_or(BuildError::Kernel(KernelError::InvalidArgument))?;
            let data = Vfs::read_file(&format!("{}/{}", bdir.trim_end_matches('/'), bp))?;
            Vfs::write_file(&format!("{dest}/{bp}"), &data)?;
        }
    }
    for layer in &spec.layers {
        let tar = build_layer_tar(layer);
        diff_ids.push(sha256_digest(&tar));
        let gz = crate::fs::compress::gzip(&tar);
        layer_descs.push(write_blob(dest, MEDIA_TYPE_LAYER_GZIP, &gz)?);
    }
    if layer_descs.len() > MAX_LAYERS {
        return Err(BuildError::Kernel(KernelError::InvalidArgument));
    }
    finish_image(dest, &spec, &layer_descs, &diff_ids).map_err(BuildError::Kernel)
}

/// Set or replace an `ENV` key in `env` (Docker keeps at most one entry per key).
fn set_env(env: &mut Vec<String>, key: &str, value: &str) {
    let prefix = format!("{key}=");
    let entry = format!("{key}={value}");
    if let Some(slot) = env.iter_mut().find(|e| e.starts_with(&prefix)) {
        *slot = entry;
    } else {
        env.push(entry);
    }
}

/// Set or replace a `LABEL` key in `labels`.
fn set_label(labels: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some(slot) = labels.iter_mut().find(|(k, _)| k == key) {
        slot.1 = String::from(value);
    } else {
        labels.push((String::from(key), String::from(value)));
    }
}

/// Everything after the first whitespace-delimited token, trimmed.
fn rest_after_first_token(s: &str) -> String {
    match s.trim().split_once(char::is_whitespace) {
        Some((_, rest)) => String::from(rest.trim()),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test OCI image format parsing with synthetic data.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[oci] Running self-test...");

    // Test 1: Descriptor parsing.
    {
        let desc_json = json::parse_str(
            r#"{"mediaType": "application/vnd.oci.image.config.v1+json", "digest": "sha256:abc123", "size": 1024}"#
        )?;
        let desc = Descriptor::from_json(&desc_json)?;
        assert_eq!(desc.media_type, MEDIA_TYPE_CONFIG);
        assert_eq!(desc.digest, "sha256:abc123");
        assert_eq!(desc.size, 1024);
        assert_eq!(desc.blob_path(), Some(String::from("blobs/sha256/abc123")));
        assert_eq!(desc.split_digest(), Some(("sha256", "abc123")));
        serial_println!("[oci]   descriptor parsing: OK");
    }

    // Test 2: Platform matching.
    {
        let plat_json = json::parse_str(
            r#"{"os": "linux", "architecture": "amd64"}"#
        )?;
        let plat = Platform::from_json(&plat_json);
        assert!(plat.matches_host());

        let plat_json = json::parse_str(
            r#"{"os": "linux", "architecture": "arm64", "variant": "v8"}"#
        )?;
        let plat = Platform::from_json(&plat_json);
        assert!(!plat.matches_host());
        serial_println!("[oci]   platform matching: OK");
    }

    // Test 3: Image index parsing.
    {
        let index_json = r#"{
            "schemaVersion": 2,
            "manifests": [
                {
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "digest": "sha256:manifest_amd64",
                    "size": 500,
                    "platform": {"os": "linux", "architecture": "amd64"}
                },
                {
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "digest": "sha256:manifest_arm64",
                    "size": 501,
                    "platform": {"os": "linux", "architecture": "arm64"}
                }
            ]
        }"#;
        let index = ImageIndex::parse(index_json.as_bytes())?;
        assert_eq!(index.schema_version, 2);
        assert_eq!(index.manifests.len(), 2);

        // Should find amd64 manifest.
        let host_manifest = index.find_manifest_for_host()
            .expect("should find amd64");
        assert_eq!(host_manifest.digest, "sha256:manifest_amd64");
        serial_println!("[oci]   image index: OK");
    }

    // Test 4: Image manifest parsing.
    {
        let manifest_json = r#"{
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": "sha256:config_digest",
                "size": 2048
            },
            "layers": [
                {
                    "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                    "digest": "sha256:layer1_digest",
                    "size": 10000
                },
                {
                    "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                    "digest": "sha256:layer2_digest",
                    "size": 5000
                }
            ]
        }"#;
        let manifest = ImageManifest::parse(manifest_json.as_bytes())?;
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.config.digest, "sha256:config_digest");
        assert_eq!(manifest.config.size, 2048);
        assert_eq!(manifest.layers.len(), 2);
        assert_eq!(manifest.layers[0].digest, "sha256:layer1_digest");
        assert_eq!(manifest.layers[1].size, 5000);
        serial_println!("[oci]   image manifest: OK");
    }

    // Test 5: Image config parsing.
    {
        let config_json = r#"{
            "architecture": "amd64",
            "os": "linux",
            "config": {
                "Env": ["PATH=/usr/bin:/bin", "HOME=/root"],
                "Cmd": ["/bin/sh"],
                "Entrypoint": ["/docker-entrypoint.sh"],
                "WorkingDir": "/app",
                "User": "nobody",
                "ExposedPorts": {"8080/tcp": {}, "443/tcp": {}},
                "Labels": {"version": "1.0", "maintainer": "test@example.com"},
                "Healthcheck": {
                    "Test": ["CMD", "/bin/health", "--check"],
                    "Interval": 5000000000,
                    "Timeout": 2000000000,
                    "StartPeriod": 1000000000,
                    "Retries": 4
                }
            },
            "rootfs": {
                "type": "layers",
                "diff_ids": [
                    "sha256:diff1",
                    "sha256:diff2"
                ]
            }
        }"#;
        let config = ImageConfig::parse(config_json.as_bytes())?;
        assert_eq!(config.architecture, "amd64");
        assert_eq!(config.os, "linux");
        assert_eq!(config.env.len(), 2);
        assert_eq!(config.env[0], "PATH=/usr/bin:/bin");
        assert_eq!(config.cmd.len(), 1);
        assert_eq!(config.cmd[0], "/bin/sh");
        assert_eq!(config.entrypoint.len(), 1);
        assert_eq!(config.entrypoint[0], "/docker-entrypoint.sh");
        assert_eq!(config.working_dir, "/app");
        assert_eq!(config.user, "nobody");
        assert_eq!(config.exposed_ports.len(), 2);
        assert_eq!(config.labels.len(), 2);
        assert_eq!(config.diff_ids.len(), 2);

        // Full command = entrypoint + cmd.
        let cmd = config.command();
        assert_eq!(cmd.len(), 2);
        assert_eq!(cmd[0], "/docker-entrypoint.sh");
        assert_eq!(cmd[1], "/bin/sh");

        // Healthcheck (CMD form) is parsed with its timing/retry fields.
        let hc = config.healthcheck.as_ref().expect("healthcheck present");
        assert!(!hc.is_disabled());
        assert!(!hc.is_shell(), "CMD form is a direct exec, not a shell");
        assert!(hc.is_runnable());
        assert_eq!(hc.probe_args(), &["/bin/health", "--check"]);
        assert_eq!(hc.interval_ns, 5_000_000_000);
        assert_eq!(hc.timeout_ns, 2_000_000_000);
        assert_eq!(hc.start_period_ns, 1_000_000_000);
        assert_eq!(hc.retries, 4);
        // Explicit values override the defaults.
        assert_eq!(hc.effective_interval_ns(), 5_000_000_000);
        assert_eq!(hc.effective_retries(), 4);
        serial_println!("[oci]   image config: OK");
    }

    // Test 5b: Healthcheck variants — CMD-SHELL, NONE (disable), and default
    // application for a present-but-timing-less healthcheck.
    {
        // CMD-SHELL: a single shell command line; unset timings → defaults.
        let shell_json = r#"{
            "architecture": "amd64", "os": "linux",
            "config": {
                "Healthcheck": {
                    "Test": ["CMD-SHELL", "curl -f http://localhost/ || exit 1"]
                }
            }
        }"#;
        let cfg = ImageConfig::parse(shell_json.as_bytes())?;
        let hc = cfg.healthcheck.as_ref().expect("shell healthcheck present");
        assert!(hc.is_shell());
        assert!(hc.is_runnable());
        assert_eq!(hc.probe_args(), &["curl -f http://localhost/ || exit 1"]);
        // Unset → Docker defaults (30s interval/timeout, 3 retries).
        assert_eq!(hc.interval_ns, 0);
        assert_eq!(hc.effective_interval_ns(), HealthcheckConfig::DEFAULT_INTERVAL_NS);
        assert_eq!(hc.effective_timeout_ns(), HealthcheckConfig::DEFAULT_INTERVAL_NS);
        assert_eq!(hc.effective_retries(), HealthcheckConfig::DEFAULT_RETRIES);

        // NONE: explicitly disabled — parsed but not runnable.
        let none_json = r#"{
            "architecture": "amd64", "os": "linux",
            "config": {"Healthcheck": {"Test": ["NONE"]}}
        }"#;
        let cfg = ImageConfig::parse(none_json.as_bytes())?;
        let hc = cfg.healthcheck.as_ref().expect("none healthcheck present");
        assert!(hc.is_disabled());
        assert!(!hc.is_runnable());
        assert!(hc.probe_args().is_empty());

        // Absent Healthcheck key → None.
        let bare = r#"{"architecture": "amd64", "os": "linux", "config": {}}"#;
        let cfg = ImageConfig::parse(bare.as_bytes())?;
        assert!(cfg.healthcheck.is_none());
        serial_println!("[oci]   healthcheck variants: OK");
    }

    // Test 6: Digest verification.
    {
        // SHA-256 of "hello" is well-known.
        let data = b"hello";
        let expected = "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        verify_digest(data, expected)?;

        // Tampered data should fail.
        let tampered = b"hallo";
        assert!(verify_digest(tampered, expected).is_err());
        serial_println!("[oci]   digest verification: OK");
    }

    // Test 7: Single-manifest index (no platform).
    {
        let index_json = r#"{
            "schemaVersion": 2,
            "manifests": [
                {
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "digest": "sha256:only_one",
                    "size": 300
                }
            ]
        }"#;
        let index = ImageIndex::parse(index_json.as_bytes())?;
        let found = index.find_manifest_for_host()
            .expect("should return the only manifest");
        assert_eq!(found.digest, "sha256:only_one");
        serial_println!("[oci]   single-manifest index: OK");
    }

    // Test 8: Empty config fields are handled.
    {
        let config_json = r#"{"architecture": "amd64", "os": "linux"}"#;
        let config = ImageConfig::parse(config_json.as_bytes())?;
        assert!(config.env.is_empty());
        assert!(config.cmd.is_empty());
        assert!(config.entrypoint.is_empty());
        assert!(config.working_dir.is_empty());
        serial_println!("[oci]   minimal config: OK");
    }

    // Test 9: Docker ENTRYPOINT/CMD override rules (effective_command).
    {
        let config_json = r#"{
            "architecture": "amd64",
            "os": "linux",
            "config": {
                "Entrypoint": ["/entry.sh"],
                "Cmd": ["serve", "--port", "80"]
            }
        }"#;
        let config = ImageConfig::parse(config_json.as_bytes())?;

        // No overrides → ENTRYPOINT ++ CMD (matches command()).
        let dflt = config.effective_command(None, &[]);
        assert_eq!(dflt, config.command());
        assert_eq!(dflt, alloc::vec!["/entry.sh", "serve", "--port", "80"]);

        // Trailing CMD override replaces CMD, keeps ENTRYPOINT.
        let cmd_ov = config.effective_command(None, &["ls", "-la"]);
        assert_eq!(cmd_ov, alloc::vec!["/entry.sh", "ls", "-la"]);

        // --entrypoint replaces ENTRYPOINT and drops the default CMD.
        let ep_ov = config.effective_command(Some("/bin/sh"), &[]);
        assert_eq!(ep_ov, alloc::vec!["/bin/sh"]);

        // --entrypoint + trailing args: new ENTRYPOINT + args as CMD.
        let ep_cmd = config.effective_command(Some("/bin/sh"), &["-c", "echo hi"]);
        assert_eq!(ep_cmd, alloc::vec!["/bin/sh", "-c", "echo hi"]);

        // --entrypoint "" clears ENTRYPOINT; trailing args become the whole
        // command line.
        let ep_clear = config.effective_command(Some(""), &["/usr/bin/env"]);
        assert_eq!(ep_clear, alloc::vec!["/usr/bin/env"]);

        // --entrypoint "" with no trailing args → empty command.
        let ep_clear_empty = config.effective_command(Some(""), &[]);
        assert!(ep_clear_empty.is_empty());

        serial_println!("[oci]   entrypoint/cmd override: OK");
    }

    // Test 10: Docker --env-file parsing (parse_env_file) and key extraction.
    {
        // Valid entries, a blank line, a comment, leading/trailing whitespace,
        // a value containing '=', a bare key (rejected), and an empty key
        // (rejected). Uses raw bytes including a non-UTF-8 value byte (0xFF).
        let mut file: Vec<u8> = Vec::new();
        file.extend_from_slice(b"# comment line\n");
        file.extend_from_slice(b"FOO=bar\n");
        file.extend_from_slice(b"  BAZ = qux \n"); // trimmed to "BAZ = qux"
        file.extend_from_slice(b"\n"); // blank
        file.extend_from_slice(b"URL=http://x/?a=1&b=2\n"); // '=' in value
        file.extend_from_slice(b"BARE\n"); // rejected (no '=')
        file.extend_from_slice(b"=novalue\n"); // rejected (empty key)
        file.extend_from_slice(b"R="); // key R, value is one raw byte:
        file.push(0xFF); // non-UTF-8 value byte preserved verbatim (no newline)

        let parsed = parse_env_file(&file);
        // FOO, "BAZ = qux", URL, R=<0xFF>  → 4 accepted.
        assert_eq!(parsed.entries.len(), 4, "four valid entries");
        assert_eq!(parsed.entries.first().map(Vec::as_slice), Some(&b"FOO=bar"[..]));
        assert_eq!(parsed.entries.get(1).map(Vec::as_slice), Some(&b"BAZ = qux"[..]));
        assert_eq!(
            parsed.entries.get(2).map(Vec::as_slice),
            Some(&b"URL=http://x/?a=1&b=2"[..])
        );
        // Last entry preserves the raw 0xFF byte (no UTF-8 corruption).
        assert_eq!(parsed.entries.get(3).map(|e| e.last().copied()), Some(Some(0xFF)));
        // Bare key on line 6 and empty key on line 7 were rejected.
        assert_eq!(parsed.rejected_lines, alloc::vec![6, 7]);

        // env_entry_key: bytes before first '='; whole entry if none.
        assert_eq!(env_entry_key(b"FOO=bar"), b"FOO");
        assert_eq!(env_entry_key(b"URL=a=b"), b"URL");
        assert_eq!(env_entry_key(b"NOEQ"), b"NOEQ");
        serial_println!("[oci]   env-file parse + key extraction: OK");
    }

    // Test 11: write_image → load_image round-trip (image authoring).
    {
        use crate::fs::Vfs;
        let dir = "/tmp/oci_wr_test";
        // Best-effort clean slate.
        cleanup_image_dir(dir);

        let mut spec = ImageSpec::new();
        spec.env.push(String::from("PATH=/usr/bin:/bin"));
        spec.env.push(String::from("APP=demo"));
        spec.cmd.push(String::from("/bin/hello"));
        spec.entrypoint.push(String::from("/entry.sh"));
        spec.working_dir = String::from("/app");
        spec.user = String::from("nobody");
        spec.exposed_ports.push(String::from("8080/tcp"));
        spec.labels.push((String::from("version"), String::from("1.0")));
        // Two layers: one file each.
        spec.layers.push(BuildLayer {
            dirs: Vec::new(),
            files: alloc::vec![LayerFile {
                path: String::from("entry.sh"),
                data: b"#!/bin/sh\necho base\n".to_vec(),
                mode: 0o755,
                uid: 0,
                gid: 0,
            }],
        });
        spec.layers.push(BuildLayer {
            dirs: Vec::new(),
            files: alloc::vec![LayerFile {
                path: String::from("bin/hello"),
                data: b"hello-binary-contents".to_vec(),
                mode: 0o755,
                uid: 0,
                gid: 0,
            }],
        });

        let manifest_desc = write_image(dir, &spec)?;
        assert_eq!(manifest_desc.media_type, MEDIA_TYPE_MANIFEST);
        assert!(manifest_desc.digest.starts_with("sha256:"));

        // Load it back and verify metadata survives the round-trip.
        let image = load_image(dir)?;
        assert_eq!(image.config.architecture, "amd64");
        assert_eq!(image.config.os, "linux");
        assert_eq!(image.config.env.len(), 2);
        assert_eq!(image.config.env[0], "PATH=/usr/bin:/bin");
        assert_eq!(image.config.cmd, alloc::vec![String::from("/bin/hello")]);
        assert_eq!(image.config.entrypoint, alloc::vec![String::from("/entry.sh")]);
        assert_eq!(image.config.working_dir, "/app");
        assert_eq!(image.config.user, "nobody");
        assert_eq!(image.config.exposed_ports.len(), 1);
        assert_eq!(image.config.exposed_ports[0], "8080/tcp");
        assert_eq!(image.config.labels.len(), 1);
        assert_eq!(image.config.diff_ids.len(), 2);
        assert_eq!(image.manifest.layers.len(), 2);

        // Extract layer 1 and confirm the file content survived tar+gzip+digest.
        let layer1 = image.manifest.layers.get(1)
            .ok_or(KernelError::InvalidArgument)?;
        let extract_dir = "/tmp/oci_wr_extract";
        let _ = Vfs::mkdir(extract_dir);
        extract_layer(dir, layer1, extract_dir)?;
        let hello = Vfs::read_file(&format!("{extract_dir}/bin/hello"))?;
        assert_eq!(hello, b"hello-binary-contents");

        // Clean up.
        let _ = Vfs::remove(&format!("{extract_dir}/bin/hello"));
        let _ = Vfs::rmdir(&format!("{extract_dir}/bin"));
        let _ = Vfs::rmdir(extract_dir);
        cleanup_image_dir(dir);
        serial_println!("[oci]   write_image round-trip: OK");
    }

    // Test 12: Dockerfile builder (build_image) — full instruction coverage,
    // RUN rejection, and FROM base-image inheritance.
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_build_ctx";
        let img = "/tmp/oci_build_img";
        let img2 = "/tmp/oci_build_img2";
        let ext = "/tmp/oci_build_extract";
        cleanup_image_dir(img);
        cleanup_image_dir(img2);

        // Build context: a directory source and a plain file source.
        let _ = Vfs::mkdir(ctx);
        let _ = Vfs::mkdir(&format!("{ctx}/app"));
        Vfs::write_file(&format!("{ctx}/app/run.sh"), b"#!/bin/sh\necho serving\n")?;
        Vfs::write_file(&format!("{ctx}/readme.txt"), b"read me")?;

        let dockerfile = br#"# demo build
FROM scratch
ARG APPDIR=/srv
LABEL version=1.0 role=web
ENV PATH=/usr/bin GREETING="hello world"
ENV SINGLE valueone
WORKDIR ${APPDIR}
COPY app /srv/app
COPY readme.txt /srv/
EXPOSE 8080 9090/udp
USER nobody
ENTRYPOINT ["/srv/app/run.sh"]
CMD ["--serve"]
"#;
        let desc = match build_image(dockerfile, ctx, img) {
            Ok(d) => d,
            Err(e) => panic!("build_image failed: {}", e.describe()),
        };
        assert_eq!(desc.media_type, MEDIA_TYPE_MANIFEST);

        let image = load_image(img)?;
        // ENV: key=value form, quoted value, and the KEY value form.
        assert!(image.config.env.iter().any(|e| e == "PATH=/usr/bin"));
        assert!(image.config.env.iter().any(|e| e == "GREETING=hello world"));
        assert!(image.config.env.iter().any(|e| e == "SINGLE=valueone"));
        assert_eq!(image.config.working_dir, "/srv");
        assert_eq!(image.config.user, "nobody");
        assert!(image.config.exposed_ports.iter().any(|p| p == "8080/tcp"));
        assert!(image.config.exposed_ports.iter().any(|p| p == "9090/udp"));
        assert!(image.config.labels.iter().any(|(k, v)| k == "version" && v == "1.0"));
        assert!(image.config.labels.iter().any(|(k, v)| k == "role" && v == "web"));
        assert_eq!(image.config.entrypoint, alloc::vec![String::from("/srv/app/run.sh")]);
        assert_eq!(image.config.cmd, alloc::vec![String::from("--serve")]);
        // WORKDIR now materialises its directory as a layer, so this build
        // yields 3 layers: WORKDIR ${APPDIR} (→ /srv), COPY app, COPY readme.txt.
        assert_eq!(image.manifest.layers.len(), 3);
        assert_eq!(image.config.diff_ids.len(), 3);

        // Build history: FROM/ARG contribute none; the 10 remaining steps each
        // add an entry, with WORKDIR + the two COPY steps non-empty (1:1 with
        // layers, in Dockerfile order).
        assert_eq!(image.config.history.len(), 10, "history step count");
        let non_empty: Vec<&HistoryEntry> =
            image.config.history.iter().filter(|h| !h.empty_layer).collect();
        assert_eq!(non_empty.len(), 3, "three layer-producing steps");
        assert!(non_empty.first().is_some_and(|h| h.created_by.starts_with("WORKDIR")));
        assert!(non_empty.get(1).is_some_and(|h| h.created_by.starts_with("COPY app")));
        assert!(non_empty.get(2).is_some_and(|h| h.created_by.starts_with("COPY readme.txt")));
        assert!(
            image.config.history.iter().any(|h| h.empty_layer && h.created_by.starts_with("LABEL")),
            "LABEL recorded as empty layer"
        );

        // Extract both layers and verify the copied files survived.
        let _ = Vfs::mkdir(ext);
        for layer in &image.manifest.layers {
            extract_layer(img, layer, ext)?;
        }
        let run = Vfs::read_file(&format!("{ext}/srv/app/run.sh"))?;
        assert_eq!(run, b"#!/bin/sh\necho serving\n");
        let readme = Vfs::read_file(&format!("{ext}/srv/readme.txt"))?;
        assert_eq!(readme, b"read me");

        // RUN now executes the real in-container exec (§58/Q17). A shell-form
        // RUN on a `FROM scratch` image has no `/bin/sh` to launch, so it must
        // fail with a precise RunLaunch (not silently succeed). The full
        // materialize→overlay→create→launch→cleanup pipeline runs here; the
        // happy path (a successful RUN capturing a layer) is exercised
        // end-to-end via the interactive `docker build` path, which is kept out
        // of this dense boot self-test because it parks the boot thread on
        // wait_process (see the B-PTHREAD-YIELDBUDGET boot-hang note).
        let with_run = b"FROM scratch\nRUN echo hi\n";
        match build_image(with_run, ctx, "/tmp/oci_build_run") {
            Err(BuildError::RunLaunch { line, .. }) => assert_eq!(line, 2),
            other => panic!("expected RunLaunch, got is_ok={:?}", other.is_ok()),
        }

        // FROM <local image> inherits base layers + config, appends a layer.
        let df2 = b"FROM /tmp/oci_build_img\nENV EXTRA=1\nCOPY readme.txt /srv/readme2.txt\n";
        build_image(df2, ctx, img2).map_err(|e| {
            serial_println!("[oci] inherit build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let child = load_image(img2)?;
        assert_eq!(child.manifest.layers.len(), 4, "3 inherited + 1 new layer");
        assert!(child.config.env.iter().any(|e| e == "PATH=/usr/bin"), "inherited env");
        assert!(child.config.env.iter().any(|e| e == "EXTRA=1"), "new env");
        assert_eq!(child.config.entrypoint, alloc::vec![String::from("/srv/app/run.sh")]);
        // History carried forward: base's 10 + ENV + COPY = 12; 4 non-empty
        // entries stay 1:1 with the 4 layers.
        assert_eq!(child.config.history.len(), 12, "base history carried forward");
        assert_eq!(
            child.config.history.iter().filter(|h| !h.empty_layer).count(),
            4,
            "non-empty history == layer count"
        );

        // --build-arg overrides an ARG default (only for a declared ARG).
        let img3 = "/tmp/oci_build_img3";
        cleanup_image_dir(img3);
        let df3 = b"FROM scratch\nARG TARGET=default\nWORKDIR /${TARGET}\nLABEL built=${TARGET}\n";
        let args = alloc::vec![
            (String::from("TARGET"), String::from("prod")),
            // Undeclared override must be ignored (no ARG NOPE in the file).
            (String::from("NOPE"), String::from("x")),
        ];
        build_image_with_args(df3, ctx, img3, &args).map_err(|e| {
            serial_println!("[oci] build-arg build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let ba = load_image(img3)?;
        assert_eq!(ba.config.working_dir, "/prod", "--build-arg overrode ARG default");
        assert!(ba.config.labels.iter().any(|(k, v)| k == "built" && v == "prod"));
        cleanup_image_dir(img3);

        // HEALTHCHECK: parse options + command, serialize into the image
        // config, and round-trip through load_image (§58/Q17). Exec-form CMD.
        let img4 = "/tmp/oci_build_img4";
        cleanup_image_dir(img4);
        let df4 = b"FROM scratch\nHEALTHCHECK --interval=30s --timeout=5s --retries=3 CMD [\"/bin/health\",\"-q\"]\n";
        build_image(df4, ctx, img4).map_err(|e| {
            serial_println!("[oci] healthcheck build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let hc_img = load_image(img4)?;
        let hc = hc_img.config.healthcheck.as_ref().expect("built healthcheck present");
        assert_eq!(hc.test, alloc::vec![
            String::from("CMD"), String::from("/bin/health"), String::from("-q"),
        ], "exec-form CMD → CMD + argv");
        assert_eq!(hc.interval_ns, 30_000_000_000, "30s interval");
        assert_eq!(hc.timeout_ns, 5_000_000_000, "5s timeout");
        assert_eq!(hc.retries, 3, "retries=3");
        assert!(hc.is_runnable() && !hc.is_shell(), "runnable exec-form probe");

        // Shell-form HEALTHCHECK → CMD-SHELL + the whole line; and a child image
        // may disable it with HEALTHCHECK NONE (must override the inherited one).
        let img5 = "/tmp/oci_build_img5";
        cleanup_image_dir(img5);
        let df5 = b"FROM /tmp/oci_build_img4\nHEALTHCHECK NONE\n";
        build_image(df5, ctx, img5).map_err(|e| {
            serial_println!("[oci] healthcheck-none build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let none_img = load_image(img5)?;
        let nhc = none_img.config.healthcheck.as_ref().expect("NONE healthcheck present");
        assert!(nhc.is_disabled(), "HEALTHCHECK NONE disables the inherited probe");
        cleanup_image_dir(img5);
        cleanup_image_dir(img4);

        // parse_go_duration unit coverage.
        assert_eq!(parse_go_duration("0"), Some(0));
        assert_eq!(parse_go_duration("100ms"), Some(100_000_000));
        assert_eq!(parse_go_duration("1h30m"), Some(5_400_000_000_000));
        assert_eq!(parse_go_duration("1.5s"), Some(1_500_000_000));
        assert_eq!(parse_go_duration("500us"), Some(500_000));
        assert_eq!(parse_go_duration("nonsense"), None);
        assert_eq!(parse_go_duration("10x"), None);
        serial_println!("[oci]   build HEALTHCHECK + durations: OK");

        // Clean up.
        let _ = Vfs::remove(&format!("{ext}/srv/app/run.sh"));
        let _ = Vfs::remove(&format!("{ext}/srv/readme.txt"));
        let _ = Vfs::rmdir(&format!("{ext}/srv/app"));
        let _ = Vfs::rmdir(&format!("{ext}/srv"));
        let _ = Vfs::rmdir(ext);
        let _ = Vfs::remove(&format!("{ctx}/app/run.sh"));
        let _ = Vfs::remove(&format!("{ctx}/readme.txt"));
        let _ = Vfs::rmdir(&format!("{ctx}/app"));
        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(img);
        cleanup_image_dir(img2);
        serial_println!("[oci]   Dockerfile builder (build_image): OK");
    }

    // Test 13: `.dockerignore` filtering of the build context — glob excludes,
    // directory excludes, and `!` re-inclusion (last-match-wins).
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_di_ctx";
        let img = "/tmp/oci_di_img";
        let ext = "/tmp/oci_di_ext";
        cleanup_image_dir(img);

        let _ = Vfs::mkdir(ctx);
        let _ = Vfs::mkdir(&format!("{ctx}/secret"));
        let _ = Vfs::mkdir(&format!("{ctx}/logs"));
        Vfs::write_file(&format!("{ctx}/keep.txt"), b"keep")?;
        Vfs::write_file(&format!("{ctx}/debug.log"), b"noisy")?;
        Vfs::write_file(&format!("{ctx}/logs/app.log"), b"applog")?;
        Vfs::write_file(&format!("{ctx}/logs/important.log"), b"important")?;
        Vfs::write_file(&format!("{ctx}/secret/key.pem"), b"topsecret")?;
        // Exclude all *.log and the whole secret/ dir, but re-include one log.
        Vfs::write_file(
            &format!("{ctx}/.dockerignore"),
            b"# ignore rules\n*.log\nlogs/*.log\nsecret\n!logs/important.log\n",
        )?;

        let df = b"FROM scratch\nCOPY . /data\n";
        build_image(df, ctx, img).map_err(|e| {
            serial_println!("[oci] dockerignore build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let di = load_image(img)?;
        assert_eq!(di.manifest.layers.len(), 1, "single COPY layer");

        let _ = Vfs::mkdir(ext);
        for layer in &di.manifest.layers {
            extract_layer(img, layer, ext)?;
        }
        // Kept: keep.txt and the re-included important.log.
        assert_eq!(Vfs::read_file(&format!("{ext}/data/keep.txt"))?, b"keep");
        assert_eq!(
            Vfs::read_file(&format!("{ext}/data/logs/important.log"))?,
            b"important"
        );
        // Excluded: top-level log, dir log, and everything under secret/.
        assert!(Vfs::read_file(&format!("{ext}/data/debug.log")).is_err(), "*.log excluded");
        assert!(Vfs::read_file(&format!("{ext}/data/logs/app.log")).is_err(), "logs/*.log excluded");
        assert!(Vfs::read_file(&format!("{ext}/data/secret/key.pem")).is_err(), "secret/ excluded");

        // Clean up context, extract tree, and image.
        let _ = Vfs::remove(&format!("{ext}/data/keep.txt"));
        let _ = Vfs::remove(&format!("{ext}/data/logs/important.log"));
        let _ = Vfs::rmdir(&format!("{ext}/data/logs"));
        let _ = Vfs::rmdir(&format!("{ext}/data"));
        let _ = Vfs::rmdir(ext);
        let _ = Vfs::remove(&format!("{ctx}/.dockerignore"));
        let _ = Vfs::remove(&format!("{ctx}/keep.txt"));
        let _ = Vfs::remove(&format!("{ctx}/debug.log"));
        let _ = Vfs::remove(&format!("{ctx}/logs/app.log"));
        let _ = Vfs::remove(&format!("{ctx}/logs/important.log"));
        let _ = Vfs::remove(&format!("{ctx}/secret/key.pem"));
        let _ = Vfs::rmdir(&format!("{ctx}/logs"));
        let _ = Vfs::rmdir(&format!("{ctx}/secret"));
        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(img);
        serial_println!("[oci]   .dockerignore context filtering: OK");
    }

    // Test 14: metadata instructions VOLUME / STOPSIGNAL / SHELL / ONBUILD.
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_meta_ctx";
        let img = "/tmp/oci_meta_img";
        cleanup_image_dir(img);
        let _ = Vfs::mkdir(ctx);

        let df = br#"FROM scratch
VOLUME /data /var/log
VOLUME ["/cache"]
STOPSIGNAL SIGINT
SHELL ["/bin/bash","-c"]
CMD echo hi
ONBUILD RUN echo triggered
"#;
        build_image(df, ctx, img).map_err(|e| {
            serial_println!("[oci] metadata build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let m = load_image(img)?;
        // VOLUME accepts both shell and JSON forms, accumulating paths.
        assert!(m.config.volumes.iter().any(|v| v == "/data"));
        assert!(m.config.volumes.iter().any(|v| v == "/var/log"));
        assert!(m.config.volumes.iter().any(|v| v == "/cache"));
        assert_eq!(m.config.stop_signal, "SIGINT");
        assert_eq!(
            m.config.shell,
            alloc::vec![String::from("/bin/bash"), String::from("-c")]
        );
        // SHELL changes the shell-form CMD wrapping (bash instead of /bin/sh).
        assert_eq!(
            m.config.cmd,
            alloc::vec![
                String::from("/bin/bash"),
                String::from("-c"),
                String::from("echo hi")
            ]
        );
        // ONBUILD stores its trigger verbatim (not executed now).
        assert_eq!(m.config.onbuild, alloc::vec![String::from("RUN echo triggered")]);

        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(img);
        serial_println!("[oci]   metadata instructions (VOLUME/STOPSIGNAL/SHELL/ONBUILD): OK");
    }

    // Test 15: multi-stage builds — named + indexed stages, `FROM <stage>`
    // base inheritance, and `COPY --from=<stage>` cross-stage copies.
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_ms_ctx";
        let img = "/tmp/oci_ms_img";
        let ext = "/tmp/oci_ms_ext";
        cleanup_image_dir(img);
        let _ = Vfs::mkdir(ctx);
        Vfs::write_file(&format!("{ctx}/app.txt"), b"appdata")?;
        Vfs::write_file(&format!("{ctx}/extra.txt"), b"extradata")?;

        // Stage 0 (builder): puts /out/app.txt into its rootfs.
        // Stage 1 (mid): FROM builder, so it inherits /out/app.txt, adds
        //   /extra.txt.
        // Stage 2 (final): scratch; pulls app.txt from stage 0 by index and
        //   extra.txt from `mid` by name via COPY --from.
        let df = br#"FROM scratch AS builder
COPY app.txt /out/app.txt

FROM builder AS mid
COPY extra.txt /extra.txt

FROM scratch
COPY --from=0 /out/app.txt /app.txt
COPY --from=mid /extra.txt /extra.txt
"#;
        build_image(df, ctx, img).map_err(|e| {
            serial_println!("[oci] multi-stage build failed: {}", e.describe());
            KernelError::InternalError
        })?;

        let ms = load_image(img)?;
        // Final image = two COPY --from layers only (the scratch final stage
        // inherits no base layers).
        assert_eq!(ms.manifest.layers.len(), 2, "final stage has 2 COPY layers");

        let _ = Vfs::mkdir(ext);
        for layer in &ms.manifest.layers {
            extract_layer(img, layer, ext)?;
        }
        assert_eq!(Vfs::read_file(&format!("{ext}/app.txt"))?, b"appdata");
        assert_eq!(Vfs::read_file(&format!("{ext}/extra.txt"))?, b"extradata");
        // The final stage copied only specific files: the builder's /out/
        // directory must NOT leak into the final image.
        assert!(
            Vfs::read_file(&format!("{ext}/out/app.txt")).is_err(),
            "builder /out/ must not leak into the final image"
        );

        // Intermediate stage images and `--from` scratch rootfs dirs must be
        // cleaned up by the builder.
        assert!(
            Vfs::metadata(&format!("{img}.stage0")).is_err(),
            "stage0 temp image removed"
        );
        assert!(
            Vfs::metadata(&format!("{img}.stage1")).is_err(),
            "stage1 temp image removed"
        );
        assert!(
            Vfs::metadata(&format!("{img}.from0")).is_err(),
            "COPY --from rootfs scratch removed"
        );

        // `--target=builder` outputs the intermediate builder stage itself
        // (its /out/app.txt), not the final stage — and stops before `mid`.
        let timg = "/tmp/oci_ms_timg";
        let text = "/tmp/oci_ms_text";
        cleanup_image_dir(timg);
        build_image_targeted(df, ctx, timg, &[], Some("builder")).map_err(|e| {
            serial_println!("[oci] --target build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let tms = load_image(timg)?;
        assert_eq!(tms.manifest.layers.len(), 1, "builder stage has 1 layer");
        let _ = Vfs::mkdir(text);
        for layer in &tms.manifest.layers {
            extract_layer(timg, layer, text)?;
        }
        assert_eq!(Vfs::read_file(&format!("{text}/out/app.txt"))?, b"appdata");
        // The `mid`/final stages must not have been built past the target.
        assert!(Vfs::read_file(&format!("{text}/extra.txt")).is_err(), "target stops before mid");
        let _ = Vfs::remove(&format!("{text}/out/app.txt"));
        let _ = Vfs::rmdir(&format!("{text}/out"));
        let _ = Vfs::rmdir(text);
        cleanup_image_dir(timg);

        let _ = Vfs::remove(&format!("{ext}/app.txt"));
        let _ = Vfs::remove(&format!("{ext}/extra.txt"));
        let _ = Vfs::rmdir(ext);
        let _ = Vfs::remove(&format!("{ctx}/app.txt"));
        let _ = Vfs::remove(&format!("{ctx}/extra.txt"));
        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(img);
        serial_println!("[oci]   multi-stage builds (FROM..AS / COPY --from / --target): OK");
    }

    // Test 16: COPY --chmod=<octal> overrides the copied file's mode bits.
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_chmod_ctx";
        let img = "/tmp/oci_chmod_img";
        cleanup_image_dir(img);
        let _ = Vfs::mkdir(ctx);
        Vfs::write_file(&format!("{ctx}/run.sh"), b"#!/bin/sh\n")?;

        let df = b"FROM scratch\nCOPY --chmod=0600 --chown=1000:1001 run.sh /run.sh\n";
        build_image(df, ctx, img).map_err(|e| {
            serial_println!("[oci] chmod build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let ci = load_image(img)?;
        let layer = ci.manifest.layers.first().ok_or(KernelError::InternalError)?;
        let bp = layer.blob_path().ok_or(KernelError::InvalidArgument)?;
        let blob = Vfs::read_file(&format!("{img}/{bp}"))?;
        let tar = crate::fs::compress::gunzip(&blob)?;
        let entries = crate::fs::tar::parse(&tar)?;
        let run = entries
            .iter()
            .find(|e| e.name.trim_start_matches('/') == "run.sh")
            .ok_or(KernelError::InternalError)?;
        assert_eq!(run.mode & 0o777, 0o600, "--chmod=0600 applied to run.sh");
        assert_eq!(run.uid, 1000, "--chown uid applied to run.sh");
        assert_eq!(run.gid, 1001, "--chown gid applied to run.sh");

        let _ = Vfs::remove(&format!("{ctx}/run.sh"));
        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(img);
        serial_println!("[oci]   COPY --chmod / --chown: OK");
    }

    // Test 17: ADD auto-extracts a local tar archive (plain + gzip), while
    // COPY of the same archive copies it verbatim.
    {
        use crate::fs::tar::{EntryKind, TarWriteEntry};
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_addtar_ctx";
        let img = "/tmp/oci_addtar_img";
        let ext = "/tmp/oci_addtar_ext";
        cleanup_image_dir(img);
        let _ = Vfs::mkdir(ctx);

        // Author a small tar with a file and a nested file.
        let bundle = crate::fs::tar::create(&[
            TarWriteEntry {
                name: String::from("a.txt"),
                data: b"alpha".to_vec(),
                kind: EntryKind::File,
                link_target: String::new(),
                mode: 0o644,
                uid: 0,
                gid: 0,
                mtime: 0,
            },
            TarWriteEntry {
                name: String::from("sub/b.txt"),
                data: b"bravo".to_vec(),
                kind: EntryKind::File,
                link_target: String::new(),
                mode: 0o644,
                uid: 0,
                gid: 0,
                mtime: 0,
            },
        ]);
        Vfs::write_file(&format!("{ctx}/bundle.tar"), &bundle)?;
        let gz = crate::fs::compress::gzip(&bundle);
        Vfs::write_file(&format!("{ctx}/bundle.tgz"), &gz)?;

        // ADD unpacks both the plain and gzip archives; COPY keeps it whole.
        let df = b"FROM scratch\nADD bundle.tar /opt\nADD bundle.tgz /gzp\nCOPY bundle.tar /raw/bundle.tar\n";
        build_image(df, ctx, img).map_err(|e| {
            serial_println!("[oci] ADD-tar build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let ai = load_image(img)?;
        let _ = Vfs::mkdir(ext);
        for layer in &ai.manifest.layers {
            extract_layer(img, layer, ext)?;
        }
        // ADD extracted the archive contents into /opt and /gzp.
        assert_eq!(Vfs::read_file(&format!("{ext}/opt/a.txt"))?, b"alpha");
        assert_eq!(Vfs::read_file(&format!("{ext}/opt/sub/b.txt"))?, b"bravo");
        assert_eq!(Vfs::read_file(&format!("{ext}/gzp/a.txt"))?, b"alpha");
        // ADD did not leave the archive file itself behind.
        assert!(
            Vfs::read_file(&format!("{ext}/opt/bundle.tar")).is_err(),
            "ADD tar must not leave the archive file"
        );
        // COPY of the same archive copied it verbatim (no extraction).
        assert_eq!(Vfs::read_file(&format!("{ext}/raw/bundle.tar"))?, bundle);
        assert!(
            Vfs::read_file(&format!("{ext}/raw/a.txt")).is_err(),
            "COPY must not extract the archive"
        );

        let _ = Vfs::remove_recursive(ext);
        let _ = Vfs::remove(&format!("{ctx}/bundle.tar"));
        let _ = Vfs::remove(&format!("{ctx}/bundle.tgz"));
        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(img);
        serial_println!("[oci]   ADD local-tar auto-extraction: OK");
    }

    // Test 18: WORKDIR creates the directory (and parents) in the image
    // filesystem, records the config WorkingDir, and keeps the OCI history
    // `empty_layer` accounting in step with the real layer count.
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_workdir_ctx";
        let img = "/tmp/oci_workdir_img";
        let ext = "/tmp/oci_workdir_ext";
        cleanup_image_dir(img);
        let _ = Vfs::mkdir(ctx);
        Vfs::write_file(&format!("{ctx}/app.txt"), b"payload")?;

        // WORKDIR /srv then a relative WORKDIR app → /srv/app.
        let df = b"FROM scratch\nWORKDIR /srv\nWORKDIR app\nCOPY app.txt ./app.txt\n";
        build_image(df, ctx, img).map_err(|e| {
            serial_println!("[oci] WORKDIR build failed: {}", e.describe());
            KernelError::InternalError
        })?;

        // Config records the final WorkingDir.
        let wi = load_image(img)?;
        assert_eq!(wi.config.working_dir, "/srv/app", "WorkingDir must be /srv/app");

        // Three filesystem layers: `WORKDIR /srv`, `WORKDIR app` (→ /srv/app),
        // and the COPY.
        assert_eq!(
            wi.manifest.layers.len(),
            3,
            "expected 3 layers (2 WORKDIR + 1 COPY), got {}",
            wi.manifest.layers.len()
        );
        // OCI invariant: non-empty history entries == layer count.
        let non_empty = wi.config.history.iter().filter(|h| !h.empty_layer).count();
        assert_eq!(non_empty, 3, "non-empty history entries must equal layer count");

        // Extracting all layers yields the directory tree and the copied file.
        let _ = Vfs::mkdir(ext);
        for layer in &wi.manifest.layers {
            extract_layer(img, layer, ext)?;
        }
        assert!(Vfs::is_directory(&format!("{ext}/srv")), "/srv must be a directory");
        assert!(Vfs::is_directory(&format!("{ext}/srv/app")), "/srv/app must be a directory");
        assert_eq!(Vfs::read_file(&format!("{ext}/srv/app/app.txt"))?, b"payload");
        let _ = Vfs::remove_recursive(ext);
        cleanup_image_dir(img);

        // `WORKDIR /` resets the working dir to root and must NOT emit a
        // spurious layer (root already exists) — only the COPY produces one.
        let df_root = b"FROM scratch\nWORKDIR /\nCOPY app.txt /app.txt\n";
        build_image(df_root, ctx, img).map_err(|e| {
            serial_println!("[oci] WORKDIR-root build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let ri = load_image(img)?;
        assert_eq!(ri.config.working_dir, "/", "WORKDIR / resets to root");
        assert_eq!(
            ri.manifest.layers.len(),
            1,
            "WORKDIR / emits no layer; only the COPY does, got {}",
            ri.manifest.layers.len()
        );

        let _ = Vfs::remove(&format!("{ctx}/app.txt"));
        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(img);
        serial_println!("[oci]   WORKDIR creates image directory: OK");
    }

    // Test 19: COPY/ADD wildcard source matching (Docker `filepath.Match`) —
    // `*` expands to matching context entries; a non-matching glob errors.
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_glob_ctx";
        let img = "/tmp/oci_glob_img";
        let ext = "/tmp/oci_glob_ext";
        cleanup_image_dir(img);
        let _ = Vfs::mkdir(ctx);
        Vfs::write_file(&format!("{ctx}/package.json"), b"pkg")?;
        Vfs::write_file(&format!("{ctx}/package-lock.json"), b"lock")?;
        Vfs::write_file(&format!("{ctx}/readme.md"), b"readme")?;

        // `COPY package*.json /app/` matches the two JSON files but not the
        // markdown; the trailing-slash dest keeps their basenames.
        let df = b"FROM scratch\nCOPY package*.json /app/\n";
        build_image(df, ctx, img).map_err(|e| {
            serial_println!("[oci] glob build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let gi = load_image(img)?;
        let _ = Vfs::mkdir(ext);
        for layer in &gi.manifest.layers {
            extract_layer(img, layer, ext)?;
        }
        assert_eq!(Vfs::read_file(&format!("{ext}/app/package.json"))?, b"pkg");
        assert_eq!(Vfs::read_file(&format!("{ext}/app/package-lock.json"))?, b"lock");
        assert!(
            Vfs::read_file(&format!("{ext}/app/readme.md")).is_err(),
            "glob must not pull in non-matching files"
        );
        let _ = Vfs::remove_recursive(ext);
        cleanup_image_dir(img);

        // A wildcard that matches nothing is a build error (missing source).
        let df_none = b"FROM scratch\nCOPY nomatch*.zip /app/\n";
        match build_image(df_none, ctx, img) {
            Err(BuildError::CopySourceMissing { .. }) => {}
            other => panic!("expected CopySourceMissing for empty glob, ok={}", other.is_ok()),
        }
        cleanup_image_dir(img);

        let _ = Vfs::remove(&format!("{ctx}/package.json"));
        let _ = Vfs::remove(&format!("{ctx}/package-lock.json"));
        let _ = Vfs::remove(&format!("{ctx}/readme.md"));
        let _ = Vfs::rmdir(ctx);
        serial_println!("[oci]   COPY/ADD wildcard source matching: OK");
    }

    // Test 20: named image store — tag a built image into the store, list and
    // resolve it by reference, add a second tag, then remove one tag and verify
    // the surviving tag (and its blobs) remain while the removed tag is gone.
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_store_ctx";
        let img = "/tmp/oci_store_img";
        cleanup_image_dir(img);
        // Start from a clean store so counts are deterministic.
        let _ = Vfs::remove_recursive(STORE_DIR);
        let _ = Vfs::mkdir(ctx);
        Vfs::write_file(&format!("{ctx}/app.txt"), b"store-payload")?;

        let df = b"FROM scratch\nCOPY app.txt /app.txt\n";
        build_image(df, ctx, img).map_err(|e| {
            serial_println!("[oci] store build failed: {}", e.describe());
            KernelError::InternalError
        })?;

        // Tag the built image into the store as `demo:v1`.
        let digest = store_tag_from_dir(img, "demo:v1")?;
        assert!(digest.starts_with("sha256:"), "manifest digest must be sha256");

        // `latest` default: `store_resolve("demo")` == `demo:v1`? No — different
        // tags. Resolve the explicit tag we set.
        assert_eq!(store_resolve("demo:v1")?, digest, "resolve demo:v1");

        // List shows exactly the one reference.
        let listed = store_list()?;
        assert_eq!(listed.len(), 1, "store must hold one tag, got {}", listed.len());
        assert_eq!(listed.first().map(|s| s.reference.as_str()), Some("demo:v1"));

        // Add a second tag (no blob recopy) pointing at the same manifest.
        store_add_tag("demo:v1", "demo:latest")?;
        assert_eq!(store_resolve("demo:latest")?, digest, "second tag resolves");
        assert_eq!(store_list()?.len(), 2, "two tags after add");

        // Blob count before removal.
        let blobs_dir = format!("{STORE_DIR}/blobs/sha256");
        let blob_count = |dir: &str| -> usize {
            Vfs::readdir(dir)
                .map(|l| l.iter().filter(|d| d.name != "." && d.name != "..").count())
                .unwrap_or(0)
        };
        let before_blobs = blob_count(&blobs_dir);
        assert!(before_blobs > 0, "store must have blobs");

        // Remove one tag: the other still points at the same manifest, so no
        // blob should be GC'd.
        store_remove("demo:v1")?;
        assert!(store_resolve("demo:v1").is_err(), "removed tag is gone");
        assert_eq!(store_resolve("demo:latest")?, digest, "surviving tag intact");
        assert_eq!(store_list()?.len(), 1, "one tag remains");
        assert_eq!(
            blob_count(&blobs_dir),
            before_blobs,
            "shared blobs must survive while another tag references them"
        );

        // Remove the last tag: now every blob is unreachable and GC'd.
        store_remove("demo:latest")?;
        assert!(store_list()?.is_empty(), "store empty after last removal");
        assert_eq!(blob_count(&blobs_dir), 0, "all blobs GC'd after last tag removed");

        let _ = Vfs::remove_recursive(STORE_DIR);
        let _ = Vfs::remove(&format!("{ctx}/app.txt"));
        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(img);
        serial_println!("[oci]   named image store tag/list/resolve/rmi+GC: OK");
    }

    // Test 21: store reference resolution — a `name:tag` reference resolves via
    // the store (blob-source = STORE_DIR), a directory path resolves in place,
    // and `FROM name:tag` inherits the stored base image's config + layers.
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_ref_ctx";
        let base = "/tmp/oci_ref_base";
        let child = "/tmp/oci_ref_child";
        cleanup_image_dir(base);
        cleanup_image_dir(child);
        let _ = Vfs::remove_recursive(STORE_DIR);
        let _ = Vfs::mkdir(ctx);
        Vfs::write_file(&format!("{ctx}/base.txt"), b"base-data")?;

        // Build a base image and tag it into the store as `base:1`.
        let df_base = b"FROM scratch\nENV FOO=bar\nCOPY base.txt /base.txt\n";
        build_image(df_base, ctx, base).map_err(|e| {
            serial_println!("[oci] ref base build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let base_digest = store_tag_from_dir(base, "base:1")?;

        // A directory path resolves in place (blob-source == the dir).
        let (dsrc, dimg) = resolve_image_source(base)?;
        assert_eq!(dsrc, base, "directory arg resolves to itself");
        assert_eq!(dimg.manifest.layers.len(), 1);

        // A store reference resolves to STORE_DIR with the tagged manifest.
        let (rsrc, rimg) = resolve_image_source("base:1")?;
        assert_eq!(rsrc, STORE_DIR, "store ref resolves to STORE_DIR");
        assert!(rimg.config.env.iter().any(|e| e == "FOO=bar"), "config inherited");
        let _ = base_digest;

        // `FROM base:1` (resolved from the store) inherits ENV + the base layer,
        // then adds a COPY layer of its own.
        Vfs::write_file(&format!("{ctx}/child.txt"), b"child-data")?;
        let df_child = b"FROM base:1\nCOPY child.txt /child.txt\n";
        build_image(df_child, ctx, child).map_err(|e| {
            serial_println!("[oci] FROM name:tag build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let ci = load_image(child)?;
        assert_eq!(ci.manifest.layers.len(), 2, "base layer + child COPY");
        assert!(ci.config.env.iter().any(|e| e == "FOO=bar"), "child inherits base ENV");

        // An unknown reference is a clean NotFound, not a panic.
        assert!(resolve_image_source("nope:9").is_err(), "unknown ref errors");

        let _ = Vfs::remove_recursive(STORE_DIR);
        let _ = Vfs::remove(&format!("{ctx}/base.txt"));
        let _ = Vfs::remove(&format!("{ctx}/child.txt"));
        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(base);
        cleanup_image_dir(child);
        serial_println!("[oci]   store reference resolution + FROM name:tag: OK");
    }

    // Test 22: store export/import round-trip (the core of `oci save`/`load`).
    // Tag an image into the store, export it to a standalone single-manifest
    // layout, then import that layout into a fresh store and confirm the tag +
    // blobs come back intact.
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_si_ctx";
        let img = "/tmp/oci_si_img";
        let exp = "/tmp/oci_si_exp";
        cleanup_image_dir(img);
        cleanup_image_dir(exp);
        let _ = Vfs::remove_recursive(STORE_DIR);
        let _ = Vfs::mkdir(ctx);
        Vfs::write_file(&format!("{ctx}/data.txt"), b"round-trip")?;

        let df = b"FROM scratch\nCOPY data.txt /data.txt\n";
        build_image(df, ctx, img).map_err(|e| {
            serial_println!("[oci] save/load build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let digest = store_tag_from_dir(img, "roundtrip:1")?;

        // Export the single image to a standalone layout: exactly one manifest,
        // its config, and its layer blobs (nothing else from the store).
        store_export_ref("roundtrip:1", exp)?;
        let exp_entries = read_index_at(exp)?;
        assert_eq!(exp_entries.len(), 1, "export holds one manifest");
        assert_eq!(
            exp_entries.first().map(|e| e.reference.as_str()),
            Some("roundtrip:1"),
            "ref.name preserved in export"
        );
        // The exported layout must itself load as a valid image.
        let ei = load_image(exp)?;
        assert_eq!(ei.manifest.layers.len(), 1, "exported image has its layer");

        // Wipe the store and import the exported layout back.
        let _ = Vfs::remove_recursive(STORE_DIR);
        let added = store_import_dir(exp)?;
        assert_eq!(added, alloc::vec![String::from("roundtrip:1")], "import re-adds the tag");
        assert_eq!(store_resolve("roundtrip:1")?, digest, "digest survives round-trip");
        // The imported blobs must extract to the original content.
        let (src, _img) = resolve_image_source("roundtrip:1")?;
        assert_eq!(src, STORE_DIR);
        let li = load_manifest_by_digest(STORE_DIR, &digest)?;
        let ext = "/tmp/oci_si_ext";
        let _ = Vfs::mkdir(ext);
        for layer in &li.manifest.layers {
            extract_layer(STORE_DIR, layer, ext)?;
        }
        assert_eq!(Vfs::read_file(&format!("{ext}/data.txt"))?, b"round-trip");
        let _ = Vfs::remove_recursive(ext);

        let _ = Vfs::remove_recursive(STORE_DIR);
        let _ = Vfs::remove(&format!("{ctx}/data.txt"));
        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(img);
        cleanup_image_dir(exp);
        serial_println!("[oci]   store export/import round-trip (save/load): OK");
    }

    // Test 23: commit an image from a container's filesystem changes
    // (`oci commit` / `docker commit`). Build a base image, synthesise an
    // overlay upper tree (added file + nested file) plus a whiteout (a deleted
    // base file), then `commit_image` and confirm the new image carries the
    // base layer forward, adds exactly one commit layer, preserves the base
    // config (Cmd/Env), records a COMMIT history entry, and that the commit
    // layer contains the added files and a `.wh.` whiteout marker.
    {
        use crate::fs::Vfs;
        let ctx = "/tmp/oci_ci_ctx";
        let base = "/tmp/oci_ci_base";
        let upper = "/tmp/oci_ci_upper";
        let dest = "/tmp/oci_ci_dest";
        cleanup_image_dir(base);
        cleanup_image_dir(dest);
        let _ = Vfs::remove_recursive(upper);
        let _ = Vfs::mkdir(ctx);
        Vfs::write_file(&format!("{ctx}/base.txt"), b"base-content")?;

        // Base image: one layer (base.txt), plus Cmd/Env to verify config carry.
        let df = b"FROM scratch\nCOPY base.txt /base.txt\nENV FOO=bar\nCMD [\"/bin/sh\"]\n";
        build_image(df, ctx, base).map_err(|e| {
            serial_println!("[oci] commit base build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let base_img = load_image(base)?;
        let base_layers = base_img.manifest.layers.len();
        assert_eq!(base_layers, 1, "base image has one layer");

        // Synthesise the container's overlay upper: a new top-level file and a
        // nested file under a new subdirectory.
        let _ = Vfs::mkdir(upper);
        Vfs::write_file(&format!("{upper}/newfile.txt"), b"added-by-container")?;
        let _ = Vfs::mkdir(&format!("{upper}/sub"));
        Vfs::write_file(&format!("{upper}/sub/nested.txt"), b"nested")?;
        // The container deleted base.txt → an OCI whiteout.
        let whiteouts = alloc::vec![String::from("base.txt")];

        let desc = commit_image(base, upper, &whiteouts, dest)?;
        assert!(desc.digest.starts_with("sha256:"), "commit returns a digest");

        // The committed image: base layers carried forward + one commit layer.
        let committed = load_image(dest)?;
        assert_eq!(
            committed.manifest.layers.len(),
            base_layers.saturating_add(1),
            "committed image = base layers + one commit layer"
        );
        assert_eq!(
            committed.config.diff_ids.len(),
            base_layers.saturating_add(1),
            "diff_ids grow with the new layer"
        );
        // Base runtime config preserved.
        assert_eq!(committed.config.cmd, alloc::vec![String::from("/bin/sh")], "Cmd carried forward");
        assert!(
            committed.config.env.iter().any(|e| e == "FOO=bar"),
            "Env carried forward"
        );
        // A COMMIT history entry was appended (last entry, non-empty layer).
        let last_hist = committed.config.history.last().ok_or(KernelError::InternalError)?;
        assert!(last_hist.created_by.contains("COMMIT"), "COMMIT history entry appended");
        assert!(!last_hist.empty_layer, "commit layer is not an empty_layer");

        // Inspect the commit layer (the top descriptor): decompress + parse and
        // confirm it holds the added files and the whiteout marker.
        let commit_layer = committed.manifest.layers.last().ok_or(KernelError::InternalError)?;
        let blob = Vfs::read_file(&format!(
            "{dest}/{}",
            commit_layer.blob_path().ok_or(KernelError::InvalidArgument)?
        ))?;
        let tar = crate::fs::compress::gunzip(&blob)?;
        let entries = crate::fs::tar::parse(&tar)?;
        let has = |n: &str| entries.iter().any(|e| e.name.trim_start_matches('/') == n);
        assert!(has("newfile.txt"), "commit layer holds the added file");
        assert!(has("sub/nested.txt"), "commit layer holds the nested added file");
        assert!(has(".wh.base.txt"), "commit layer holds the whiteout marker");

        // The carried base layer blob must also be present in the new layout.
        let base_layer = committed.manifest.layers.first().ok_or(KernelError::InternalError)?;
        let base_blob = format!(
            "{dest}/{}",
            base_layer.blob_path().ok_or(KernelError::InvalidArgument)?
        );
        assert!(Vfs::metadata(&base_blob).is_ok(), "base layer blob carried into new layout");

        let _ = Vfs::remove_recursive(upper);
        let _ = Vfs::remove(&format!("{ctx}/base.txt"));
        let _ = Vfs::rmdir(ctx);
        cleanup_image_dir(base);
        cleanup_image_dir(dest);
        serial_println!("[oci]   commit image from container changes (commit): OK");
    }

    serial_println!("[oci] Self-test PASSED (23 tests)");
    Ok(())
}

/// Best-effort recursive removal of an OCI image directory written by
/// [`write_image`] (layout skeleton + blobs).  Used only by the self-test.
fn cleanup_image_dir(dir: &str) {
    use crate::fs::Vfs;
    let sha_dir = format!("{dir}/blobs/sha256");
    if let Ok(entries) = Vfs::readdir(&sha_dir) {
        for de in entries {
            if de.name == "." || de.name == ".." {
                continue;
            }
            let _ = Vfs::remove(&format!("{sha_dir}/{}", de.name));
        }
    }
    let _ = Vfs::rmdir(&sha_dir);
    let _ = Vfs::rmdir(&format!("{dir}/blobs"));
    let _ = Vfs::remove(&format!("{dir}/index.json"));
    let _ = Vfs::remove(&format!("{dir}/oci-layout"));
    let _ = Vfs::rmdir(dir);
}
