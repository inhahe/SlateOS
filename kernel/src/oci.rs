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

    serial_println!("[oci] Self-test PASSED (8 tests)");
    Ok(())
}
