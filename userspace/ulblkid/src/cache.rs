//! libblkid's device cache: the block devices libblkid knows of and the
//! tags (LABEL, UUID, TYPE, ...) found on each, kept between runs in
//! `/run/blkid/blkid.tab` -- `cache.c`, `read.c`, `save.c`, `dev.c`,
//! `tag.c`, `verify.c`, `devname.c`, `resolve.c` and `config.c` of
//! util-linux 2.39.3, with upstream's names.
//!
//! What a caller sees depends on who it runs as, and the port keeps both:
//!
//! * **root** can open every device, so a device is re-probed ("verified")
//!   whenever its entry is more than two seconds old, `probe_all` walks
//!   `/sys/block` and probes every partition, and the cache file is written
//!   back when anything changed -- as upstream writes it.
//! * **anyone else** gets `EACCES` from each open, and libblkid then returns
//!   what the cache file says, unverified; devices it had never seen are
//!   known by name only, with no tags. The file itself is not written.
//!
//! Devices are addressed by a stable index into the cache. Upstream's lists
//! keep insertion order, and two orders show: the devices' (iteration, and
//! the file's lines), and, for each tag name, the order its tags were set
//! across all devices (which of two devices with the same UUID a lookup
//! finds first); both are kept -- the second as a sequence number per tag.

use crate::devno::{devno_to_devname, scan_dir};
use crate::probe::open_nonblock;
use crate::{PARTS_ENTRY_DETAILS, Probe, SUBLKS_LABEL, SUBLKS_SECTYPE, SUBLKS_TYPE, SUBLKS_UUID};
use std::path::PathBuf;
use std::rc::Rc;
use ulsysfs::canonicalize::{canonicalize_dm_name, canonicalize_path};
use ulsysfs::{major, makedev, minor};

/// `BLKID_DEV_FIND`: look a device up only.
pub const DEV_FIND: u32 = 0;
/// `BLKID_DEV_CREATE`: add a device not known yet.
pub const DEV_CREATE: u32 = 1;
/// `BLKID_DEV_VERIFY`: make sure what is known is still true.
pub const DEV_VERIFY: u32 = 2;
/// `BLKID_DEV_NORMAL`.
pub const DEV_NORMAL: u32 = DEV_CREATE | DEV_VERIFY;

/// `BLKID_BID_FL_VERIFIED`: probed from the disk this run.
const BID_FL_VERIFIED: u32 = 0x0001;
/// `BLKID_BID_FL_REMOVABLE`: added by `blkid_probe_all_removable`.
const BID_FL_REMOVABLE: u32 = 0x0008;
/// `BLKID_BIC_FL_PROBED`: `/sys/block` was walked.
const BIC_FL_PROBED: u32 = 0x0002;
/// `BLKID_BIC_FL_CHANGED`: the cache differs from its file.
const BIC_FL_CHANGED: u32 = 0x0004;
/// `BLKID_PROBE_MIN`: an entry younger than this many seconds is trusted.
const PROBE_MIN: i64 = 2;
/// `BLKID_PROBE_INTERVAL`: `/sys/block` is walked at most this often.
const PROBE_INTERVAL: i64 = 200;
/// `BLKID_PRI_*`: which of two devices with one tag a lookup prefers.
const PRI_UBI: i32 = 50;
const PRI_DM: i32 = 40;
const PRI_LVM: i32 = 20;
const PRI_MD: i32 = 10;

/// `BLKID_CONFIG_FILE`.
const CONFIG_FILE: &[u8] = b"/etc/blkid.conf";
/// `BLKID_RUNTIME_TOPDIR`, `BLKID_RUNTIME_DIR`.
const RUNTIME_TOPDIR: &[u8] = b"/run";
const RUNTIME_DIR: &[u8] = b"/run/blkid";
/// `BLKID_CACHE_FILE`, `BLKID_CACHE_FILE_OLD`.
const CACHE_FILE: &[u8] = b"/run/blkid/blkid.tab";
const CACHE_FILE_OLD: &[u8] = b"/etc/blkid.tab";
/// `VG_DIR`, `PROC`'s LVM1 tree.
const VG_DIR: &[u8] = b"/proc/lvm/VGs";
/// `dirlist[]`: where a device's node is looked for by its name.
const DIRLIST: [&[u8]; 3] = [b"/dev", b"/devfs", b"/devices"];

/// A path from bytes.
fn path_of(b: &[u8]) -> PathBuf {
    PathBuf::from(quoting::os_from_bytes(b))
}

/// `a/b`.
fn join(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut p = a.to_vec();
    p.push(b'/');
    p.extend_from_slice(b);
    p
}

/// The environment variable (`safe_getenv`).
fn env(name: &str) -> Option<Vec<u8>> {
    std::env::var_os(name).map(|v| quoting::os_bytes(&v).into_owned())
}

/// `time(NULL)` and the microseconds of `gettimeofday`.
fn now() -> (i64, i64) {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => (
            i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
            i64::from(d.subsec_micros()),
        ),
        Err(_) => (0, 0),
    }
}

/// `BLKID_EVAL_*`: how a tag is turned into a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eval {
    /// The `/dev/disk/by-*` links.
    Udev,
    /// This cache, probing every device if need be.
    Scan,
}

/// `struct blkid_config`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub uevent: bool,
    pub cachefile: Option<Vec<u8>>,
    pub evals: Vec<Eval>,
}

/// `parse_next(fd, conf)` of `/etc/blkid.conf`: `Ok(false)` at the end.
fn parse_config_line(line: &[u8], conf: &mut Config, uevent: &mut i32) -> Result<(), ()> {
    let s = line
        .get(
            line.iter()
                .take_while(|&&b| b == b' ' || b == b'\t')
                .count()..,
        )
        .unwrap_or_default();
    if let Some(v) = s.strip_prefix(b"SEND_UEVENT=") {
        if !v.is_empty() {
            *uevent = i32::from(v.eq_ignore_ascii_case(b"yes"));
        }
    } else if let Some(v) = s.strip_prefix(b"CACHE_FILE=") {
        conf.cachefile = (!v.is_empty()).then(|| v.to_vec());
    } else if let Some(v) = s.strip_prefix(b"EVALUATE=") {
        if !v.is_empty() {
            // `parse_evaluate`: at most two methods in all, across lines.
            for m in v.split(|&b| b == b',') {
                if conf.evals.len() >= 2 {
                    return Err(());
                }
                conf.evals.push(match m {
                    b"udev" => Eval::Udev,
                    b"scan" => Eval::Scan,
                    _ => return Err(()),
                });
            }
        }
    } else {
        return Err(());
    }
    Ok(())
}

/// `blkid_read_config(filename)`: `/etc/blkid.conf` (or `BLKID_CONF`, or
/// `filename`), and the defaults for what it does not say -- `EVALUATE=udev,scan`,
/// the cache in `/run/blkid/blkid.tab`, uevents sent. `None` when the file
/// does not parse.
#[must_use]
pub fn read_config(filename: Option<&[u8]>) -> Option<Config> {
    let filename = filename
        .map(<[u8]>::to_vec)
        .or_else(|| env("BLKID_CONF"))
        .unwrap_or_else(|| CONFIG_FILE.to_vec());
    let mut conf = Config {
        uevent: true,
        cachefile: None,
        evals: Vec::new(),
    };
    let mut uevent = -1i32;
    if let Ok(text) = std::fs::read(path_of(&filename)) {
        // `fgets(buf, BUFSIZ)` line by line: a line with no newline is an
        // error unless it is the last; `\r` before the newline is dropped;
        // blank and `#` lines are skipped.
        let mut rest: &[u8] = &text;
        while !rest.is_empty() {
            let (line, next, had_nl) = match rest.iter().position(|&b| b == b'\n') {
                Some(i) if i < 8191 => (
                    rest.get(..i).unwrap_or_default(),
                    rest.get(i.saturating_add(1)..).unwrap_or_default(),
                    true,
                ),
                Some(_) => return None,
                None if rest.len() < 8192 => (rest, &b""[..], false),
                None => return None,
            };
            let _ = had_nl;
            rest = next;
            let line = crate::c_str(line);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let s = line
                .get(
                    line.iter()
                        .take_while(|&&b| b == b' ' || b == b'\t')
                        .count()..,
                )
                .unwrap_or_default();
            if s.is_empty() || s.first() == Some(&b'#') {
                continue;
            }
            parse_config_line(s, &mut conf, &mut uevent).ok()?;
        }
    }
    if conf.evals.is_empty() {
        conf.evals = vec![Eval::Udev, Eval::Scan];
    }
    if conf.cachefile.is_none() {
        conf.cachefile = Some(CACHE_FILE.to_vec());
    }
    conf.uevent = uevent != 0;
    Some(conf)
}

/// `get_default_cache_filename()`: `/run/blkid/blkid.tab` where there is a
/// `/run`, else the old `/etc/blkid.tab`.
fn default_cache_filename() -> Vec<u8> {
    if std::fs::metadata(path_of(RUNTIME_TOPDIR)).is_ok_and(|m| m.is_dir()) {
        CACHE_FILE.to_vec()
    } else {
        CACHE_FILE_OLD.to_vec()
    }
}

/// `blkid_get_cache_filename(conf)`: `BLKID_FILE`, or the configuration's
/// -- which may name none.
#[must_use]
pub fn cache_filename(conf: Option<&Config>) -> Option<Vec<u8>> {
    match conf {
        Some(c) => env("BLKID_FILE").or_else(|| c.cachefile.clone()),
        None => Some(default_filename()),
    }
}

/// `blkid_get_cache_filename(NULL)`: `BLKID_FILE`, else the configured
/// file (`/run/blkid/blkid.tab` unless `/etc/blkid.conf` says otherwise),
/// else -- the configuration not parsing -- `/run/blkid/blkid.tab` where
/// there is a `/run`, `/etc/blkid.tab` where there is not.
#[must_use]
pub fn default_filename() -> Vec<u8> {
    if let Some(f) = env("BLKID_FILE") {
        return f;
    }
    match read_config(None) {
        Some(c) => c.cachefile.unwrap_or_else(|| CACHE_FILE.to_vec()),
        None => default_cache_filename(),
    }
}

/// `struct blkid_struct_tag`, on its device: the name, the value, and when
/// it joined its name's list across devices.
#[derive(Clone, Debug)]
struct Tag {
    name: Vec<u8>,
    val: Vec<u8>,
    seq: u64,
}

/// `struct blkid_struct_dev`.
#[derive(Clone, Debug, Default)]
struct Dev {
    name: Vec<u8>,
    /// The name it was asked for by, when that was not its canonical one.
    xname: Option<Vec<u8>>,
    devno: u64,
    time: i64,
    utime: i64,
    pri: i32,
    flags: u32,
    tags: Vec<Tag>,
}

impl Dev {
    fn tag(&self, name: &[u8]) -> Option<&Tag> {
        self.tags.iter().find(|t| t.name == name)
    }
}

/// `struct blkid_struct_cache`.
#[derive(Debug)]
pub struct BlkCache {
    /// Every device ever added, by index; `None` once freed.
    devs: Vec<Option<Dev>>,
    /// The live devices, in list order.
    order: Vec<usize>,
    filename: Vec<u8>,
    /// `bic_ftime`: the cache file's mtime when it was read.
    ftime: Option<i64>,
    flags: u32,
    /// `bic_time`: when `/sys/block` was walked.
    time: i64,
    /// The next tag's place in its name's list.
    next_seq: u64,
}

impl BlkCache {
    /// `blkid_get_cache(&cache, filename)`: a cache read from `filename`,
    /// or from the configured file when `None` or empty.
    #[must_use]
    pub fn get_cache(filename: Option<&[u8]>) -> Self {
        let filename = match filename.filter(|f| !f.is_empty()) {
            Some(f) => f.to_vec(),
            None => default_filename(),
        };
        let mut cache = BlkCache {
            devs: Vec::new(),
            order: Vec::new(),
            filename,
            ftime: None,
            flags: 0,
            time: 0,
            next_seq: 0,
        };
        cache.read_cache();
        cache
    }

    fn dev(&self, id: usize) -> Option<&Dev> {
        self.devs.get(id).and_then(Option::as_ref)
    }

    fn dev_mut(&mut self, id: usize) -> Option<&mut Dev> {
        self.devs.get_mut(id).and_then(Option::as_mut)
    }

    /// `blkid_new_dev` and `list_add_tail`.
    fn add_dev(&mut self, dev: Dev) -> usize {
        let id = self.devs.len();
        self.devs.push(Some(dev));
        self.order.push(id);
        id
    }

    /// `blkid_free_dev(dev)`.
    fn free_dev(&mut self, id: usize) {
        if let Some(slot) = self.devs.get_mut(id) {
            *slot = None;
        }
        self.order.retain(|&x| x != id);
    }

    /// `blkid_dev_devname(dev)`: the name it was asked for by, else its own.
    #[must_use]
    pub fn devname(&self, id: usize) -> Option<&[u8]> {
        let d = self.dev(id)?;
        Some(d.xname.as_deref().unwrap_or(&d.name))
    }

    /// `blkid_tag_iterate_begin(dev)` and `blkid_tag_next`: the device's
    /// tags, `(name, value)`, in the order they were first set.
    #[must_use]
    pub fn tags(&self, id: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.dev(id)
            .map(|d| {
                d.tags
                    .iter()
                    .map(|t| (t.name.clone(), t.val.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `blkid_gc_cache(cache)`: forget every device whose node no longer
    /// exists.
    pub fn gc_cache(&mut self) {
        for id in self.dev_ids() {
            let gone = self
                .dev(id)
                .is_some_and(|d| std::fs::metadata(path_of(&d.name)).is_err());
            if gone {
                self.free_dev(id);
                self.flags |= BIC_FL_CHANGED;
            }
        }
    }

    /// The live devices, in list order (`blkid_dev_iterate_begin`).
    #[must_use]
    pub fn dev_ids(&self) -> Vec<usize> {
        self.order.clone()
    }

    /// `blkid_dev_has_tag(dev, type, value)`: `value` `None` for any.
    #[must_use]
    pub fn dev_has_tag(&self, id: usize, ty: &[u8], value: Option<&[u8]>) -> bool {
        let Some(t) = self.dev(id).and_then(|d| d.tag(ty)) else {
            return false;
        };
        value.is_none_or(|v| t.val == v)
    }

    /// The next device from `cursor` on with tag `ty`=`value` (or any
    /// device without a search): `blkid_dev_next`. `cursor` is the index in
    /// the list the walk has reached; a device freed behind it does not
    /// move it.
    pub fn dev_next(
        &self,
        cursor: &mut Option<usize>,
        search: Option<(&[u8], &[u8])>,
    ) -> Option<usize> {
        // The walk remembers the device it will visit next, as upstream's
        // list pointer does.
        loop {
            let id = (*cursor)?;
            let pos = self.order.iter().position(|&x| x == id)?;
            *cursor = self.order.get(pos.saturating_add(1)).copied();
            if let Some((t, v)) = search
                && !self.dev_has_tag(id, t, Some(v))
            {
                continue;
            }
            return Some(id);
        }
    }

    /// The cursor at the start of the list.
    #[must_use]
    pub fn dev_iter_begin(&self) -> Option<usize> {
        self.order.first().copied()
    }

    /// `blkid_set_tag(dev, name, value)`: set, change or (`None`) remove a
    /// tag. A changed value keeps the tag's place in its name's list.
    fn set_tag(&mut self, id: usize, name: &[u8], value: Option<&[u8]>) {
        let seq = self.next_seq;
        let Some(dev) = self.dev_mut(id) else {
            return;
        };
        let pos = dev.tags.iter().position(|t| t.name == name);
        match (value, pos) {
            (None, Some(p)) => {
                dev.tags.remove(p);
            }
            (None, None) => {}
            (Some(v), Some(p)) => {
                if let Some(t) = dev.tags.get_mut(p) {
                    if t.val == v {
                        return;
                    }
                    t.val = v.to_vec();
                }
            }
            (Some(v), None) => {
                dev.tags.push(Tag {
                    name: name.to_vec(),
                    val: v.to_vec(),
                    seq,
                });
                self.next_seq = self.next_seq.saturating_add(1);
            }
        }
        self.flags |= BIC_FL_CHANGED;
    }

    /// `blkid_read_cache(cache)`: the cache file's devices, unless it was
    /// read at this mtime already or the cache has changed since.
    pub fn read_cache(&mut self) {
        let Ok(meta) = std::fs::metadata(path_of(&self.filename)) else {
            return;
        };
        #[cfg(unix)]
        let mtime = {
            use std::os::unix::fs::MetadataExt;
            meta.mtime()
        };
        #[cfg(not(unix))]
        let mtime = {
            let _ = &meta;
            0i64
        };
        if self.ftime == Some(mtime) || self.flags & BIC_FL_CHANGED != 0 {
            return;
        }
        let Ok(text) = std::fs::read(path_of(&self.filename)) else {
            return;
        };
        // `fgets(buf, 4096)`: lines of at most 4095 bytes; a line ending in
        // a backslash is continued, the backslash overwritten.
        let mut rest: &[u8] = &text;
        while !rest.is_empty() {
            let mut buf = Vec::new();
            loop {
                let room = 4095usize.saturating_sub(buf.len());
                let take = match rest.iter().position(|&b| b == b'\n') {
                    Some(i) if i < room => i.saturating_add(1),
                    _ => room.min(rest.len()),
                };
                buf.extend_from_slice(rest.get(..take).unwrap_or_default());
                rest = rest.get(take..).unwrap_or_default();
                let line = crate::c_str(&buf);
                let end = line.len().saturating_sub(1);
                if line.is_empty()
                    || end >= 4094
                    || line.get(end) != Some(&b'\\')
                    || rest.is_empty()
                {
                    buf.truncate(line.len());
                    break;
                }
                buf.truncate(end);
            }
            if buf.is_empty() {
                continue;
            }
            self.parse_line(&buf);
        }
        self.flags &= !BIC_FL_CHANGED;
        self.ftime = Some(mtime);
    }

    /// `blkid_parse_line(cache, &dev, line)`: one `<device ...>` line; a
    /// device with no TYPE is dropped.
    fn parse_line(&mut self, line: &[u8]) {
        let Some((name, mut rest)) = parse_dev(line) else {
            return;
        };
        let Some(id) = self.get_dev(&name, DEV_CREATE) else {
            return;
        };
        while let Some((tname, value, after)) = parse_token(rest) {
            rest = after;
            match tname.as_slice() {
                b"DEVNO" => {
                    if let Some(d) = self.dev_mut(id) {
                        d.devno = strtoull0(&value).0;
                    }
                }
                b"PRI" => {
                    if let Some(d) = self.dev_mut(id) {
                        d.pri = strtol0(&value);
                    }
                }
                b"TIME" => {
                    let (t, end) = strtoull0(&value);
                    if let Some(d) = self.dev_mut(id) {
                        d.time = t as i64;
                        if value.get(end) == Some(&b'.') {
                            d.utime =
                                strtoull0(value.get(end.saturating_add(1)..).unwrap_or_default()).0
                                    as i64;
                        }
                    }
                }
                _ => self.set_tag(id, &tname, Some(&value)),
            }
        }
        if self.dev(id).is_some_and(|d| d.tag(b"TYPE").is_none()) {
            self.free_dev(id);
        }
    }

    /// `blkid_get_dev(cache, devname, flags)`: the device by its name (or
    /// its canonical name, which then keeps the asked-for one), created if
    /// asked and it exists; verified if asked -- and then every unverified
    /// entry that looks like the same filesystem is checked too, and freed
    /// if it no longer is.
    pub fn get_dev(&mut self, devname: &[u8], flags: u32) -> Option<usize> {
        let mut found = self
            .order
            .iter()
            .copied()
            .find(|&i| self.dev(i).is_some_and(|d| d.name == devname));
        let mut cn: Option<Vec<u8>> = None;
        if found.is_none()
            && let Some(c) = canonicalize_path(devname)
            && c != devname
        {
            found = self
                .order
                .iter()
                .copied()
                .find(|&i| self.dev(i).is_some_and(|d| d.name == c));
            if let Some(i) = found
                && let Some(d) = self.dev_mut(i)
            {
                d.xname = Some(devname.to_vec());
            }
            cn = Some(c);
        }
        if found.is_none() && flags & DEV_CREATE != 0 {
            // `access(devname, F_OK)`.
            std::fs::metadata(path_of(devname)).ok()?;
            let dev = match cn.take() {
                Some(c) => Dev {
                    name: c,
                    xname: Some(devname.to_vec()),
                    time: i64::MIN,
                    ..Dev::default()
                },
                None => Dev {
                    name: devname.to_vec(),
                    time: i64::MIN,
                    ..Dev::default()
                },
            };
            found = Some(self.add_dev(dev));
            self.flags |= BIC_FL_CHANGED;
        }
        if flags & DEV_VERIFY != 0 {
            let id = self.verify(found?)?;
            if self.dev(id).is_none_or(|d| d.flags & BID_FL_VERIFIED == 0) {
                return Some(id);
            }
            let (ty, label, uuid) = {
                let d = self.dev(id)?;
                (
                    d.tag(b"TYPE").map(|t| t.val.clone()),
                    d.tag(b"LABEL").map(|t| t.val.clone()),
                    d.tag(b"UUID").map(|t| t.val.clone()),
                )
            };
            for other in self.order.clone() {
                let Some(d2) = self.dev(other) else {
                    continue;
                };
                if d2.flags & BID_FL_VERIFIED != 0 {
                    continue;
                }
                let t2 = d2.tag(b"TYPE").map(|t| t.val.clone());
                let l2 = d2.tag(b"LABEL").map(|t| t.val.clone());
                let u2 = d2.tag(b"UUID").map(|t| t.val.clone());
                match (&ty, &t2) {
                    (Some(a), Some(b)) if a == b => {}
                    _ => continue,
                }
                if matches!((&label, &l2), (Some(a), Some(b)) if a != b)
                    || matches!((&uuid, &u2), (Some(a), Some(b)) if a != b)
                {
                    continue;
                }
                if label.is_some() != l2.is_some() || uuid.is_some() != u2.is_some() {
                    continue;
                }
                if let Some(v) = self.verify(other)
                    && self.dev(v).is_some_and(|d| d.flags & BID_FL_VERIFIED == 0)
                {
                    self.free_dev(v);
                }
            }
            return Some(id);
        }
        found
    }

    /// `blkid_verify(cache, dev)`: re-probe the device unless its entry is
    /// under two seconds old and the node has not changed since. A device
    /// that cannot be read (no permission, gone) is returned unverified,
    /// with what the cache knew; one that probes as nothing is freed.
    pub fn verify(&mut self, id: usize) -> Option<usize> {
        let (name, time, utime) = {
            let d = self.dev(id)?;
            (d.name.clone(), d.time, d.utime)
        };
        let (now_s, _) = now();
        // `diff = (uintmax_t) now - dev->bid_time`, back into a `time_t`.
        let diff = (now_s as u64).wrapping_sub(time as u64) as i64;
        let meta = match std::fs::metadata(path_of(&name)) {
            Ok(m) => m,
            Err(e) => return self.open_err(id, &e),
        };
        #[cfg(unix)]
        let (mtime, mtime_us, rdev) = {
            use std::os::unix::fs::MetadataExt;
            (meta.mtime(), meta.mtime_nsec() / 1000, meta.rdev())
        };
        #[cfg(not(unix))]
        let (mtime, mtime_us, rdev) = {
            let _ = &meta;
            (0i64, 0i64, 0u64)
        };
        if now_s >= time
            && (mtime < time || (mtime == time && mtime_us <= utime))
            && (0..PROBE_MIN).contains(&diff)
        {
            if let Some(d) = self.dev_mut(id) {
                d.flags |= BID_FL_VERIFIED;
            }
            return Some(id);
        }
        if devno_is_dm_private(rdev) {
            self.free_dev(id);
            return None;
        }
        let file = match open_nonblock(&name) {
            Ok(f) => f,
            Err(e) => return self.open_err(id, &std::io::Error::from_raw_os_error(e)),
        };
        // A fresh probe for each device, where upstream reuses the cache's
        // one: `blkid_probe_set_device` resets everything this asks about,
        // and the filters are reset after each use.
        let mut pr = Probe::new();
        if pr.set_device(Some(Rc::new(file)), 0, 0) != 0 {
            // The device cannot be read.
            self.free_dev(id);
            return None;
        }
        // Remove what the cache knew, then probe.
        let old: Vec<Vec<u8>> = self
            .dev(id)
            .map(|d| d.tags.iter().map(|t| t.name.clone()).collect())
            .unwrap_or_default();
        for name in old {
            self.set_tag(id, &name, None);
        }
        pr.enable_superblocks(true);
        pr.set_superblocks_flags(SUBLKS_LABEL | SUBLKS_UUID | SUBLKS_TYPE | SUBLKS_SECTYPE);
        pr.enable_partitions(true);
        pr.set_partitions_flags(PARTS_ENTRY_DETAILS);
        if pr.do_safeprobe() != 0 {
            // Found nothing, or an error.
            self.free_dev(id);
            return None;
        }
        let values: Vec<(&'static str, Vec<u8>)> = pr
            .values()
            .iter()
            .map(|v| (v.name, v.as_c_str().to_vec()))
            .collect();
        let (s, us) = now();
        if let Some(d) = self.dev_mut(id) {
            d.time = s;
            d.utime = us;
            d.devno = rdev;
            d.flags |= BID_FL_VERIFIED;
        }
        self.flags |= BIC_FL_CHANGED;
        // `blkid_probe_to_tags`: partition entries' UUID and NAME as
        // PARTUUID and PARTLABEL, the rest but for `*_ID` as they are.
        for (vname, v) in values {
            if let Some(p) = vname.strip_prefix("PART_ENTRY_") {
                match p {
                    "UUID" => self.set_tag(id, b"PARTUUID", Some(&v)),
                    "NAME" => self.set_tag(id, b"PARTLABEL", Some(&v)),
                    _ => {}
                }
            } else if !vname.contains("_ID") {
                self.set_tag(id, vname.as_bytes(), Some(&v));
            }
        }
        Some(id)
    }

    /// `open_err:` of `blkid_verify`: no permission, or gone, keeps what the
    /// cache knew; anything else frees the device.
    fn open_err(&mut self, id: usize, e: &std::io::Error) -> Option<usize> {
        // EPERM, ENOENT, EACCES.
        if matches!(e.raw_os_error(), Some(1 | 2 | 13)) {
            return Some(id);
        }
        self.free_dev(id);
        None
    }

    /// `blkid_find_dev_with_tag(cache, type, value)`: the device of highest
    /// priority (the first set, among equals) with the tag, that exists --
    /// verified if it can be; failing that, after probing new devices, then
    /// all of them.
    pub fn find_dev_with_tag(&mut self, ty: &[u8], value: &[u8]) -> Option<usize> {
        self.read_cache();
        let (mut probe_new, mut probe_all) = (false, false);
        loop {
            let mut best: Option<(usize, i32)> = None;
            let mut tags: Vec<(u64, usize)> = self
                .order
                .iter()
                .filter_map(|&i| {
                    self.dev(i)
                        .and_then(|d| d.tag(ty))
                        .filter(|t| t.val == value)
                        .map(|t| (t.seq, i))
                })
                .collect();
            tags.sort_unstable();
            for (_, i) in tags {
                let Some(d) = self.dev(i) else {
                    continue;
                };
                let pri = d.pri;
                if best.is_none_or(|(_, p)| pri > p) && std::fs::metadata(path_of(&d.name)).is_ok()
                {
                    best = Some((i, pri));
                }
            }
            let mut dev = best.map(|(i, _)| i);
            if let Some(i) = dev
                && self.dev(i).is_some_and(|d| d.flags & BID_FL_VERIFIED == 0)
            {
                dev = self.verify(i);
                if dev.is_none_or(|v| self.dev(v).is_some_and(|d| d.flags & BID_FL_VERIFIED != 0)) {
                    continue;
                }
            }
            if dev.is_none() && !probe_new {
                if self.probe_all_new() < 0 {
                    return None;
                }
                probe_new = true;
                continue;
            }
            if dev.is_none() && !probe_all && self.flags & BIC_FL_PROBED == 0 {
                if self.probe_all() < 0 {
                    return None;
                }
                probe_all = true;
                continue;
            }
            return dev;
        }
    }

    /// `blkid_get_tag_value(cache, tagname, devname)`.
    pub fn get_tag_value(&mut self, tagname: &[u8], devname: &[u8]) -> Option<Vec<u8>> {
        let id = self.get_dev(devname, DEV_NORMAL)?;
        self.dev(id)?.tag(tagname).map(|t| t.val.clone())
    }

    /// `blkid_get_devname(cache, token, value)`: the device with the tag.
    pub fn get_devname(&mut self, token: &[u8], value: &[u8]) -> Option<Vec<u8>> {
        let id = self.find_dev_with_tag(token, value)?;
        self.dev(id).map(|d| d.name.clone())
    }

    /// `blkid_probe_all(cache)`.
    pub fn probe_all(&mut self) -> i32 {
        self.probe_all_inner(false, true)
    }

    /// `blkid_probe_all_new(cache)`.
    pub fn probe_all_new(&mut self) -> i32 {
        self.probe_all_inner(true, false)
    }

    /// `probe_all(cache, only_if_new, update_interval)`: LVM1 volumes, UBI
    /// volumes and `/sys/block`'s disks and partitions, at most once every
    /// 200 seconds; then the cache file written if it changed.
    fn probe_all_inner(&mut self, only_if_new: bool, update_interval: bool) -> i32 {
        if self.flags & BIC_FL_PROBED != 0 && now().0.saturating_sub(self.time) < PROBE_INTERVAL {
            return 0;
        }
        self.read_cache();
        self.lvm_probe_all(only_if_new);
        self.ubi_probe_all(only_if_new);
        let rc = self.sysfs_probe_all(only_if_new);
        if update_interval && rc == 0 {
            self.time = now().0;
            self.flags |= BIC_FL_PROBED;
        }
        let _ = self.flush();
        0
    }

    /// `lvm_probe_all`: `/proc/lvm/VGs/*/LVs/*`, LVM1's.
    fn lvm_probe_all(&mut self, only_if_new: bool) {
        let Ok(vgs) = std::fs::read_dir(path_of(VG_DIR)) else {
            return;
        };
        for vg in vgs.flatten() {
            let vg_name = quoting::os_bytes(&vg.file_name()).into_owned();
            let lvdir = join(&join(VG_DIR, &vg_name), b"LVs");
            let Ok(lvs) = std::fs::read_dir(path_of(&lvdir)) else {
                continue;
            };
            for lv in lvs.flatten() {
                let lv_name = quoting::os_bytes(&lv.file_name()).into_owned();
                let devno = lvm_get_devno(&join(&lvdir, &lv_name));
                self.probe_one(
                    &join(&vg_name, &lv_name),
                    devno,
                    PRI_LVM,
                    only_if_new,
                    false,
                );
            }
        }
    }

    /// `ubi_probe_all`: UBI volumes' character nodes in the device
    /// directories.
    fn ubi_probe_all(&mut self, only_if_new: bool) {
        for dir in DIRLIST {
            let Ok(entries) = std::fs::read_dir(path_of(dir)) else {
                continue;
            };
            for e in entries.flatten() {
                let name = quoting::os_bytes(&e.file_name()).into_owned();
                let kind_ok = e.file_type().is_ok_and(|t| is_char(&t) || t.is_symlink());
                if !kind_ok || !name.windows(3).any(|w| w == b"ubi") || name == b"ubi_ctrl" {
                    continue;
                }
                let Ok(m) = std::fs::metadata(path_of(&join(dir, &name))) else {
                    continue;
                };
                let rdev = rdev_of(&m);
                if !is_char(&m.file_type()) || minor(rdev) == 0 {
                    continue;
                }
                self.probe_one(&name, rdev, PRI_UBI, only_if_new, false);
            }
        }
    }

    /// `sysfs_probe_all(cache, only_if_new, 0)`: each disk in `/sys/block`
    /// that has a size, and is not a removable one without partitions; its
    /// partitions (not extended ones), or the disk itself when it has none.
    fn sysfs_probe_all(&mut self, only_if_new: bool) -> i32 {
        let Ok(disks) = std::fs::read_dir("/sys/block") else {
            return -9;
        };
        for disk in disks.flatten() {
            let dname = quoting::os_bytes(&disk.file_name()).into_owned();
            let devno = sysfs_devname_to_devno(&dname, None);
            if devno == 0 {
                continue;
            }
            let dir = format!(
                "/sys/dev/block/{}:{}",
                major(devno) as i32,
                minor(devno) as i32
            );
            if std::fs::read_dir(&dir).is_err() {
                continue;
            }
            let read_u64 = |p: &str| -> Option<u64> {
                read_trimmed(p).and_then(|t| std::str::from_utf8(&t).ok()?.parse().ok())
            };
            let size = read_u64(&format!("{dir}/size")).unwrap_or(0);
            let removable = read_u64(&format!("{dir}/removable")).unwrap_or(0);
            if size == 0 {
                continue;
            }
            let maxparts = read_u64(&format!("{dir}/ext_range")).unwrap_or(0);
            if maxparts == 0 && removable != 0 {
                continue;
            }
            let Ok(parts) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut nparts = 0usize;
            for part in parts.flatten() {
                let pname = quoting::os_bytes(&part.file_name()).into_owned();
                if !is_partition_dirent(&dir, &part, &pname, &dname) {
                    continue;
                }
                // An extended partition is 1 KiB, two sectors.
                if read_u64(&format!("{dir}/{}/size", String::from_utf8_lossy(&pname)))
                    .is_some_and(|s| s >> 1 == 1)
                {
                    continue;
                }
                let partno = sysfs_devname_to_devno(&pname, Some(&dname));
                if partno == 0 {
                    continue;
                }
                nparts = nparts.saturating_add(1);
                self.probe_one(&pname, partno, 0, only_if_new, false);
            }
            if nparts == 0 {
                self.probe_one(&dname, devno, 0, only_if_new, false);
            } else if let Some(i) = self
                .order
                .iter()
                .copied()
                .find(|&i| self.dev(i).is_some_and(|d| d.devno == devno))
            {
                // A partitioned disk is not itself a filesystem.
                self.free_dev(i);
                self.flags |= BIC_FL_CHANGED;
            }
        }
        0
    }

    /// `probe_one(cache, ptname, devno, pri, only_if_new, removable)`: the
    /// device with this number into the cache, found by name in the device
    /// directories or by searching for the number, and verified.
    fn probe_one(
        &mut self,
        ptname: &[u8],
        devno: u64,
        pri: i32,
        only_if_new: bool,
        removable: bool,
    ) {
        let mut dev: Option<usize> = None;
        for id in self.order.clone() {
            let Some(d) = self.dev(id) else {
                continue;
            };
            if d.devno != devno {
                continue;
            }
            if only_if_new && std::fs::metadata(path_of(&d.name)).is_ok() {
                return;
            }
            dev = self.verify(id);
            if dev.is_some_and(|v| self.dev(v).is_some_and(|d| d.flags & BID_FL_VERIFIED != 0)) {
                break;
            }
            dev = None;
        }
        let found = if dev.is_some_and(|v| self.dev(v).is_some_and(|d| d.devno == devno)) {
            dev
        } else {
            let devname = self.find_devname(ptname, devno);
            match devname {
                Some(Ok(id)) => Some(id),
                Some(Err(name)) => self.get_dev(&name, DEV_NORMAL),
                None => return,
            }
        };
        let Some(id) = found else {
            return;
        };
        let dm_leaf = is_dm_leaf(ptname);
        if let Some(d) = self.dev_mut(id) {
            if pri != 0 {
                d.pri = pri;
            } else if d.name.starts_with(b"/dev/mapper/") {
                d.pri = PRI_DM.saturating_add(if dm_leaf { 5 } else { 0 });
            } else if ptname.starts_with(b"md") {
                d.pri = PRI_MD;
            }
            if removable {
                d.flags |= BID_FL_REMOVABLE;
            }
        }
    }

    /// The part of `probe_one` that finds the device's node: `Ok` for a
    /// cache entry already under that name with that number, `Err` for a
    /// node to add, `None` for no node at all.
    fn find_devname(&mut self, ptname: &[u8], devno: u64) -> Option<Result<usize, Vec<u8>>> {
        if ptname.starts_with(b"dm-") && ptname.get(3).is_some_and(u8::is_ascii_digit) {
            let name =
                canonicalize_dm_name(ptname).or_else(|| scan_dir(b"/dev/mapper", devno, None));
            if let Some(n) = name {
                return Some(Err(n));
            }
        }
        for dir in DIRLIST {
            let mut device = join(dir, ptname);
            device.truncate(255);
            if let Some(id) = self.get_dev(&device, DEV_FIND)
                && self.dev(id).is_some_and(|d| d.devno == devno)
            {
                return Some(Ok(id));
            }
            if let Ok(m) = std::fs::metadata(path_of(&device)) {
                let ft = m.file_type();
                if (is_block(&ft) || (is_char(&ft) && ptname.starts_with(b"ubi")))
                    && rdev_of(&m) == devno
                {
                    return Some(Err(device));
                }
            }
        }
        scan_dir(b"/dev/mapper", devno, None)
            .or_else(|| devno_to_devname(devno))
            .map(Err)
    }

    /// `blkid_flush_cache(cache)`: the cache written back to its file --
    /// through a temporary file, keeping the old one as `.old` -- when it
    /// changed and the file may be written. Returns upstream's code.
    pub fn flush(&mut self) -> i32 {
        if self.order.is_empty() || self.flags & BIC_FL_CHANGED == 0 {
            return 0;
        }
        let filename = self.filename.clone();
        if filename.is_empty() {
            return -22;
        }
        if filename.starts_with(RUNTIME_DIR) && filename.get(RUNTIME_DIR.len()) == Some(&b'/') {
            // The default place: make the directory if need be.
            if let Err(e) = std::fs::metadata(path_of(RUNTIME_DIR))
                && e.kind() == std::io::ErrorKind::NotFound
                && let Err(e) = make_runtime_dir()
                && e.kind() != std::io::ErrorKind::AlreadyExists
            {
                return 0;
            }
        }
        let meta = std::fs::metadata(path_of(&filename));
        match &meta {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return 0,
            Ok(_) if !writable(&filename) => return 0,
            _ => {}
        }
        let mut text = Vec::new();
        for &id in &self.order {
            let Some(d) = self.dev(id) else {
                continue;
            };
            if d.tag(b"TYPE").is_none()
                || d.flags & BID_FL_REMOVABLE != 0
                || d.name.first() != Some(&b'/')
            {
                continue;
            }
            save_dev(d, &mut text);
        }
        let regular = meta.as_ref().is_ok_and(std::fs::Metadata::is_file);
        let written = if regular {
            write_via_temp(&filename, &text)
        } else {
            None
        };
        let ok = match written {
            Some(r) => r,
            None => std::fs::write(path_of(&filename), &text).is_ok(),
        };
        if ok {
            self.flags &= !BIC_FL_CHANGED;
            1
        } else {
            0
        }
    }
}

impl BlkCache {
    /// Let the cache go unwritten, as a program that never calls
    /// `blkid_put_cache` lets it go (`findmnt`'s SOURCES cache).
    pub fn abandon(mut self) {
        self.flags &= !BIC_FL_CHANGED;
    }
}

impl Drop for BlkCache {
    /// `blkid_put_cache`: written back before it goes.
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// `mkdir(BLKID_RUNTIME_DIR, 0755)`.
fn make_runtime_dir() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o755)
            .create(path_of(RUNTIME_DIR))
    }
    #[cfg(not(unix))]
    {
        std::fs::DirBuilder::new().create(path_of(RUNTIME_DIR))
    }
}

/// `access(filename, W_OK)`.
fn writable(filename: &[u8]) -> bool {
    std::fs::OpenOptions::new()
        .append(true)
        .open(path_of(filename))
        .is_ok()
}

/// The temporary-file half of `blkid_flush_cache`: `FILE-XXXXXX`, mode
/// 0644, then the old file linked to `FILE.old` and the new renamed over
/// it. `None` when no temporary file could be made (the caller then writes
/// the file directly); `Some(false)` when the rename failed.
fn write_via_temp(filename: &[u8], text: &[u8]) -> Option<bool> {
    let pid = std::process::id();
    let mut tmp = filename.to_vec();
    tmp.extend_from_slice(format!("-{:06}", pid % 1_000_000).as_bytes());
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path_of(&tmp))
        .ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if file
            .set_permissions(std::fs::Permissions::from_mode(0o644))
            .is_err()
        {
            drop(file);
            let _ = std::fs::remove_file(path_of(&tmp));
            return None;
        }
    }
    let mut file = file;
    // Upstream notes a failed write only in its debug output, and renames
    // what it wrote over the cache all the same.
    let _ = std::io::Write::write_all(&mut file, text);
    drop(file);
    let mut backup = filename.to_vec();
    backup.extend_from_slice(b".old");
    // Upstream ignores both: a missing old file, or one that will not link.
    let _ = std::fs::remove_file(path_of(&backup));
    let _ = std::fs::hard_link(path_of(filename), path_of(&backup));
    Some(std::fs::rename(path_of(&tmp), path_of(filename)).is_ok())
}

/// `save_dev(dev, file)`: `<device DEVNO="0x0801" TIME="s.us" PRI="n"
/// NAME="value" ...>name</device>`.
fn save_dev(d: &Dev, out: &mut Vec<u8>) {
    out.extend_from_slice(
        format!(
            "<device DEVNO=\"0x{:04x}\" TIME=\"{}.{}\"",
            d.devno, d.time, d.utime
        )
        .as_bytes(),
    );
    if d.pri != 0 {
        out.extend_from_slice(format!(" PRI=\"{}\"", d.pri).as_bytes());
    }
    for t in &d.tags {
        out.push(b' ');
        out.extend_from_slice(&t.name);
        out.extend_from_slice(b"=\"");
        for &b in crate::c_str(&t.val) {
            if b == b'"' || b == b'\\' {
                out.push(b'\\');
            }
            out.push(b);
        }
        out.push(b'"');
    }
    out.push(b'>');
    out.extend_from_slice(&d.name);
    out.extend_from_slice(b"</device>\n");
}

/// C's `isspace`.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `skip_over_blank`.
fn skip_blank(s: &[u8]) -> &[u8] {
    s.get(s.iter().take_while(|&&b| is_space(b)).count()..)
        .unwrap_or_default()
}

/// `skip_over_word`: to a blank, `<` or `>`, a backslash escaping the byte
/// after it.
fn word_end(s: &[u8]) -> usize {
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        if c == b'\\' {
            i = i.saturating_add(1);
            if s.get(i).is_none() {
                break;
            }
            i = i.saturating_add(1);
            continue;
        }
        if is_space(c) || c == b'<' || c == b'>' {
            break;
        }
        i = i.saturating_add(1);
    }
    i
}

/// `strip_line`: blanks off both ends.
fn strip(s: &[u8]) -> &[u8] {
    let s = skip_blank(s);
    let end = s
        .iter()
        .rposition(|&b| !is_space(b))
        .map_or(0, |i| i.saturating_add(1));
    s.get(..end).unwrap_or_default()
}

/// `parse_dev(cache, &dev, &cp)`: `<device ...>NAME</device>`'s name, and
/// the attribute text to parse tags from. `None` for a comment, a blank
/// line, another element, or a malformed one.
fn parse_dev(line: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let p = strip(line);
    if p.is_empty() || p.first() == Some(&b'#') {
        return None;
    }
    let attrs_start = p.strip_prefix(b"<device")?;
    let gt = attrs_start.iter().position(|&b| b == b'>')?;
    // The attributes start one byte past `<device` (its blank), or are
    // none at all for `<device>`.
    let attrs = if gt == 0 {
        &b""[..]
    } else {
        attrs_start.get(1..gt)?
    };
    let body = skip_blank(attrs_start.get(gt.saturating_add(1)..)?);
    let end = word_end(body);
    // `if (end - start <= 1)`: a name of one byte or none is refused.
    if end <= 1 {
        return None;
    }
    Some((body.get(..end)?.to_vec(), attrs))
}

/// `parse_token(&name, &value, &cp)`: the next `NAME="value"` (or
/// `NAME=value`) and what follows it; `None` when no `=` is left or the
/// quote does not close.
fn parse_token(cp: &[u8]) -> Option<(Vec<u8>, Vec<u8>, &[u8])> {
    let eq = cp.iter().position(|&b| b == b'=')?;
    let name = strip(cp.get(..eq)?).to_vec();
    let rest = skip_blank(cp.get(eq.saturating_add(1)..)?);
    if rest.first() == Some(&b'"') {
        // `foo\"bar` is `foo"bar`.
        let mut value = Vec::new();
        let mut i = 1usize;
        loop {
            match rest.get(i) {
                None => return None,
                Some(b'\\') => {
                    i = i.saturating_add(1);
                    value.push(*rest.get(i)?);
                }
                Some(b'"') => break,
                Some(&c) => value.push(c),
            }
            i = i.saturating_add(1);
        }
        Some((
            name,
            value,
            rest.get(i.saturating_add(1)..).unwrap_or_default(),
        ))
    } else {
        let end = word_end(rest);
        let value = rest.get(..end)?.to_vec();
        let next = if end < rest.len() {
            rest.get(end.saturating_add(1)..).unwrap_or_default()
        } else {
            &b""[..]
        };
        Some((name, value, next))
    }
}

/// `strtoull(value, &end, 0)`: the value (0x, 0 and decimal) and where the
/// digits ended.
fn strtoull0(s: &[u8]) -> (u64, usize) {
    match ulstrutils::scan_integer(s, 0) {
        Some(sc) => {
            let m = if sc.saturated {
                u64::MAX
            } else {
                u64::try_from(sc.magnitude).unwrap_or(u64::MAX)
            };
            (if sc.negative { m.wrapping_neg() } else { m }, sc.end)
        }
        None => (0, 0),
    }
}

/// `strtol(value, NULL, 0)` into an `int`.
fn strtol0(s: &[u8]) -> i32 {
    let (v, _) = strtoull0(s);
    v as i64 as i32
}

/// `S_ISBLK`.
fn is_block(ft: &std::fs::FileType) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        ft.is_block_device()
    }
    #[cfg(not(unix))]
    {
        let _ = ft;
        false
    }
}

/// `S_ISCHR`.
fn is_char(ft: &std::fs::FileType) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        ft.is_char_device()
    }
    #[cfg(not(unix))]
    {
        let _ = ft;
        false
    }
}

/// `st_rdev`.
fn rdev_of(m: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.rdev()
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        0
    }
}

/// A sysfs attribute's first line.
fn read_trimmed(path: &str) -> Option<Vec<u8>> {
    let t = std::fs::read(path).ok()?;
    let line = t.split(|&b| b == b'\n').next().unwrap_or_default();
    Some(line.to_vec())
}

/// `read_devno(path)`: `fscanf(f, "%d:%d")` of a `dev` file.
fn read_devno(path: &[u8]) -> u64 {
    let Ok(t) = std::fs::read(path_of(path)) else {
        return 0;
    };
    let s = |t: &[u8]| -> Option<(i64, usize)> {
        let sc = ulstrutils::scan_integer(t, 10)?;
        let m = i64::try_from(sc.magnitude).unwrap_or(i64::MAX);
        Some((if sc.negative { m.saturating_neg() } else { m }, sc.end))
    };
    let Some((maj, end)) = s(&t) else {
        return 0;
    };
    if t.get(end) != Some(&b':') {
        return 0;
    }
    let Some((min, _)) = s(t.get(end.saturating_add(1)..).unwrap_or_default()) else {
        return 0;
    };
    makedev(maj as i32 as u32, min as i32 as u32)
}

/// `__sysfs_devname_to_devno(NULL, name, parent)`: a device's number from
/// sysfs -- `/sys/block/PARENT/NAME/dev` for a partition, else
/// `/sys/block/NAME/dev`, then `/sys/block/NAME/device/dev`.
fn sysfs_devname_to_devno(name: &[u8], parent: Option<&[u8]>) -> u64 {
    let mut name = name;
    if let Some(rest) = name.strip_prefix(b"/dev/") {
        if let Ok(m) = std::fs::metadata(path_of(name)) {
            return rdev_of(&m);
        }
        name = rest;
    }
    let sys = |n: &[u8]| -> Vec<u8> {
        n.iter()
            .map(|&b| if b == b'/' { b'!' } else { b })
            .collect()
    };
    let n = sys(name);
    if let Some(p) = parent
        && !name.starts_with(b"dm-")
    {
        return read_devno(&[b"/sys/block/", &sys(p)[..], b"/", &n[..], b"/dev"].concat());
    }
    let mut dev = read_devno(&[b"/sys/block/", &n[..], b"/dev"].concat());
    if dev == 0
        && let Some(p) = parent
        && name.starts_with(p)
    {
        dev = read_devno(&[b"/sys/block/", p, b"/", &n[..], b"/dev"].concat());
    }
    if dev == 0 {
        dev = read_devno(&[b"/sys/block/", &n[..], b"/device/dev"].concat());
    }
    dev
}

/// `sysfs_blkdev_is_partition_dirent(dir, d, parent_name)`: named
/// `PARENT<digit>` or `PARENTp<digit>`, or -- named otherwise -- with a
/// readable `start` file.
fn is_partition_dirent(dir: &str, e: &std::fs::DirEntry, name: &[u8], parent: &[u8]) -> bool {
    if !e.file_type().is_ok_and(|t| t.is_dir() || t.is_symlink()) {
        return false;
    }
    let len = if name.len() > parent.len() && name.starts_with(parent) {
        parent.len()
    } else {
        0
    };
    if len > 0 {
        let c = name.get(len).copied().unwrap_or(0);
        return (c == b'p'
            && name
                .get(len.saturating_add(1))
                .is_some_and(u8::is_ascii_digit))
            || c.is_ascii_digit();
    }
    std::fs::File::open(format!("{dir}/{}/start", String::from_utf8_lossy(name))).is_ok()
}

/// `sysfs_devno_is_dm_private(devno)`: an LVM volume's private device
/// (`LVM-<uuid>-<name>`) or a private Stratis one.
fn devno_is_dm_private(devno: u64) -> bool {
    let path = format!(
        "/sys/dev/block/{}:{}/dm/uuid",
        major(devno) as i32,
        minor(devno) as i32
    );
    let Some(id) = read_trimmed(&path) else {
        return false;
    };
    if let Some(rest) = id.strip_prefix(b"LVM-") {
        return rest
            .iter()
            .rposition(|&b| b == b'-')
            .is_some_and(|p| p.saturating_add(1) < rest.len());
    }
    id.starts_with(b"stratis-1-private")
}

/// `is_dm_leaf(devname)`: a device-mapper device nothing is stacked on.
fn is_dm_leaf(devname: &[u8]) -> bool {
    let path = format!("/sys/block/{}/holders", String::from_utf8_lossy(devname));
    match std::fs::read_dir(path) {
        Ok(mut d) => d.next().is_none(),
        Err(_) => false,
    }
}

/// `lvm_get_devno(path)`: the `device: MAJ:MIN` line of an LVM1 volume.
fn lvm_get_devno(path: &[u8]) -> u64 {
    let Ok(t) = std::fs::read(path_of(path)) else {
        return 0;
    };
    for line in t.split(|&b| b == b'\n') {
        if let Some(rest) = line.strip_prefix(b"device:") {
            let rest = skip_blank(rest);
            if let Some((maj, min)) = rest.split_once_colon() {
                return makedev(maj as u32, min as u32);
            }
        }
    }
    0
}

/// `%d:%d`.
trait SplitColon {
    fn split_once_colon(&self) -> Option<(i32, i32)>;
}

impl SplitColon for [u8] {
    fn split_once_colon(&self) -> Option<(i32, i32)> {
        let a = ulstrutils::scan_integer(self, 10)?;
        if self.get(a.end) != Some(&b':') {
            return None;
        }
        let b = ulstrutils::scan_integer(self.get(a.end.saturating_add(1)..)?, 10)?;
        let v = |sc: &ulstrutils::Scanned| {
            let m = i64::try_from(sc.magnitude).unwrap_or(i64::MAX);
            (if sc.negative { m.saturating_neg() } else { m }) as i32
        };
        Some((v(&a), v(&b)))
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "tests index what they built"
)]
mod tests {
    use super::*;

    fn scratch_cache(text: &str) -> (scratchdir::ScratchDir, BlkCache) {
        let dir = scratchdir::ScratchDir::new("blkidcache");
        let file = dir.path("blkid.tab");
        std::fs::write(&file, text).unwrap();
        let name = quoting::os_bytes(file.as_os_str()).into_owned();
        let cache = BlkCache::get_cache(Some(&name));
        (dir, cache)
    }

    #[test]
    fn config_lines_parse_as_upstream() {
        let mut c = Config {
            uevent: true,
            cachefile: None,
            evals: Vec::new(),
        };
        let mut ue = -1;
        assert!(parse_config_line(b"EVALUATE=scan", &mut c, &mut ue).is_ok());
        assert!(parse_config_line(b"EVALUATE=udev", &mut c, &mut ue).is_ok());
        assert_eq!(c.evals, vec![Eval::Scan, Eval::Udev]);
        // A third method, across lines, is an error.
        assert!(parse_config_line(b"EVALUATE=udev", &mut c, &mut ue).is_err());
        assert!(parse_config_line(b"CACHE_FILE=/x", &mut c, &mut ue).is_ok());
        assert_eq!(c.cachefile.as_deref(), Some(&b"/x"[..]));
        assert!(parse_config_line(b"SEND_UEVENT=No", &mut c, &mut ue).is_ok());
        assert_eq!(ue, 0);
        assert!(parse_config_line(b"BOGUS=1", &mut c, &mut ue).is_err());
    }

    #[test]
    fn tokens_parse_as_upstream() {
        let (n, v, rest) = parse_token(b" UUID=\"a\\\"b\" TYPE=ext4").unwrap();
        assert_eq!((n.as_slice(), v.as_slice()), (&b"UUID"[..], &b"a\"b"[..]));
        let (n, v, rest) = parse_token(rest).unwrap();
        assert_eq!(
            (n.as_slice(), v.as_slice(), rest),
            (&b"TYPE"[..], &b"ext4"[..], &b""[..])
        );
        assert!(parse_token(b"LABEL=\"open").is_none());
        let (name, attrs) =
            parse_dev(b"  <device DEVNO=\"0x0801\" TYPE=\"ext4\">/dev/sda1</device>\n").unwrap();
        assert_eq!(name, b"/dev/sda1");
        assert_eq!(attrs, b"DEVNO=\"0x0801\" TYPE=\"ext4\"");
        assert!(parse_dev(b"# comment").is_none());
        assert!(parse_dev(b"<device TYPE=\"x\">/</device>").is_none());
    }

    #[test]
    fn a_cache_file_is_read_and_written_back_the_same() {
        let (dir, c) = scratch_cache(
            "<device DEVNO=\"0x0801\" TIME=\"1700000000.5\" LABEL=\"root\" UUID=\"u1\" TYPE=\"ext4\">/nonexistent-blkid/sda1</device>\n\
             <device DEVNO=\"0x0802\" TIME=\"1\" UUID=\"u2\">/nonexistent-blkid/sda2</device>\n",
        );
        // Devices that do not exist are not created from the file.
        assert!(c.dev_ids().is_empty());
        drop(c);
        let _ = dir;
    }

    #[test]
    fn devices_are_saved_in_upstreams_format() {
        let d = Dev {
            name: b"/dev/sda1".to_vec(),
            devno: 0x801,
            time: 1_700_000_000,
            utime: 5,
            pri: 40,
            tags: vec![
                Tag {
                    name: b"LABEL".to_vec(),
                    val: b"a\"b\\c".to_vec(),
                    seq: 0,
                },
                Tag {
                    name: b"TYPE".to_vec(),
                    val: b"ext4".to_vec(),
                    seq: 1,
                },
            ],
            ..Dev::default()
        };
        let mut out = Vec::new();
        save_dev(&d, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "<device DEVNO=\"0x0801\" TIME=\"1700000000.5\" PRI=\"40\" LABEL=\"a\\\"b\\\\c\" TYPE=\"ext4\">/dev/sda1</device>\n"
        );
    }

    #[test]
    fn a_lookup_prefers_priority_then_the_first_tag_set() {
        let mut c = BlkCache {
            devs: Vec::new(),
            order: Vec::new(),
            filename: Vec::new(),
            ftime: None,
            flags: BIC_FL_PROBED,
            time: i64::MAX,
            next_seq: 0,
        };
        let here = std::env::current_dir().unwrap();
        let exists = quoting::os_bytes(here.as_os_str()).into_owned();
        let a = c.add_dev(Dev {
            name: exists.clone(),
            flags: BID_FL_VERIFIED,
            ..Dev::default()
        });
        let b = c.add_dev(Dev {
            name: exists,
            flags: BID_FL_VERIFIED,
            pri: 40,
            ..Dev::default()
        });
        c.set_tag(a, b"UUID", Some(b"x"));
        c.set_tag(b, b"UUID", Some(b"x"));
        assert_eq!(c.find_dev_with_tag(b"UUID", b"x"), Some(b));
        c.dev_mut(b).unwrap().pri = 0;
        assert_eq!(c.find_dev_with_tag(b"UUID", b"x"), Some(a));
        let mut cursor = c.dev_iter_begin();
        assert_eq!(c.dev_next(&mut cursor, Some((b"UUID", b"x"))), Some(a));
        c.free_dev(a);
        assert_eq!(c.dev_next(&mut cursor, Some((b"UUID", b"x"))), Some(b));
        assert_eq!(c.dev_next(&mut cursor, None), None);
        // Nothing to write: the cache has no file.
        c.flags &= !BIC_FL_CHANGED;
    }
}
