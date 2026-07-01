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
// Dockerfile builder (`docker build` / `oci build`)
// ---------------------------------------------------------------------------

/// Upper bound on instructions processed from one Dockerfile.
const MAX_BUILD_INSTRUCTIONS: usize = 4096;
/// Upper bound on files collected by a single COPY/ADD (runaway-recursion guard).
const MAX_COPY_FILES: usize = 100_000;

/// Failure modes of [`build_image`].
///
/// Kept distinct from a bare [`KernelError`] so the shell can print a precise,
/// Docker-style diagnostic — in particular, a `RUN` instruction is *deferred*
/// (it needs the operator-gated in-container exec, see open-questions.md Q17),
/// not merely "invalid".
pub enum BuildError {
    /// A `RUN` instruction was found; executing it needs rootfs exec (Q17).
    RunUnsupported { line: usize },
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
            BuildError::RunUnsupported { line } => format!(
                "line {line}: RUN requires in-container exec (deferred — see Q17); \
                 every other instruction is supported"
            ),
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
/// shell-form wrapped as `["/bin/sh","-c","<rest>"]` (matching Docker).
fn parse_cmd_form(rest: &str) -> Vec<String> {
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
        alloc::vec![String::from("/bin/sh"), String::from("-c"), String::from(t)]
    }
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

/// Collect files for a single COPY/ADD source into `files`, computing each
/// entry's archive-relative destination path per Docker's COPY semantics and
/// skipping context files excluded by `.dockerignore` (`ignore`).
fn collect_copy_src(
    context_dir: &str,
    src: &str,
    dest: &str,
    single_source: bool,
    ignore: &[(bool, String)],
    files: &mut Vec<LayerFile>,
    line: usize,
) -> Result<(), BuildError> {
    use crate::fs::vfs::{normalize_path, EntryType, Vfs};
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
            files.push(LayerFile { path: target, data, mode: file_mode(&meta) });
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
                                mode: file_mode(&cmeta),
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

    let mut spec = ImageSpec::new();
    // Build-time variables (ARG defaults + ENV), used for `${VAR}` expansion.
    let mut vars: Vec<(String, String)> = Vec::new();
    // Base-image layer blobs carried forward verbatim (FROM <dir>).
    let mut base_layer_descs: Vec<Descriptor> = Vec::new();
    let mut base_diff_ids: Vec<String> = Vec::new();
    let mut base_dir: Option<String> = None;
    let mut from_seen = false;

    for (line, logical) in &instrs {
        let line = *line;
        let (instr, rest_raw) = match logical.split_once(char::is_whitespace) {
            Some((a, b)) => (a, b.trim()),
            None => (logical.as_str(), ""),
        };
        let instr_up = instr.to_ascii_uppercase();

        // ARG may legally precede FROM (a "global" build arg).
        if instr_up == "ARG" {
            let expanded = expand_vars(rest_raw, &vars);
            let (name, default) = match expanded.split_once('=') {
                Some((n, v)) => (String::from(n.trim()), String::from(v)),
                None => (String::from(expanded.trim()), String::new()),
            };
            if !name.is_empty() {
                // A `--build-arg NAME=value` override supersedes the default,
                // but only for a NAME the Dockerfile actually declares (here).
                let value = build_args
                    .iter()
                    .rev()
                    .find(|(k, _)| *k == name)
                    .map_or(default, |(_, v)| v.clone());
                vars.push((name, value));
            }
            continue;
        }

        if !from_seen && instr_up != "FROM" {
            return Err(BuildError::MissingFrom);
        }

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
                    let base = load_image(base_ref).map_err(BuildError::Kernel)?;
                    spec.architecture = base.config.architecture.clone();
                    spec.os = base.config.os.clone();
                    spec.env = base.config.env.clone();
                    spec.cmd = base.config.cmd.clone();
                    spec.entrypoint = base.config.entrypoint.clone();
                    spec.working_dir = base.config.working_dir.clone();
                    spec.user = base.config.user.clone();
                    spec.exposed_ports = base.config.exposed_ports.clone();
                    spec.labels = base.config.labels.clone();
                    // Seed vars with the inherited ENV so `${VAR}` sees them.
                    for e in &base.config.env {
                        if let Some((k, v)) = e.split_once('=') {
                            vars.push((String::from(k), String::from(v)));
                        }
                    }
                    base_layer_descs = base.manifest.layers.clone();
                    base_diff_ids = base.config.diff_ids.clone();
                    if base_layer_descs.len() != base_diff_ids.len() {
                        return Err(BuildError::Parse {
                            line,
                            msg: String::from("base image layer/diff_id count mismatch"),
                        });
                    }
                    base_dir = Some(String::from(base_ref));
                }
                from_seen = true;
            }
            "RUN" => return Err(BuildError::RunUnsupported { line }),
            "CMD" => {
                spec.cmd = parse_cmd_form(rest_raw);
            }
            "ENTRYPOINT" => {
                spec.entrypoint = parse_cmd_form(rest_raw);
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
                // Drop leading flag tokens (e.g. --chown=, --chmod=).
                toks.retain(|t| !t.starts_with("--"));
                if toks.len() < 2 {
                    return Err(BuildError::Parse {
                        line,
                        msg: String::from("COPY/ADD needs at least one source and a destination"),
                    });
                }
                let dest_path = toks.last().cloned().unwrap_or_default();
                let src_count = toks.len().saturating_sub(1);
                let single = src_count == 1;
                let mut files: Vec<LayerFile> = Vec::new();
                for src in toks.iter().take(src_count) {
                    collect_copy_src(context_dir, src, &dest_path, single, &ignore, &mut files, line)?;
                }
                spec.layers.push(BuildLayer { files });
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
        assert_eq!(image.manifest.layers.len(), 2);
        assert_eq!(image.config.diff_ids.len(), 2);

        // Extract both layers and verify the copied files survived.
        let _ = Vfs::mkdir(ext);
        for layer in &image.manifest.layers {
            extract_layer(img, layer, ext)?;
        }
        let run = Vfs::read_file(&format!("{ext}/srv/app/run.sh"))?;
        assert_eq!(run, b"#!/bin/sh\necho serving\n");
        let readme = Vfs::read_file(&format!("{ext}/srv/readme.txt"))?;
        assert_eq!(readme, b"read me");

        // RUN must be rejected as deferred (Q17), not silently ignored.
        let with_run = b"FROM scratch\nRUN echo hi\n";
        match build_image(with_run, ctx, "/tmp/oci_build_run") {
            Err(BuildError::RunUnsupported { line }) => assert_eq!(line, 2),
            other => panic!("expected RunUnsupported, got {:?}", other.is_ok()),
        }

        // FROM <local image> inherits base layers + config, appends a layer.
        let df2 = b"FROM /tmp/oci_build_img\nENV EXTRA=1\nCOPY readme.txt /srv/readme2.txt\n";
        build_image(df2, ctx, img2).map_err(|e| {
            serial_println!("[oci] inherit build failed: {}", e.describe());
            KernelError::InternalError
        })?;
        let child = load_image(img2)?;
        assert_eq!(child.manifest.layers.len(), 3, "2 inherited + 1 new layer");
        assert!(child.config.env.iter().any(|e| e == "PATH=/usr/bin"), "inherited env");
        assert!(child.config.env.iter().any(|e| e == "EXTRA=1"), "new env");
        assert_eq!(child.config.entrypoint, alloc::vec![String::from("/srv/app/run.sh")]);

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

    serial_println!("[oci] Self-test PASSED (13 tests)");
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
