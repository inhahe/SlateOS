//! gnulib's `group-list.c` and `mgetgroups.c`: the group list that `id -G`,
//! `id`'s `groups=` field and `groups` all print.
//!
//! GNU compiles `src/group-list.c` into both `id` and `groups`, so the two
//! cannot disagree about which groups a user or a process has, in which
//! order, or what a missing group name looks like. This module is that file
//! and the gnulib helpers under it, moved out of `id` on 2026-09-25 when
//! `groups` arrived as its second caller -- a second copy would have been a
//! second opinion about the same question.
//!
//! The list is two different lists, and `id`'s module documentation says why
//! that is kept rather than smoothed over: with a username it is the account
//! database's answer (`getgrouplist`); without one it is the kernel's
//! (`getgroups(2)`), which says what the process *was* granted rather than
//! what the account *may* be.

use pwdb::Db;

/// The four ids a single report is about.
///
/// For a USER operand all four come from that account's passwd line, so
/// `ruid == euid` and `rgid == egid`; for the current process they are the
/// four getters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ids {
    pub ruid: u32,
    pub euid: u32,
    pub rgid: u32,
    pub egid: u32,
}

/// What a report produced: bytes for stdout, sentences for stderr, and whether
/// anything went wrong.
///
/// Names come out of the account database as bytes and may be any byte but `/`
/// and NUL, so stdout is assembled as `Vec<u8>` rather than `String`. Errors
/// are collected rather than printed so that the report functions stay pure
/// and testable — upstream's are not, which is why upstream's `ok` is a file
/// static.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Output {
    pub out: Vec<u8>,
    pub errors: Vec<String>,
    pub ok: bool,
}

impl Output {
    /// An empty report that has not failed: nothing to print, nothing to say
    /// on stderr, status 0 so far.
    pub fn new() -> Self {
        Output {
            out: Vec::new(),
            errors: Vec::new(),
            ok: true,
        }
    }

    /// Record a diagnostic and remember that the exit status is now 1.
    ///
    /// Upstream's `ok &= false` — the run continues to the next operand, which
    /// is why `id -u root nosuchuser` prints `0` *and* exits 1.
    pub fn fail(&mut self, message: String) {
        self.errors.push(message);
        self.ok = false;
    }

    /// A uid or gid in decimal, the form used when there is no name for it.
    pub fn push_number(&mut self, value: u32) {
        self.out.extend_from_slice(value.to_string().as_bytes());
    }

    /// `(name)`, the parenthesised half of `uid=0(root)`.
    pub fn push_parenthesised(&mut self, name: &[u8]) {
        self.out.push(b'(');
        self.out.extend_from_slice(name);
        self.out.push(b')');
    }
}

/// A derived `Default` would start with `ok: false` -- a report that had failed
/// before it began -- so the default is [`Output::new`], spelled out.
impl Default for Output {
    fn default() -> Self {
        Self::new()
    }
}

/// gnulib's `mgetgroups`, which is two unrelated functions sharing a name.
///
/// With a `username` it is `getgrouplist` — a query against `/etc/group`,
/// answered by [`pwdb::Db::group_list`]. Without one it is `getgroups(2)`,
/// the kernel's list for *this* process, with `gid` (the effective gid)
/// pushed on the front because some systems' `getgroups` omits it and some
/// return it twice.
///
/// That prepending is what makes the duplicate reduction below necessary, and
/// it is deliberately the same weak one gnulib uses: a single pass that drops
/// any entry equal to the first element or to its immediate predecessor.
/// gnulib documents the result as free of *pair-wise* duplicates rather than
/// minimal, having judged a sort or a hash table not worth it — and since the
/// output order is user-visible, a stronger rule would print a different list.
pub fn groups_for(username: Option<&[u8]>, gid: u32, db: &Db, process_groups: &[u32]) -> Vec<u32> {
    match username {
        Some(name) => db.group_list(name, gid),
        None => {
            let mut all = Vec::with_capacity(process_groups.len().saturating_add(1));
            all.push(gid);
            all.extend_from_slice(process_groups);
            reduce_duplicates(&all)
        }
    }
}

/// gnulib's O(n) duplicate pass. See [`groups_for`] for why it is this and not
/// a real dedup.
pub fn reduce_duplicates(groups: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(groups.len());
    for &gid in groups {
        let redundant = match (out.first(), out.last()) {
            (Some(&first), Some(&last)) => gid == first || gid == last,
            // The first element is always kept.
            _ => false,
        };
        if !redundant {
            out.push(gid);
        }
    }
    out
}

/// group-list.c's `print_group`, the mirror of `id`'s `print_user`.
pub fn print_group(out: &mut Output, gid: u32, use_name: bool, db: &Db) {
    let name = if use_name {
        let found = db.group_by_gid(gid).map(|group| group.name.clone());
        if found.is_none() {
            out.fail(format!("cannot find name for group ID {gid}"));
        }
        found
    } else {
        None
    };
    match name {
        Some(name) => out.out.extend_from_slice(&name),
        None => out.push_number(gid),
    }
}

/// group-list.c's `print_group_list`: the `-G` report.
///
/// The real gid leads, the effective one follows when it differs, and the
/// supplementary list contributes only what is neither — see the module docs
/// for why this is not the same list `groups=` prints.
///
/// The login group fed to the lookup is `getpwuid(ruid)`'s, *not* `rgid`. The
/// two are normally equal, but they are reached differently — `rgid` came from
/// the account named on the command line, this one from whichever passwd line
/// holds that uid first — so a database with two lines sharing a uid can make
/// them differ. Upstream sets `ok = false` without a message when that lookup
/// fails, and so does this.
pub fn print_group_list(
    out: &mut Output,
    username: Option<&[u8]>,
    ids: Ids,
    use_name: bool,
    delim: u8,
    db: &Db,
    process_groups: &[u32],
) {
    let login_group = match username {
        Some(_) => match db.user_by_uid(ids.ruid) {
            Some(user) => user.gid,
            None => {
                out.ok = false;
                ids.egid
            }
        },
        None => ids.egid,
    };

    print_group(out, ids.rgid, use_name, db);
    if ids.egid != ids.rgid {
        out.out.push(delim);
        print_group(out, ids.egid, use_name, db);
    }
    for gid in groups_for(username, login_group, db, process_groups) {
        if gid != ids.rgid && gid != ids.egid {
            out.out.push(delim);
            print_group(out, gid, use_name, db);
        }
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn getuid() -> u32;
    fn geteuid() -> u32;
    fn getgid() -> u32;
    fn getegid() -> u32;
    /// `getgroups(0, NULL)` counts; a second call fills. Returns -1 on
    /// failure, which for us means "no supplementary groups" rather than
    /// an error worth reporting — see the module docs.
    fn getgroups(size: i32, list: *mut u32) -> i32;
}

/// All four ids, unconditionally.
///
/// Upstream fetches only the ones the chosen report will read, so that a
/// Hurd-style failure of an *unused* getter cannot abort the run. The
/// unfetched ones keep their static-storage zero, and none of the four
/// reports ever reads one: `-u` reads only `ruid`/`euid`, `-g` only the
/// gids, `-G` reads `ruid` solely to look up a username it does not have
/// in this branch, and the default line reads all four. So fetching all
/// four is observationally identical and does not need the conditional
/// ladder.
#[cfg(unix)]
#[must_use]
pub fn current_ids() -> Ids {
    // SAFETY: four POSIX getters, no arguments, no pointers, and POSIX
    // requires that they cannot fail.
    unsafe {
        Ids {
            ruid: getuid(),
            euid: geteuid(),
            rgid: getgid(),
            egid: getegid(),
        }
    }
}

/// The current process's supplementary groups, or none if the kernel has
/// no answer. Never an error: gnulib treats a failed count as an empty
/// list unless it can grow one from `gid`, which the caller supplies.
#[cfg(unix)]
#[must_use]
pub fn process_groups() -> Vec<u32> {
    // SAFETY: the counting form; POSIX requires a null list pointer be
    // ignored when the size is 0.
    let counted = unsafe { getgroups(0, std::ptr::null_mut()) };
    let Ok(count) = usize::try_from(counted) else {
        return Vec::new();
    };
    if count == 0 {
        return Vec::new();
    }
    let Ok(size) = i32::try_from(count) else {
        return Vec::new();
    };
    let mut buffer = vec![0_u32; count];
    // SAFETY: `buffer` has room for `size` gids, which is what is claimed.
    let filled = unsafe { getgroups(size, buffer.as_mut_ptr()) };
    match usize::try_from(filled) {
        // A shrinking race is possible — a group could be dropped between
        // the two calls — so trust the second answer, not the first.
        Ok(filled) if filled <= count => {
            buffer.truncate(filled);
            buffer
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::from_bytes(
            b"root:x:0:0:root:/root:/bin/sh\n\
              alice:x:1000:1000:Alice:/home/alice:/bin/sh\n\
              bob:x:1001:5000:Bob:/home/bob:/bin/sh\n\
              1000:x:4000:4000:confusing:/:/bin/sh\n\
              twin:x:1001:2000:Shares bob's uid:/:/bin/sh\n",
            b"root:x:0:\n\
              alice:x:1000:\n\
              staff:x:2000:alice,bob\n\
              wheel:x:3000:alice\n",
        )
    }

    #[test]
    fn a_username_uses_the_database_and_leads_with_the_login_group() {
        let db = db();
        assert_eq!(
            groups_for(Some(b"alice"), 1000, &db, &[]),
            vec![1000, 2000, 3000]
        );
        // The login group is suppressed from the member-list half even when it
        // is reached under a different name, so no duplicate appears.
        assert_eq!(groups_for(Some(b"alice"), 2000, &db, &[]), vec![2000, 3000]);
    }

    #[test]
    fn no_username_uses_the_process_list_with_the_gid_in_front() {
        let db = db();
        assert_eq!(groups_for(None, 1000, &db, &[]), vec![1000]);
        assert_eq!(
            groups_for(None, 1000, &db, &[2000, 3000]),
            vec![1000, 2000, 3000]
        );
        // gnulib's weak dedup: equal to the first element, or to the previous
        // kept one.
        assert_eq!(
            groups_for(None, 1000, &db, &[1000, 2000, 2000, 1000]),
            vec![1000, 2000]
        );
    }

    #[test]
    fn the_duplicate_pass_is_gnulibs_weak_one_not_a_real_dedup() {
        // 5 reappears after 7 and survives, because it is neither the first
        // element nor adjacent to its earlier self. Upstream documents this.
        assert_eq!(reduce_duplicates(&[9, 5, 7, 5]), vec![9, 5, 7, 5]);
        assert_eq!(reduce_duplicates(&[]), Vec::<u32>::new());
        assert_eq!(reduce_duplicates(&[7]), vec![7]);
    }
}
