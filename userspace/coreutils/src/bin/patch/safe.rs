//! `safe.c`: every file `patch` touches is reached by walking its path one
//! component at a time from the working directory, with `openat (...,
//! O_DIRECTORY | O_NOFOLLOW)`, so that a symbolic link cannot lead a patch
//! outside the tree it was run in.
//!
//! A link met on the way is read and followed by hand: one that is relative
//! is walked from where it is, and may not climb above the working directory
//! (`..` from there is `EXDEV`); one that is absolute is allowed only when it
//! points inside the working directory, and is walked as the rest of the path
//! below it. More than 1024 components, links included, is `ELOOP`.
//!
//! The directories opened on the way are kept open in a cache, keyed by the
//! parent's descriptor and the name, and trimmed least-recently-used first
//! to a quarter of the descriptor limit (not at all when that limit is
//! unlimited, as Debian's patch has it) -- whose misses `-x 32` prints, which
//! is why the cache is reproduced here and not just the walk.
//!
//! `unsafe` -- set when the file to patch was named on the command line --
//! turns every call into its plain form.

use std::collections::{HashMap, VecDeque};

use crate::sys::{self, AT_FDCWD, AT_REMOVEDIR, AT_SYMLINK_NOFOLLOW, Stat, Timespec, errno, oflag};

/// `MAX_PATH_COMPONENTS`.
const MAX_PATH_COMPONENTS: u32 = 1024;

/// One cached directory: `struct cached_dirfd`.
#[derive(Debug)]
struct Entry {
    /// The directory it was found in; `None` for the working directory.
    parent: Option<usize>,
    /// Still on its parent's list of children (`! list_empty
    /// (&children_link)`): false once the parent was dropped from the cache.
    attached: bool,
    name: Vec<u8>,
    fd: i32,
    /// In the hash table.
    hashed: bool,
}

/// The walk's state: the cache, the counters, and the flag.
#[derive(Debug)]
pub struct Safe {
    /// `unsafe`.
    pub unsafe_: bool,
    /// `debug`'s bits, for `-x 32`.
    pub debug: i32,
    /// The slab; index 0 is the working directory, which is never freed.
    entries: Vec<Option<Entry>>,
    /// `cached_dirfds`: (parent's descriptor, name) to entry.
    map: HashMap<(i32, Vec<u8>), usize>,
    /// Whether the table exists yet (`init_dirfd_cache` runs at the first
    /// insertion).
    initialized: bool,
    /// `lru_list`: the front is the most recently used.
    lru: VecDeque<usize>,
    /// `max_cached_fds`; `None` for `RLIM_INFINITY`, which trims nothing.
    max_cached: Option<usize>,
    /// `dirfd_cache_misses`.
    misses: u32,
    /// `cwd_stat_errno` and `cwd_stat`: `None` until first needed.
    cwd: Option<Result<Stat, i32>>,
}

/// A symbolic link being followed: what is left of its target.
struct Symlink {
    path: Vec<u8>,
    at: usize,
}

impl Symlink {
    fn rest(&self) -> &[u8] {
        self.path.get(self.at..).unwrap_or_default()
    }
}

fn is_slash(c: Option<&u8>) -> bool {
    c == Some(&b'/')
}

/// `count_path_components`.
fn count_path_components(path: &[u8]) -> u32 {
    let mut i = path.iter().take_while(|&&c| c == b'/').count();
    if i >= path.len() {
        return 1;
    }
    let mut components = 0u32;
    while i < path.len() {
        while path.get(i).is_some_and(|&c| c != b'/') {
            i = i.saturating_add(1);
        }
        while is_slash(path.get(i)) {
            i = i.saturating_add(1);
        }
        components = components.saturating_add(1);
    }
    components
}

impl Safe {
    pub fn new() -> Self {
        Self {
            unsafe_: false,
            debug: 0,
            entries: vec![Some(Entry {
                parent: None,
                attached: true,
                name: Vec::new(),
                fd: AT_FDCWD,
                hashed: false,
            })],
            map: HashMap::new(),
            initialized: false,
            lru: VecDeque::new(),
            max_cached: Some(MIN_CACHED_FDS),
            misses: 0,
            cwd: None,
        }
    }

    fn entry(&self, i: usize) -> Option<&Entry> {
        self.entries.get(i).and_then(Option::as_ref)
    }

    fn fd_of(&self, i: usize) -> i32 {
        self.entry(i).map_or(AT_FDCWD, |e| e.fd)
    }

    fn lru_remove(&mut self, i: usize) {
        self.lru.retain(|&x| x != i);
    }

    /// `init_dirfd_cache`: a quarter of the descriptor limit, at least
    /// [`MIN_CACHED_FDS`] -- and, by Debian's 0003, no limit at all when the
    /// descriptor limit is `RLIM_INFINITY`, where upstream sized its table
    /// from a quarter of that and failed.
    fn init_cache(&mut self) {
        self.max_cached = match open_file_limit() {
            Some(RLIM_INFINITY) => None,
            Some(cur) => Some(
                usize::try_from(cur / 4)
                    .unwrap_or(usize::MAX)
                    .max(MIN_CACHED_FDS),
            ),
            None => Some(MIN_CACHED_FDS),
        };
        self.initialized = true;
    }

    /// `lookup_cached_dirfd`.
    fn lookup(&self, dir: usize, name: &[u8]) -> Option<usize> {
        if !self.initialized {
            return None;
        }
        self.map.get(&(self.fd_of(dir), name.to_vec())).copied()
    }

    /// `remove_cached_dirfd`.
    fn remove(&mut self, i: usize) {
        let children: Vec<usize> = (0..self.entries.len())
            .filter(|&c| {
                self.entry(c)
                    .is_some_and(|e| e.parent == Some(i) && e.attached)
            })
            .collect();
        for c in children {
            self.unhash(c);
            if let Some(Some(e)) = self.entries.get_mut(c) {
                e.attached = false;
            }
        }
        self.lru_remove(i);
        self.unhash(i);
        if let Some(slot) = self.entries.get_mut(i)
            && let Some(e) = slot.take()
        {
            // The cache's own descriptor, closed once, with nothing to report.
            let _ = sys::close_fd(e.fd);
        }
    }

    fn unhash(&mut self, i: usize) {
        let key = match self.entry(i) {
            Some(e) if e.hashed => {
                let pfd = e.parent.map_or(AT_FDCWD, |p| self.fd_of(p));
                Some((pfd, e.name.clone()))
            }
            _ => None,
        };
        if let Some(k) = key
            && self.map.get(&k) == Some(&i)
        {
            self.map.remove(&k);
        }
        if let Some(Some(e)) = self.entries.get_mut(i) {
            e.hashed = false;
        }
    }

    /// `insert_cached_dirfd`.
    fn insert(&mut self, i: usize, keepfd: i32) {
        if !self.initialized {
            self.init_cache();
        }
        // Trim off the least recently used entries.
        if let Some(max) = self.max_cached {
            while self.map.len() >= max {
                let Some(mut pos) = self.lru.len().checked_sub(1) else {
                    break;
                };
                let mut last = self.lru.get(pos).copied().unwrap_or(0);
                if self.fd_of(last) == keepfd {
                    let Some(p) = pos.checked_sub(1) else {
                        break;
                    };
                    pos = p;
                    last = self.lru.get(pos).copied().unwrap_or(0);
                }
                self.remove(last);
            }
        }
        // Only insert if the parent still exists.
        let (attached, key) = match self.entry(i) {
            Some(e) => {
                let pfd = e.parent.map_or(AT_FDCWD, |p| self.fd_of(p));
                (e.attached, (pfd, e.name.clone()))
            }
            None => return,
        };
        if attached {
            self.map.insert(key, i);
            if let Some(Some(e)) = self.entries.get_mut(i) {
                e.hashed = true;
            }
        }
    }

    /// `invalidate_cached_dirfd`.
    fn invalidate(&mut self, dirfd: i32, name: &[u8]) {
        if !self.initialized {
            return;
        }
        if let Some(&i) = self.map.get(&(dirfd, name.to_vec())) {
            self.remove(i);
        }
    }

    /// `put_path`: the walked path back onto the list, deepest first.
    fn put_path(&mut self, i: usize) -> i32 {
        let fd = self.fd_of(i);
        let mut cur = Some(i);
        while let Some(c) = cur {
            let Some(parent) = self.entry(c).and_then(|e| e.parent) else {
                break;
            };
            self.lru_remove(c);
            self.lru.push_front(c);
            cur = Some(parent);
        }
        fd
    }

    /// `new_cached_dirfd`.
    fn new_entry(&mut self, dir: usize, name: &[u8], fd: i32) -> usize {
        self.entries.push(Some(Entry {
            parent: Some(dir),
            attached: true,
            name: name.to_vec(),
            fd,
            hashed: false,
        }));
        self.entries.len().saturating_sub(1)
    }

    fn free_entry(&mut self, i: usize) {
        if let Some(slot) = self.entries.get_mut(i) {
            *slot = None;
        }
        self.lru_remove(i);
    }

    /// `openat_cached`.
    fn openat_cached(&mut self, dir: usize, name: &[u8], keepfd: i32) -> Result<usize, i32> {
        if let Some(i) = self.lookup(dir, name) {
            self.lru_remove(i);
            return Ok(i);
        }
        self.misses = self.misses.wrapping_add(1);
        let fd = sys::open_at(self.fd_of(dir), name, oflag::DIRECTORY | oflag::NOFOLLOW, 0)?;
        let i = self.new_entry(dir, name, fd);
        self.insert(i, keepfd);
        Ok(i)
    }

    /// `read_symlink`: `name` in `dirfd` if it is a symbolic link, its target
    /// made relative to the working directory when it is absolute; `None`
    /// with `Err (saved)` restored when it is not a link.
    fn read_symlink(&mut self, dirfd: i32, name: &[u8], saved: i32) -> Result<Symlink, i32> {
        let st = match sys::stat_at(dirfd, name, AT_SYMLINK_NOFOLLOW) {
            Ok(st) if st.is_lnk() => st,
            _ => return Err(saved),
        };
        let size = usize::try_from(st.size).unwrap_or(0);
        let target = sys::readlink_at(dirfd, name, size)?;
        if target.is_empty() {
            return Err(errno::EINVAL);
        }
        if target.first() != Some(&b'/') {
            return Ok(Symlink {
                path: target,
                at: 0,
            });
        }
        if self.cwd.is_none() {
            self.cwd = Some(sys::stat_at(AT_FDCWD, b".", 0));
        }
        let Some(Ok(cwd)) = self.cwd else {
            return Err(errno::EXDEV);
        };
        let mut end = target.len();
        loop {
            let prefix = target.get(..end).unwrap_or_default();
            if let Ok(st) = sys::stat_at(AT_FDCWD, prefix, 0)
                && st.dev == cwd.dev
                && st.ino == cwd.ino
            {
                let mut at = end;
                while is_slash(target.get(at)) {
                    at = at.saturating_add(1);
                }
                return Ok(Symlink { path: target, at });
            }
            end = end.saturating_sub(1);
            if end == 0 {
                break;
            }
            while end != 1 && !is_slash(target.get(end)) {
                end = end.saturating_sub(1);
            }
            while end != 1 && end.checked_sub(1).is_some_and(|e| is_slash(target.get(e))) {
                end = end.saturating_sub(1);
            }
        }
        Err(errno::EXDEV)
    }

    /// `traverse_next`: one component of `path` from `*at`, inside `dir`.
    /// `Ok (entry)` with `*at` moved past it; a link found there comes back
    /// in `symlink`, with `entry` still `dir`.
    fn traverse_next(
        &mut self,
        dir: usize,
        path: &[u8],
        at: &mut usize,
        keepfd: i32,
        symlink: &mut Option<Symlink>,
    ) -> Result<usize, i32> {
        let start = *at;
        let mut p = start;
        while path.get(p).is_some_and(|&c| c != b'/') {
            p = p.saturating_add(1);
        }
        let component = path.get(start..p).unwrap_or_default();
        let entry = if component == b"." {
            dir
        } else if component == b".." {
            let Some(parent) = self.entry(dir).and_then(|e| e.parent) else {
                *at = p;
                return Err(errno::EXDEV);
            };
            self.lru_remove(dir);
            self.lru.push_front(dir);
            parent
        } else {
            match self.openat_cached(dir, component, keepfd) {
                Ok(e) => e,
                Err(e) => {
                    let r = if matches!(e, errno::ELOOP | errno::EMLINK | errno::ENOTDIR) {
                        match self.read_symlink(self.fd_of(dir), component, e) {
                            Ok(s) => {
                                *symlink = Some(s);
                                Ok(dir)
                            }
                            Err(_) => Err(errno::ELOOP),
                        }
                    } else {
                        Err(e)
                    };
                    match r {
                        Ok(d) => d,
                        Err(e) => {
                            *at = p;
                            return Err(e);
                        }
                    }
                }
            }
        };
        while is_slash(path.get(p)) {
            p = p.saturating_add(1);
        }
        *at = p;
        Ok(entry)
    }

    /// `traverse_another_path`: the directory holding `path`'s last
    /// component, and where that component starts. `AT_FDCWD` for a path
    /// with no directory part, or one that is absolute.
    fn traverse(&mut self, path: &[u8], keepfd: i32) -> Result<(i32, usize), i32> {
        let misses = self.misses;
        let mut steps = count_path_components(path);
        if steps > MAX_PATH_COMPONENTS {
            return Err(errno::ELOOP);
        }
        if path.is_empty() || path.first() == Some(&b'/') {
            return Ok((AT_FDCWD, 0));
        }
        // The last component: past any trailing slashes, back to the slash
        // before it.
        let mut last = path.len().saturating_sub(1);
        if is_slash(path.get(last)) {
            while last != 0 {
                last = last.saturating_sub(1);
                if !is_slash(path.get(last)) {
                    break;
                }
            }
        }
        while last != 0 && !is_slash(path.get(last.saturating_sub(1))) {
            last = last.saturating_sub(1);
        }
        if last == 0 {
            return Ok((AT_FDCWD, 0));
        }
        if self.debug & 32 != 0 {
            let mut m = b"Resolving path \"".to_vec();
            m.extend_from_slice(path.get(..last).unwrap_or_default());
            m.push(b'"');
            crate::util::print_stdout(&m);
        }

        let mut dir = 0usize;
        let mut stack: Vec<Symlink> = Vec::new();
        let mut at = 0usize;
        let mut traversed: Option<usize> = None;
        while !stack.is_empty() || at != last {
            let mut symlink: Option<Symlink> = None;
            let prev = at;
            let step = if let Some(top) = stack.last_mut() {
                let link_path = top.path.clone();
                let mut link_at = top.at;
                let r = self.traverse_next(dir, &link_path, &mut link_at, keepfd, &mut symlink);
                if let Some(top) = stack.last_mut() {
                    top.at = link_at;
                }
                r
            } else {
                self.traverse_next(dir, path, &mut at, keepfd, &mut symlink)
            };
            let entry = match step {
                Ok(e) => e,
                Err(e) => {
                    if self.debug & 32 != 0 {
                        crate::util::print_stdout(b" (failed)\n");
                    }
                    if let Some(t) = traversed {
                        self.free_entry(t);
                    }
                    self.put_path(dir);
                    return Err(e);
                }
            };
            dir = entry;
            if stack.is_empty() && symlink.is_some() {
                let mut p = prev;
                while path.get(p).is_some_and(|&c| c != b'/') {
                    p = p.saturating_add(1);
                }
                let name = path.get(prev..p).unwrap_or_default().to_vec();
                traversed = Some(self.new_entry(dir, &name, -1));
            }
            if stack.last().is_some_and(|t| t.rest().is_empty()) {
                stack.pop();
            }
            if let Some(s) = symlink {
                if !s.rest().is_empty() {
                    steps = steps.saturating_add(count_path_components(s.rest()));
                    stack.push(s);
                    if steps > MAX_PATH_COMPONENTS {
                        if let Some(t) = traversed {
                            self.free_entry(t);
                        }
                        self.put_path(dir);
                        return Err(errno::ELOOP);
                    }
                }
            }
            if let Some(t) = traversed
                && stack.is_empty()
            {
                let efd = self.fd_of(entry);
                let fd = if efd == AT_FDCWD {
                    Ok(AT_FDCWD)
                } else {
                    sys::dup_fd(efd)
                };
                match fd {
                    Ok(fd) => {
                        if let Some(Some(e)) = self.entries.get_mut(t) {
                            e.fd = fd;
                        }
                        self.insert(t, keepfd);
                        self.lru_remove(t);
                        self.lru.push_front(t);
                    }
                    Err(_) => self.free_entry(t),
                }
                traversed = None;
            }
        }
        if self.debug & 32 != 0 {
            let m = self.misses.wrapping_sub(misses);
            if m == 0 {
                crate::util::print_stdout(b" (cached)\n");
            } else {
                let s = if m == 1 { "" } else { "es" };
                crate::util::print_stdout(format!(" ({m} miss{s})\n").as_bytes());
            }
        }
        Ok((self.put_path(dir), last))
    }

    /// The directory and last component to act on: the plain path when
    /// `unsafe`.
    fn resolve<'a>(&mut self, path: &'a [u8], keepfd: i32) -> Result<(i32, &'a [u8]), i32> {
        if self.unsafe_ {
            return Ok((AT_FDCWD, path));
        }
        let (fd, at) = self.traverse(path, keepfd)?;
        Ok((fd, path.get(at..).unwrap_or_default()))
    }

    /// `safe_stat`.
    pub fn stat(&mut self, path: &[u8]) -> Result<Stat, i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::stat_at(fd, name, 0)
    }

    /// `safe_lstat`.
    pub fn lstat(&mut self, path: &[u8]) -> Result<Stat, i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::stat_at(fd, name, AT_SYMLINK_NOFOLLOW)
    }

    /// `safe_open`.
    pub fn open(&mut self, path: &[u8], flags: i32, mode: u32) -> Result<i32, i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::open_at(fd, name, flags | oflag::CLOEXEC, mode)
    }

    /// `safe_rename`.
    pub fn rename(&mut self, old: &[u8], new: &[u8]) -> Result<(), i32> {
        if self.unsafe_ {
            return sys::rename_at(AT_FDCWD, old, AT_FDCWD, new);
        }
        let (ofd, oname) = self.resolve(old, -1)?;
        let (nfd, nname) = self.resolve(new, ofd)?;
        sys::rename_at(ofd, oname, nfd, nname)?;
        self.invalidate(ofd, oname);
        self.invalidate(nfd, nname);
        Ok(())
    }

    /// `safe_mkdir`.
    pub fn mkdir(&mut self, path: &[u8], mode: u32) -> Result<(), i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::mkdir_at(fd, name, mode)
    }

    /// `safe_rmdir`.
    pub fn rmdir(&mut self, path: &[u8]) -> Result<(), i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::unlink_at(fd, name, AT_REMOVEDIR)?;
        if !self.unsafe_ {
            self.invalidate(fd, name);
        }
        Ok(())
    }

    /// `safe_unlink`.
    pub fn unlink(&mut self, path: &[u8]) -> Result<(), i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::unlink_at(fd, name, 0)
    }

    /// `safe_symlink`.
    pub fn symlink(&mut self, target: &[u8], path: &[u8]) -> Result<(), i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::symlink_at(target, fd, name)
    }

    /// `safe_chmod`.
    pub fn chmod(&mut self, path: &[u8], mode: u32) -> Result<(), i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::chmod_at(fd, name, mode)
    }

    /// `safe_lchown`.
    pub fn lchown(&mut self, path: &[u8], uid: u32, gid: u32) -> Result<(), i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::lchown_at(fd, name, uid, gid)
    }

    /// `safe_lutimens`.
    pub fn lutimens(&mut self, path: &[u8], times: [Timespec; 2]) -> Result<(), i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::lutimens_at(fd, name, times)
    }

    /// `safe_readlink`, the whole target.
    pub fn readlink(&mut self, path: &[u8], size: usize) -> Result<Vec<u8>, i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::readlink_at(fd, name, size)
    }

    /// `safe_access`.
    pub fn access(&mut self, path: &[u8], mode: i32) -> Result<(), i32> {
        let (fd, name) = self.resolve(path, -1)?;
        sys::access_at(fd, name, mode)
    }
}

impl Default for Safe {
    fn default() -> Self {
        Self::new()
    }
}

/// `min_cached_fds`.
const MIN_CACHED_FDS: usize = 8;

/// `RLIM_INFINITY`.
const RLIM_INFINITY: u64 = u64::MAX;

/// `getrlimit (RLIMIT_NOFILE)`'s soft limit, if it can be read.
fn open_file_limit() -> Option<u64> {
    #[cfg(unix)]
    {
        #[repr(C)]
        struct Rlimit {
            cur: u64,
            max: u64,
        }
        unsafe extern "C" {
            fn getrlimit(resource: i32, rlim: *mut Rlimit) -> i32;
        }
        /// `RLIMIT_NOFILE`.
        const RLIMIT_NOFILE: i32 = 7;
        let mut r = Rlimit { cur: 0, max: 0 };
        // SAFETY: `r` is a live `struct rlimit`, written once by the call.
        if unsafe { getrlimit(RLIMIT_NOFILE, &raw mut r) } == 0 {
            return Some(r.cur);
        }
        None
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::count_path_components;

    #[test]
    fn components_are_counted_as_safe_c_counts_them() {
        assert_eq!(count_path_components(b""), 1);
        assert_eq!(count_path_components(b"/"), 1);
        assert_eq!(count_path_components(b"a"), 1);
        assert_eq!(count_path_components(b"a/b"), 2);
        assert_eq!(count_path_components(b"//a//b//"), 2);
        assert_eq!(count_path_components(b"a/./b/../c"), 5);
    }
}
