//! Virtual Filesystem (VFS) layer.
//!
//! The VFS provides a uniform interface for filesystem operations,
//! decoupling the kernel and userspace from specific filesystem
//! implementations.  All file I/O goes through VFS traits.
//!
//! ## Architecture
//!
//! ```text
//! kshell / syscalls
//!       ↓
//!   VFS (mount table, path resolution)
//!       ↓
//!   Filesystem trait impl (FAT16, ext4, …)
//!       ↓
//!   BlockDevice trait
//!       ↓
//!   driver (virtio-blk, NVMe, …)
//! ```
//!
//! ## Current limitations
//!
//! - Read-only (write support will come with ext4)
//! - No directory nesting in FAT (root directory only for now)
//! - No caching / buffer cache (each read goes to the device)
//! - Single mount point (will become a mount table)

pub mod fat16;
pub mod vfs;

pub use vfs::{DirEntry, EntryType, FileSystem, Vfs};
