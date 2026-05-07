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

pub mod a11y;
pub mod acl;
#[allow(dead_code)]
pub mod ar;
pub mod atime;
pub mod archive;
pub mod audit;
pub mod appregistry;
pub mod associations;
pub mod backup;
pub mod bootcfg;
pub mod bookmarks;
pub mod batch;
pub mod bench;
#[allow(dead_code)]
pub mod bzip2;
pub mod cache;
pub mod cas;
pub mod certmgr;
pub mod clipboard;
pub mod colorpicker;
pub mod changetrack;
pub mod columnview;
pub mod compress;
pub mod contextmenu;
#[allow(dead_code)]
pub mod cpio;
pub mod credentials;
pub mod cursorsettings;
pub mod dedup;
pub mod deskicons;
pub mod detailcols;
pub mod devfs;
pub mod directio;
pub mod display;
pub mod dirsync;
pub mod dragdrop;
pub mod encrypt;
pub mod ext4;
pub mod fat;
pub mod fcomment;
pub mod fcompress;
pub mod fileinfo;
pub mod fileops;
pub mod filepicker;
pub mod fileselect;
pub mod filetype;
pub mod findex;
pub mod freeze;
pub mod fstrim;
pub mod fstune;
pub mod fswalk;
pub mod handle;
pub mod health;
pub mod hotkeys;
pub mod history;
pub mod ime;
pub mod immutable;
pub mod index;
pub mod installer;
pub mod intercept;
pub mod ioprio;
pub mod integrity;
pub mod iso9660;
pub mod journal;
pub mod kbsettings;
pub mod keylayout;
pub mod linkcheck;
pub mod locale;
#[allow(dead_code)]
pub mod lz4;
pub mod memfs;
pub mod mime;
pub mod mount_ns;
pub mod netindicator;
pub mod notifcenter;
pub mod notify;
pub mod openwith;
pub mod osreset;
pub mod overlay;
pub mod partmgr;
pub mod pathbar;
pub mod pipe;
pub mod procfs;
pub mod policy;
pub mod power;
pub mod prefetch;
pub mod preview;
pub mod profile;
pub mod progmgr;
pub mod properties;
pub mod queryable;
pub mod quota;
pub mod readdir_plus;
pub mod recent;
pub mod reclaim;
pub mod rundialog;
pub mod screenshot;
pub mod scriptlang;
pub mod search;
pub mod sealing;
pub mod sidebar;
pub mod snapshot;
pub mod soundmixer;
pub mod sparse;
pub mod splice;
pub mod startmenu;
pub mod statusbar;
pub mod swapcfg;
#[allow(dead_code)]
pub mod rar;
pub mod rlimit;
pub mod symlink_security;
pub mod sysfs;
pub mod systray;
pub mod tags;
pub mod tar;
pub mod taskbar;
pub mod templates;
pub mod theme;
pub mod timezone;
pub mod toolbar;
pub mod thumbcache;
pub mod tmpwatch;
pub mod transaction;
pub mod trash;
pub mod undelete;
pub mod usage;
pub mod useracct;
pub mod vdesktop;
pub mod vfs;
pub mod viewstate;
pub mod wallpaper;
pub mod widgets;
pub mod winsnap;
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
