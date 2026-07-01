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
    /// Layer diff-ids (sha256 of uncompressed tar, in layer order).
    pub diff_ids: Vec<String>,
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

        // Rootfs diff-ids.
        let rootfs = root.get("rootfs");
        let diff_ids = Self::parse_string_array(
            rootfs.and_then(|r| r.get_array("diff_ids")),
        );

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
            diff_ids,
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
}

/// One layer of a built image — an ordered set of files.
pub struct BuildLayer {
    /// Files contained in this layer (bottom-to-top layer order is the order
    /// of layers in [`ImageSpec::layers`]).
    pub files: Vec<LayerFile>,
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
    /// Layers, bottom-to-top (Dockerfile `COPY`/`ADD` produce these).
    pub layers: Vec<BuildLayer>,
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
            layers: Vec::new(),
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

    // Collect all directory prefixes across all files, deduplicated and ordered
    // shallow→deep (BTreeSet gives lexicographic order; a parent always sorts
    // before its children because it is a prefix ending at a `/` boundary).
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    for f in &layer.files {
        for d in parent_prefixes(&f.path) {
            dirs.insert(d);
        }
    }

    let mut entries: Vec<TarWriteEntry> = Vec::new();
    for d in dirs {
        entries.push(TarWriteEntry {
            name: format!("{d}/"),
            data: Vec::new(),
            kind: EntryKind::Directory,
            link_target: String::new(),
            mode: 0o755,
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
            uid: 0,
            gid: 0,
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
    out.push_str("}},\"rootfs\":{\"type\":\"layers\",\"diff_ids\":");
    out.push_str(&json_string_array(diff_ids));
    out.push_str("}}");
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

    use crate::fs::Vfs;
    // Create the layout skeleton (idempotent).
    let _ = Vfs::mkdir(dest);
    let _ = Vfs::mkdir(&format!("{dest}/blobs"));
    let _ = Vfs::mkdir(&format!("{dest}/blobs/sha256"));

    // Layers: tar → diff_id (uncompressed) → gzip → blob digest.
    let mut layer_descs: Vec<Descriptor> = Vec::with_capacity(spec.layers.len());
    let mut diff_ids: Vec<String> = Vec::with_capacity(spec.layers.len());
    for layer in &spec.layers {
        let tar = build_layer_tar(layer);
        diff_ids.push(sha256_digest(&tar));
        let gz = crate::fs::compress::gzip(&tar);
        layer_descs.push(write_blob(dest, MEDIA_TYPE_LAYER_GZIP, &gz)?);
    }

    // Config blob.
    let config_json = build_config_json(spec, &diff_ids);
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
                "Labels": {"version": "1.0", "maintainer": "test@example.com"}
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
        serial_println!("[oci]   image config: OK");
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
            files: alloc::vec![LayerFile {
                path: String::from("entry.sh"),
                data: b"#!/bin/sh\necho base\n".to_vec(),
                mode: 0o755,
            }],
        });
        spec.layers.push(BuildLayer {
            files: alloc::vec![LayerFile {
                path: String::from("bin/hello"),
                data: b"hello-binary-contents".to_vec(),
                mode: 0o755,
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

    serial_println!("[oci] Self-test PASSED (11 tests)");
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
