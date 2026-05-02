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
//! ## Mount table
//!
//! Multiple filesystems can be mounted at different paths (e.g., `/`
//! for the root FAT volume and `/tmp` for a volatile ramfs).  The VFS
//! uses longest-prefix matching to route operations to the correct
//! filesystem.

pub mod cache;
pub mod devfs;
pub mod ext4;
pub mod fat;
pub mod handle;
pub mod iso9660;
pub mod journal;
pub mod memfs;
pub mod notify;
pub mod procfs;
pub mod sysfs;
pub mod trash;
pub mod vfs;

pub use vfs::{
    DirEntry, EntryType, FileAttr, FileMeta, FileSystem, FsInfo, LockType, Timestamp, Vfs,
    validate_path,
};
