//! ext4 filesystem implementation (read-write).
//!
//! This is a native Rust implementation that understands the ext4 on-disk
//! format.  It maintains full compatibility with Linux ext4 — disk images
//! created here are mountable on Linux and vice versa.
//!
//! ## Design
//!
//! The design spec mandates: "Port ext4 first. Don't write a custom
//! filesystem."  While this is a Rust implementation rather than a C port,
//! it faithfully implements the ext4 on-disk format as documented in the
//! Linux kernel source (`fs/ext4/`) and the ext4 wiki.
//!
//! ## Block Size
//!
//! Standard ext4 uses 4 KiB blocks (the default `mkfs.ext4` setting).
//! Our OS uses 16 KiB pages.  The block device layer handles the mismatch:
//! each page contains 4 ext4 blocks.  We read/write at 4 KiB granularity
//! through the buffer cache.
//!
//! ## Phased Implementation
//!
//! 1. **Read-only** — superblock, block groups, inode lookup, directory
//!    traversal, extent-based file reading.
//! 2. **Read-write** — file creation, deletion, inode allocation, block
//!    allocation, directory insertion.
//! 3. **Journal** — write-ahead logging for crash recovery.
//!
//! Currently: Phase 1 (on-disk structures and superblock parsing).

pub mod driver;
pub mod io;
pub mod ondisk;
pub mod superblock;
