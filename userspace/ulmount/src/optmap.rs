//! libmount's `optmap.c`: the mount options libmount knows by name -- the
//! kernel's `MS_*` flags and the userspace-only options `mount(8)` keeps
//! for itself -- which is how an fstab entry's options are split into VFS,
//! filesystem and userspace ones.
//!
//! Both maps are upstream's as Ubuntu 24.04 builds them: every `#ifdef MS_*`
//! entry is in except `sub`/`nosub`, whose `MS_NOSUB` the C library there
//! does not define (checked against its `libmount.so.1`).

/// `MS_*`.
pub const MS_RDONLY: u64 = 1;
pub const MS_NOSUID: u64 = 2;
pub const MS_NODEV: u64 = 4;
pub const MS_NOEXEC: u64 = 8;
pub const MS_SYNCHRONOUS: u64 = 16;
pub const MS_REMOUNT: u64 = 32;
pub const MS_MANDLOCK: u64 = 64;
pub const MS_DIRSYNC: u64 = 128;
pub const MS_NOSYMFOLLOW: u64 = 256;
pub const MS_NOATIME: u64 = 1024;
pub const MS_NODIRATIME: u64 = 2048;
pub const MS_BIND: u64 = 4096;
pub const MS_MOVE: u64 = 8192;
pub const MS_REC: u64 = 16384;
pub const MS_SILENT: u64 = 32768;
pub const MS_UNBINDABLE: u64 = 1 << 17;
pub const MS_PRIVATE: u64 = 1 << 18;
pub const MS_SLAVE: u64 = 1 << 19;
pub const MS_SHARED: u64 = 1 << 20;
pub const MS_RELATIME: u64 = 1 << 21;
pub const MS_I_VERSION: u64 = 1 << 23;
pub const MS_STRICTATIME: u64 = 1 << 24;
pub const MS_LAZYTIME: u64 = 1 << 25;
/// `MS_SECURE`: what `user` implies.
pub const MS_SECURE: u64 = MS_NOEXEC | MS_NOSUID | MS_NODEV;
/// `MS_OWNERSECURE`: what `owner` and `group` imply.
pub const MS_OWNERSECURE: u64 = MS_NOSUID | MS_NODEV;

/// `MNT_INVERT`: the option clears its flag.
pub const MNT_INVERT: u32 = 1 << 1;
/// `MNT_NOMTAB`.
pub const MNT_NOMTAB: u32 = 1 << 2;
/// `MNT_PREFIX`: the name is a prefix (`x-`).
pub const MNT_PREFIX: u32 = 1 << 3;
/// `MNT_NOHLPS`.
pub const MNT_NOHLPS: u32 = 1 << 4;
/// `MNT_NOFSTAB`.
pub const MNT_NOFSTAB: u32 = 1 << 5;
/// `MNT_SUPERBLOCK`.
pub const MNT_SUPERBLOCK: u32 = 1 << 6;

/// `MNT_MS_*`: the userspace options' ids.
pub const MNT_MS_NOAUTO: u64 = 1 << 2;
pub const MNT_MS_USER: u64 = 1 << 3;
pub const MNT_MS_USERS: u64 = 1 << 4;
pub const MNT_MS_OWNER: u64 = 1 << 5;
pub const MNT_MS_GROUP: u64 = 1 << 6;
pub const MNT_MS_NETDEV: u64 = 1 << 7;
pub const MNT_MS_COMMENT: u64 = 1 << 8;
pub const MNT_MS_LOOP: u64 = 1 << 9;
pub const MNT_MS_NOFAIL: u64 = 1 << 10;
pub const MNT_MS_UHELPER: u64 = 1 << 11;
pub const MNT_MS_HELPER: u64 = 1 << 12;
pub const MNT_MS_XCOMMENT: u64 = 1 << 13;
pub const MNT_MS_OFFSET: u64 = 1 << 14;
pub const MNT_MS_SIZELIMIT: u64 = 1 << 15;
pub const MNT_MS_ENCRYPTION: u64 = 1 << 16;
pub const MNT_MS_XFSTABCOMM: u64 = 1 << 17;
pub const MNT_MS_HASH_DEVICE: u64 = 1 << 18;
pub const MNT_MS_ROOT_HASH: u64 = 1 << 19;
pub const MNT_MS_HASH_OFFSET: u64 = 1 << 20;
pub const MNT_MS_ROOT_HASH_FILE: u64 = 1 << 21;
pub const MNT_MS_FEC_DEVICE: u64 = 1 << 22;
pub const MNT_MS_FEC_OFFSET: u64 = 1 << 23;
pub const MNT_MS_FEC_ROOTS: u64 = 1 << 24;
pub const MNT_MS_ROOT_HASH_SIG: u64 = 1 << 25;
pub const MNT_MS_VERITY_ON_CORRUPTION: u64 = 1 << 26;

/// `struct libmnt_optmap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptMapEntry {
    pub name: &'static [u8],
    pub id: u64,
    pub mask: u32,
}

const fn e(name: &'static [u8], id: u64, mask: u32) -> OptMapEntry {
    OptMapEntry { name, id, mask }
}

/// `linux_flags_map[]`.
pub const LINUX_MAP: &[OptMapEntry] = &[
    e(b"ro", MS_RDONLY, 0),
    e(b"rw", MS_RDONLY, MNT_INVERT),
    e(b"exec", MS_NOEXEC, MNT_INVERT),
    e(b"noexec", MS_NOEXEC, 0),
    e(b"suid", MS_NOSUID, MNT_INVERT),
    e(b"nosuid", MS_NOSUID, 0),
    e(b"dev", MS_NODEV, MNT_INVERT),
    e(b"nodev", MS_NODEV, 0),
    e(b"sync", MS_SYNCHRONOUS, MNT_SUPERBLOCK),
    e(b"async", MS_SYNCHRONOUS, MNT_INVERT | MNT_SUPERBLOCK),
    e(b"dirsync", MS_DIRSYNC, MNT_SUPERBLOCK),
    e(b"remount", MS_REMOUNT, MNT_NOMTAB),
    e(b"bind", MS_BIND, 0),
    e(b"rbind", MS_BIND | MS_REC, 0),
    e(b"silent", MS_SILENT, 0),
    e(b"loud", MS_SILENT, MNT_INVERT),
    e(b"mand", MS_MANDLOCK, MNT_SUPERBLOCK),
    e(b"nomand", MS_MANDLOCK, MNT_INVERT | MNT_SUPERBLOCK),
    e(b"atime", MS_NOATIME, MNT_INVERT),
    e(b"noatime", MS_NOATIME, 0),
    e(b"iversion", MS_I_VERSION, 0),
    e(b"noiversion", MS_I_VERSION, MNT_INVERT),
    e(b"diratime", MS_NODIRATIME, MNT_INVERT),
    e(b"nodiratime", MS_NODIRATIME, 0),
    e(b"relatime", MS_RELATIME, 0),
    e(b"norelatime", MS_RELATIME, MNT_INVERT),
    e(b"strictatime", MS_STRICTATIME, 0),
    e(b"nostrictatime", MS_STRICTATIME, MNT_INVERT),
    e(b"lazytime", MS_LAZYTIME, MNT_SUPERBLOCK),
    e(b"nolazytime", MS_LAZYTIME, MNT_INVERT | MNT_SUPERBLOCK),
    e(b"unbindable", MS_UNBINDABLE, MNT_NOHLPS | MNT_NOMTAB),
    e(
        b"runbindable",
        MS_UNBINDABLE | MS_REC,
        MNT_NOHLPS | MNT_NOMTAB,
    ),
    e(b"private", MS_PRIVATE, MNT_NOHLPS | MNT_NOMTAB),
    e(b"rprivate", MS_PRIVATE | MS_REC, MNT_NOHLPS | MNT_NOMTAB),
    e(b"slave", MS_SLAVE, MNT_NOHLPS | MNT_NOMTAB),
    e(b"rslave", MS_SLAVE | MS_REC, MNT_NOHLPS | MNT_NOMTAB),
    e(b"shared", MS_SHARED, MNT_NOHLPS | MNT_NOMTAB),
    e(b"rshared", MS_SHARED | MS_REC, MNT_NOHLPS | MNT_NOMTAB),
    e(b"symfollow", MS_NOSYMFOLLOW, MNT_INVERT),
    e(b"nosymfollow", MS_NOSYMFOLLOW, 0),
    e(b"move", MS_MOVE, MNT_NOHLPS | MNT_NOMTAB | MNT_NOFSTAB),
];

/// `userspace_opts_map[]`.
pub const USERSPACE_MAP: &[OptMapEntry] = &[
    e(b"defaults", 0, MNT_NOHLPS),
    e(b"auto", MNT_MS_NOAUTO, MNT_NOHLPS | MNT_INVERT | MNT_NOMTAB),
    e(b"noauto", MNT_MS_NOAUTO, MNT_NOHLPS | MNT_NOMTAB),
    e(b"user[=]", MNT_MS_USER, 0),
    e(b"nouser", MNT_MS_USER, MNT_INVERT | MNT_NOMTAB),
    e(b"users", MNT_MS_USERS, MNT_NOMTAB),
    e(b"nousers", MNT_MS_USERS, MNT_INVERT | MNT_NOMTAB),
    e(b"owner", MNT_MS_OWNER, MNT_NOMTAB),
    e(b"noowner", MNT_MS_OWNER, MNT_INVERT | MNT_NOMTAB),
    e(b"group", MNT_MS_GROUP, MNT_NOMTAB),
    e(b"nogroup", MNT_MS_GROUP, MNT_INVERT | MNT_NOMTAB),
    e(b"_netdev", MNT_MS_NETDEV, 0),
    e(b"comment=", MNT_MS_COMMENT, MNT_NOHLPS | MNT_NOMTAB),
    e(b"x-", MNT_MS_XCOMMENT, MNT_NOHLPS | MNT_PREFIX),
    e(
        b"X-",
        MNT_MS_XFSTABCOMM,
        MNT_NOHLPS | MNT_NOMTAB | MNT_PREFIX,
    ),
    e(b"loop[=]", MNT_MS_LOOP, MNT_NOHLPS),
    e(b"offset=", MNT_MS_OFFSET, MNT_NOHLPS | MNT_NOMTAB),
    e(b"sizelimit=", MNT_MS_SIZELIMIT, MNT_NOHLPS | MNT_NOMTAB),
    e(b"encryption=", MNT_MS_ENCRYPTION, MNT_NOHLPS | MNT_NOMTAB),
    e(b"nofail", MNT_MS_NOFAIL, MNT_NOMTAB),
    e(b"uhelper=", MNT_MS_UHELPER, 0),
    e(b"helper=", MNT_MS_HELPER, 0),
    e(
        b"verity.hashdevice=",
        MNT_MS_HASH_DEVICE,
        MNT_NOHLPS | MNT_NOMTAB,
    ),
    e(
        b"verity.roothash=",
        MNT_MS_ROOT_HASH,
        MNT_NOHLPS | MNT_NOMTAB,
    ),
    e(
        b"verity.hashoffset=",
        MNT_MS_HASH_OFFSET,
        MNT_NOHLPS | MNT_NOMTAB,
    ),
    e(
        b"verity.roothashfile=",
        MNT_MS_ROOT_HASH_FILE,
        MNT_NOHLPS | MNT_NOMTAB,
    ),
    e(
        b"verity.fecdevice=",
        MNT_MS_FEC_DEVICE,
        MNT_NOHLPS | MNT_NOMTAB,
    ),
    e(
        b"verity.fecoffset=",
        MNT_MS_FEC_OFFSET,
        MNT_NOHLPS | MNT_NOMTAB,
    ),
    e(
        b"verity.fecroots=",
        MNT_MS_FEC_ROOTS,
        MNT_NOHLPS | MNT_NOMTAB,
    ),
    e(
        b"verity.roothashsig=",
        MNT_MS_ROOT_HASH_SIG,
        MNT_NOHLPS | MNT_NOMTAB,
    ),
    e(
        b"verity.oncorruption=",
        MNT_MS_VERITY_ON_CORRUPTION,
        MNT_NOHLPS | MNT_NOMTAB,
    ),
];

/// Which map an entry came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Map {
    Linux,
    Userspace,
}

impl Map {
    /// The map's entries.
    #[must_use]
    pub fn entries(self) -> &'static [OptMapEntry] {
        match self {
            Map::Linux => LINUX_MAP,
            Map::Userspace => USERSPACE_MAP,
        }
    }
}

/// `mnt_optmap_get_entry(maps, nmaps, name, namelen, &ent)`: the first
/// entry of the first map naming the option. `rest` is the option string
/// from the option's start -- a prefix entry (`x-`) is matched against it
/// whole -- and `namelen` the option name's length in it.
#[must_use]
pub fn get_entry(maps: &[Map], rest: &[u8], namelen: usize) -> Option<(Map, &'static OptMapEntry)> {
    let name = rest.get(..namelen).unwrap_or(rest);
    for &map in maps {
        for ent in map.entries() {
            if ent.mask & MNT_PREFIX != 0 {
                if rest.starts_with(ent.name) {
                    return Some((map, ent));
                }
                continue;
            }
            // `strncmp(ent->name, name, namelen)`: the entry starts with
            // the name (an option string, being a C string, holds no NUL).
            if ent.name.get(..namelen) != Some(name) {
                continue;
            }
            match ent.name.get(namelen) {
                None | Some(b'=' | b'[') => return Some((map, ent)),
                _ => {}
            }
        }
    }
    None
}

/// `mnt_optmap_entry_novalue(e)`: the entry takes no `=value`.
#[must_use]
pub fn entry_novalue(ent: &OptMapEntry) -> bool {
    !ent.name.contains(&b'=') && ent.mask & MNT_PREFIX == 0
}
