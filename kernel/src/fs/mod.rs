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
pub mod appnotify;
pub mod appregistry;
pub mod appstore;
pub mod appsandbox;
pub mod associations;
pub mod audiodevice;
pub mod audiomux;
pub mod autostart;
pub mod backup;
pub mod battery;
pub mod bootcfg;
pub mod bookmarks;
pub mod brightness;
pub mod batch;
pub mod bench;
pub mod bluetooth;
#[allow(dead_code)]
pub mod bzip2;
pub mod cache;
pub mod capsettings;
pub mod cas;
pub mod certmgr;
pub mod cliphistory;
pub mod clipboard;
pub mod cloudsync;
pub mod colorpicker;
pub mod changetrack;
pub mod columnview;
pub mod compress;
pub mod contextmenu;
#[allow(dead_code)]
pub mod cpio;
pub mod crashreport;
pub mod credentials;
pub mod cursorsettings;
pub mod datausage;
pub mod dedup;
pub mod defaultapps;
pub mod deskicons;
pub mod detailcols;
pub mod devicemgr;
pub mod devfs;
pub mod dictation;
pub mod directio;
pub mod diskencrypt;
pub mod disksmart;
pub mod display;
pub mod displaycolor;
pub mod dirsync;
pub mod dragdrop;
pub mod driverupdate;
pub mod dumpanalyzer;
pub mod dyndns;
pub mod encrypt;
pub mod envvars;
pub mod ext4;
pub mod fat;
pub mod focusassist;
pub mod fcomment;
pub mod fcompress;
pub mod fontmgr;
pub mod fileinfo;
pub mod fileops;
pub mod filepicker;
pub mod fileselect;
pub mod fileshare;
pub mod filetype;
pub mod fileversion;
pub mod findex;
pub mod freeze;
pub mod fstrim;
pub mod fstune;
pub mod fwsettings;
pub mod gestures;
pub mod fswalk;
pub mod gamepadinput;
pub mod handle;
pub mod health;
pub mod hwmonitor;
pub mod hotkeys;
pub mod history;
pub mod ime;
pub mod immutable;
pub mod index;
pub mod inputa11y;
pub mod installer;
pub mod intercept;
pub mod ioprio;
pub mod integrity;
pub mod iso9660;
pub mod journal;
pub mod kbsettings;
pub mod kernelbuild;
pub mod keylayout;
pub mod langpack;
pub mod linkcheck;
pub mod locale;
pub mod location;
pub mod magnifier;
pub mod loginscreen;
#[allow(dead_code)]
pub mod lz4;
pub mod mediakeys;
pub mod memdiag;
pub mod memfs;
pub mod mime;
pub mod mmtune;
pub mod mobilelink;
pub mod monitors;
pub mod mount_ns;
pub mod mousesettings;
pub mod netdiag;
pub mod netindicator;
pub mod netshare;
pub mod netproxy;
pub mod netthrottle;
pub mod netsettings;
pub mod nightlight;
pub mod notifcenter;
pub mod notifprefs;
pub mod notify;
pub mod openwith;
pub mod osreset;
pub mod overlay;
pub mod parental;
pub mod parentaltime;
pub mod partmgr;
pub mod pathbar;
pub mod peninput;
pub mod perfmon;
pub mod pipe;
pub mod pkgmgr;
pub mod procfs;
pub mod policy;
pub mod power;
pub mod powerprofile;
pub mod prefetch;
pub mod preview;
pub mod printmgr;
pub mod printqueue;
pub mod profile;
pub mod progmgr;
pub mod properties;
pub mod queryable;
pub mod quicksettings;
pub mod quota;
pub mod readdir_plus;
pub mod recent;
pub mod remoteassist;
pub mod remotedesktop;
pub mod restorepoint;
pub mod reclaim;
pub mod rundialog;
pub mod schedtune;
pub mod screenlock;
pub mod screenreader;
pub mod screenshot;
pub mod screenrec;
pub mod screentime;
pub mod scriptlang;
pub mod search;
pub mod sealing;
pub mod servicemgr;
pub mod sessionmgr;
pub mod sidebar;
pub mod snapshot;
pub mod soundevents;
pub mod soundmixer;
pub mod sparse;
pub mod speechio;
pub mod spellcheck;
pub mod splice;
pub mod startmenu;
pub mod startuprepair;
pub mod statusbar;
pub mod storageclean;
pub mod swapcfg;
#[allow(dead_code)]
pub mod rar;
pub mod rlimit;
pub mod symlink_security;
pub mod sysdiag;
pub mod sysfs;
pub mod syslog;
pub mod sysinfo;
pub mod sysrestore;
pub mod systray;
pub mod tags;
pub mod tar;
pub mod taskbar;
pub mod tasksched;
pub mod taskmon;
pub mod templates;
pub mod theme;
pub mod timezone;
pub mod toolbar;
pub mod touchpad;
pub mod thumbcache;
pub mod tmpwatch;
pub mod transaction;
pub mod trash;
pub mod undelete;
pub mod updatemgr;
pub mod usbmgr;
pub mod usage;
pub mod useracct;
pub mod vdesktop;
pub mod vfs;
pub mod viewstate;
pub mod volumeosd;
pub mod vpn;
pub mod wakesensor;
pub mod wallpaper;
pub mod webcam;
pub mod widgets;
pub mod wintiling;
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
