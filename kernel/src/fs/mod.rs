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

pub mod acl;
#[allow(dead_code)]
pub mod ar;
pub mod atime;
pub mod archive;
pub mod audit;
pub mod associations;
pub mod backup;
pub mod batch;
pub mod bench;
#[allow(dead_code)]
pub mod bzip2;
pub mod cache;
pub mod cas;
pub mod changetrack;
pub mod compress;
#[allow(dead_code)]
pub mod cpio;
pub mod dedup;
pub mod devfs;
pub mod directio;
pub mod dirsync;
pub mod encrypt;
pub mod ext4;
pub mod fat;
pub mod fcompress;
pub mod fstrim;
pub mod handle;
pub mod health;
pub mod history;
pub mod index;
pub mod intercept;
pub mod ioprio;
pub mod integrity;
pub mod iso9660;
pub mod journal;
pub mod linkcheck;
#[allow(dead_code)]
pub mod lz4;
pub mod memfs;
pub mod mime;
pub mod mount_ns;
pub mod notify;
pub mod overlay;
pub mod pipe;
pub mod procfs;
pub mod policy;
pub mod prefetch;
pub mod profile;
pub mod quota;
pub mod readdir_plus;
pub mod reclaim;
pub mod search;
pub mod snapshot;
pub mod sparse;
pub mod splice;
#[allow(dead_code)]
pub mod rar;
pub mod rlimit;
pub mod symlink_security;
pub mod sysfs;
pub mod tags;
pub mod tar;
pub mod tmpwatch;
pub mod transaction;
pub mod trash;
pub mod undelete;
pub mod usage;
pub mod vfs;
#[allow(dead_code)]
pub mod sevenz;
#[allow(dead_code)]
pub mod xz;
#[allow(dead_code)]
pub mod zip;
#[allow(dead_code)]
pub mod zstd;

pub use vfs::{
    DirEntry, EntryType, FileAttr, FileMeta, FileSystem, FsInfo, LockType, Timestamp, Vfs,
    validate_path,
};
