//! The setuid family: how a process changes its user and group ids -- real,
//! effective, saved and filesystem (`pcb::ProcessCredentials`) -- by Linux's
//! rules (`kernel/sys.c`), for both ABIs: the Linux `setuid`, `setreuid`,
//! `setresuid`, `setfsuid` and their group twins, and the native
//! `SYS_PROCESS_SET_IDS`.
//!
//! ## Who may
//!
//! A **privileged** caller may set any id. Linux's test is `CAP_SETUID` (or
//! `CAP_SETGID`), which a process holds while its effective user id is 0;
//! here it is the same effective id 0 for a Linux program, or the
//! `SET_CREDENTIALS` right over processes -- root's, or one a parent granted
//! -- for either ABI ([`Authority`]). An unprivileged caller may only move
//! each id to one of the values its ids already hold:
//!
//! | call | unprivileged, each id it sets may be | it sets |
//! |---|---|---|
//! | `setuid(u)` | the real or saved id | the effective and fs ids (privileged: all four) |
//! | `setreuid(r, e)` | `r`: the real or effective id; `e`: any of the three | those given, then the saved id to the new effective one when `r` was given or `e` is not the old real id |
//! | `setresuid(r, e, s)` | any of the three | those given |
//! | `setfsuid(f)` | any of the four | the fs id; it answers the old one, never an error |
//!
//! Every change but `setfsuid` sets the filesystem id to the new effective
//! one. `(uid_t)-1` leaves an id as it is.
//!
//! ## Authority follows
//!
//! `pcb::update_credentials` decides and changes under one lock, and root's
//! authority follows the user ids there: put aside while the effective user
//! id is not 0 but another id is, back when it returns to 0, and gone for
//! good when every id leaves 0 -- Linux's `cap_emulate_setxuid`. So root may
//! `seteuid(1000)` to act as a user and `seteuid(0)` to come back, but not
//! after `setuid(1000)`. File access follows the filesystem ids
//! (`fs::vfs`), so a filesystem id that leaves 0 leaves root's way past
//! permission bits with it, as Linux's `cap_drop_fs_set` does.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::{self, ProcessCredentials, ProcessId};

/// Which of a process's ids a call changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// The user ids (`setuid` and its family).
    User,
    /// The group ids (`setgid` and its family).
    Group,
}

/// Whose rule of privilege applies (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authority {
    /// A Linux program's: an effective user id of 0, or the
    /// `SET_CREDENTIALS` right.
    Linux,
    /// A native program's: the `SET_CREDENTIALS` right alone -- no authority
    /// comes from an id.
    Native,
}

/// `(uid_t)-1`: leave the id as it is.
pub const KEEP: u32 = u32::MAX;

/// The four ids of `which` in `c`: real, effective, saved, filesystem.
#[must_use]
pub const fn ids(c: &ProcessCredentials, which: Which) -> [u32; 4] {
    match which {
        Which::User => [c.ruid, c.uid, c.suid, c.fsuid],
        Which::Group => [c.rgid, c.gid, c.sgid, c.fsgid],
    }
}

/// Write the four ids of `which` into `c`.
fn set_ids(c: &mut ProcessCredentials, which: Which, [r, e, s, f]: [u32; 4]) {
    match which {
        Which::User => {
            c.ruid = r;
            c.uid = e;
            c.suid = s;
            c.fsuid = f;
        }
        Which::Group => {
            c.rgid = r;
            c.gid = e;
            c.sgid = s;
            c.fsgid = f;
        }
    }
}

/// Apply `change` to `pid`'s ids of `which`, deciding and changing under one
/// lock (`pcb::update_credentials`): it is given the four ids and whether the
/// caller is privileged, and answers the four new ids or a refusal. A no-op
/// for a process that is gone -- Linux's setuid family answers 0 then, a
/// process in its teardown having nobody to tell.
fn apply(
    pid: ProcessId,
    which: Which,
    authority: Authority,
    change: impl FnOnce([u32; 4], bool) -> KernelResult<[u32; 4]>,
) -> KernelResult<()> {
    let done = pcb::update_credentials(pid, |c, may_set| {
        let privileged = (authority == Authority::Linux && c.uid == 0) || may_set;
        let new = change(ids(c, which), privileged)?;
        let mut creds = c.clone();
        set_ids(&mut creds, which, new);
        Ok(creds)
    });
    match done {
        Ok(_) | Err(KernelError::NoSuchProcess) => Ok(()),
        Err(e) => Err(e),
    }
}

/// `setuid(id)` or `setgid(id)`: privileged, all four ids; otherwise the
/// effective and filesystem ids, to the real or saved one only (`EPERM`).
///
/// # Errors
///
/// `NotPermitted` as above; `InvalidArgument` for `(uid_t)-1`.
pub fn set_id(pid: ProcessId, which: Which, authority: Authority, id: u32) -> KernelResult<()> {
    if id == KEEP {
        return Err(KernelError::InvalidArgument);
    }
    apply(pid, which, authority, |[r, _, s, _], privileged| {
        if privileged {
            Ok([id, id, id, id])
        } else if id == r || id == s {
            Ok([r, id, s, id])
        } else {
            Err(KernelError::NotPermitted)
        }
    })
}

/// `setreuid(r, e)` or `setregid(r, e)`, [`KEEP`] leaving one as it is. See
/// the module doc for the rule.
///
/// # Errors
///
/// `NotPermitted` for an id an unprivileged caller may not take.
pub fn set_re_id(
    pid: ProcessId,
    which: Which,
    authority: Authority,
    real: u32,
    effective: u32,
) -> KernelResult<()> {
    apply(pid, which, authority, |[r, e, s, _], privileged| {
        if !privileged
            && ((real != KEEP && real != r && real != e)
                || (effective != KEEP && effective != r && effective != e && effective != s))
        {
            return Err(KernelError::NotPermitted);
        }
        let new_r = if real == KEEP { r } else { real };
        let new_e = if effective == KEEP { e } else { effective };
        // The saved id follows the new effective one when the real id was
        // given, or the effective id became other than the old real one.
        let new_s = if real != KEEP || (effective != KEEP && effective != r) {
            new_e
        } else {
            s
        };
        Ok([new_r, new_e, new_s, new_e])
    })
}

/// `setresuid(r, e, s)` or `setresgid(r, e, s)`, [`KEEP`] leaving one as it
/// is: unprivileged, each id given must be one of the three it has.
///
/// # Errors
///
/// `NotPermitted` as above.
pub fn set_res_id(
    pid: ProcessId,
    which: Which,
    authority: Authority,
    real: u32,
    effective: u32,
    saved: u32,
) -> KernelResult<()> {
    apply(pid, which, authority, |[r, e, s, f], privileged| {
        // Linux's fast path: nothing would change -- the effective id given
        // equal to the effective and filesystem ids both.
        if (real == KEEP || real == r)
            && (effective == KEEP || (effective == e && effective == f))
            && (saved == KEEP || saved == s)
        {
            return Ok([r, e, s, f]);
        }
        let held = |id: u32| id == KEEP || id == r || id == e || id == s;
        let allowed = privileged || (held(real) && held(effective) && held(saved));
        if !allowed {
            return Err(KernelError::NotPermitted);
        }
        let pick = |given: u32, old: u32| if given == KEEP { old } else { given };
        let new_e = pick(effective, e);
        Ok([pick(real, r), new_e, pick(saved, s), new_e])
    })
}

/// `setfsuid(id)` or `setfsgid(id)`: the filesystem id becomes `id` if the
/// caller is privileged or `id` is one of its four ids; the answer is the
/// old filesystem id either way -- the call never fails, as Linux's. 0 for a
/// process that is gone.
#[must_use]
pub fn set_fs_id(pid: ProcessId, which: Which, authority: Authority, id: u32) -> u32 {
    let mut old = 0;
    // Only the process being gone can fail this, which leaves `old` 0.
    let _ = pcb::update_credentials(pid, |c, may_set| {
        let [r, e, s, f] = ids(c, which);
        old = f;
        let privileged = (authority == Authority::Linux && c.uid == 0) || may_set;
        let mut creds = c.clone();
        if id != KEEP && id != f && (id == r || id == e || id == s || privileged) {
            set_ids(&mut creds, which, [r, e, s, id]);
        }
        Ok(creds)
    });
    old
}

/// The in-kernel half of the set-id tests: the rule table on a synthetic
/// process, and root's authority put aside, put back and taken for good as
/// the user ids move. The calls from ring 3 are
/// `spawn::self_test_linux_setid`'s.
///
/// # Errors
///
/// `InternalError` on the first check that fails.
pub fn self_test() -> KernelResult<()> {
    use crate::cap::{ResourceType, Rights};
    use crate::serial_println;
    let fail = |what: &str, pid: ProcessId| {
        serial_println!("[setid]   FAIL: {}", what);
        pcb::destroy(pid);
        Err(KernelError::InternalError)
    };
    serial_println!("[setid] Running self-test...");
    let pid = pcb::create("setid-self-test", 0);
    let uids = |pid: ProcessId| pcb::get_credentials(pid).map(|c| ids(&c, Which::User));
    let clock =
        |pid: ProcessId| pcb::has_capability_type(pid, ResourceType::SystemClock, Rights::WRITE);
    if pcb::grant_capability(pid, ResourceType::SystemClock, 0, Rights::WRITE).is_err() {
        return fail("could not give the test process root's clock right", pid);
    }
    let l = Authority::Linux;

    // A temporary drop: the clock right is put aside, and comes back.
    if set_res_id(pid, Which::User, l, KEEP, 1000, KEEP).is_err()
        || uids(pid) != Some([0, 1000, 0, 1000])
    {
        return fail("seteuid(1000) as root did not leave 0, 1000, 0", pid);
    }
    if clock(pid) || pcb::suspended_rights(pid) != 1 {
        return fail(
            "root's clock right was not put aside while the effective id was 1000",
            pid,
        );
    }
    if set_res_id(pid, Which::User, l, KEEP, 2000, KEEP) != Err(KernelError::NotPermitted) {
        return fail("an effective id the process has none of was allowed", pid);
    }
    if set_res_id(pid, Which::User, l, KEEP, 0, KEEP).is_err()
        || uids(pid) != Some([0, 0, 0, 0])
        || !clock(pid)
        || pcb::suspended_rights(pid) != 0
    {
        return fail(
            "seteuid(0) did not bring root and its clock right back",
            pid,
        );
    }
    // setreuid: the saved id follows the effective one when the real id is
    // given.
    if set_re_id(pid, Which::User, l, 1000, KEEP).is_err() || uids(pid) != Some([1000, 0, 0, 0]) {
        return fail("setreuid(1000, -1) as root did not give 1000, 0, 0", pid);
    }
    // setfsuid: the old id answered; an id it holds none of refused, quietly.
    if set_fs_id(pid, Which::User, l, 1000) != 0
        || set_fs_id(pid, Which::User, l, 0) != 1000
        || uids(pid) != Some([1000, 0, 0, 0])
    {
        return fail("setfsuid did not answer the old id or move the fs id", pid);
    }
    // For good: every id leaves 0, and nothing brings root back.
    if set_id(pid, Which::User, l, 1000).is_err() || uids(pid) != Some([1000, 1000, 1000, 1000]) {
        return fail("setuid(1000) as root did not set all four", pid);
    }
    if clock(pid) || pcb::suspended_rights(pid) != 0 {
        return fail("root's authority outlived every id leaving 0", pid);
    }
    if set_id(pid, Which::User, l, 0) != Err(KernelError::NotPermitted)
        || set_res_id(pid, Which::User, l, KEEP, 0, KEEP) != Err(KernelError::NotPermitted)
        || set_fs_id(pid, Which::User, l, 0) != 1000
        || uids(pid) != Some([1000, 1000, 1000, 1000])
    {
        return fail("a process with no id 0 got one back", pid);
    }
    // An unprivileged setuid to its own real id is allowed (a no-op).
    if set_id(pid, Which::User, l, 1000).is_err() {
        return fail("setuid to its own id was refused", pid);
    }
    // A native caller's privilege is the right, not the id: a process whose
    // ids are 0 but that holds no SET_CREDENTIALS may not take an id it does
    // not hold.
    let native = pcb::create("setid-self-test-native", 0);
    if set_id(native, Which::User, Authority::Native, 1000) != Err(KernelError::NotPermitted) {
        pcb::destroy(native);
        return fail("a native setuid without SET_CREDENTIALS was allowed", pid);
    }
    pcb::destroy(native);
    pcb::destroy(pid);
    serial_println!(
        "[setid]   seteuid away and back with root's rights put aside, setreuid, setfsuid, \
         a drop for good, and a native caller's privilege: OK"
    );
    serial_println!("[setid] Self-test PASSED");
    Ok(())
}
