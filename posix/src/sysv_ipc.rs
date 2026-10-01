// Ids and permission bits here are shifted by constant amounts narrower than
// their types, and a slot index is below `MSGMNI`/`SEMMNI` (32000) before it
// is encoded.  Clippy cannot see either bound.
#![allow(clippy::arithmetic_side_effects)]
//! What the System V IPC objects -- message queues ([`crate::sysv_msg`]) and
//! semaphore sets ([`crate::sysv_sem`]) -- share, as Linux 6.6's ipc/util.c
//! is theirs: ids, and the permission record and its checks.  Their table of
//! slots and the counter their blocked calls sleep on are
//! [`crate::objtable`]'s, which kernel AIO uses too.
//!
//! - **Ids** carry a slot and that slot's reuse count, so an id outlives its
//!   object only as one that no longer resolves.
//! - **Permissions** are `ipcperms`: the owner's, group's or others' bits,
//!   chosen by the caller's effective ids, `CAP_IPC_OWNER` granting any;
//!   control (`IPC_SET`, `IPC_RMID`) is `ipcctl_obtain_check`'s owner or
//!   creator, or `CAP_SYS_ADMIN`.

use crate::errno;
use crate::linux_ipc::IpcPerm;

// ---------------------------------------------------------------------------
// Ids
// ---------------------------------------------------------------------------

/// The bits of a slot's reuse count an id carries.
pub(crate) const SEQ_MASK: u32 = 0x7FFF;

/// The id of the object in `slot` at reuse count `seq`: always positive, and
/// different for every reuse of the slot until the count wraps.
pub(crate) fn encode_id(slot: usize, seq: u32) -> i32 {
    let s = ((slot as u32) & 0xFFFF).wrapping_add(1);
    (((seq & SEQ_MASK) << 16) | s) as i32
}

/// The slot and reuse count an id names; `None` for one no object could
/// have.
pub(crate) fn decode_id(id: i32) -> Option<(usize, u32)> {
    if id <= 0 {
        return None;
    }
    let u = id as u32;
    let slot = ((u & 0xFFFF) as usize).checked_sub(1)?;
    Some((slot, (u >> 16) & SEQ_MASK))
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

/// `ipcperms`'s requests: read and write, in all three classes.
pub(crate) const S_IRUGO: u32 = 0o444;
pub(crate) const S_IWUGO: u32 = 0o222;

/// An object's `kern_ipc_perm`: its key, its slot's reuse count, its owner
/// and creator, and its mode.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Perm {
    pub(crate) key: i32,
    pub(crate) seq: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) cuid: u32,
    pub(crate) cgid: u32,
    /// The permission bits (`0o777` of them).
    pub(crate) mode: u32,
}

impl Perm {
    pub(crate) const EMPTY: Self = Self {
        key: 0,
        seq: 0,
        uid: 0,
        gid: 0,
        cuid: 0,
        cgid: 0,
        mode: 0,
    };

    /// A new object's: owned and created by `who`, with `flags`' permission
    /// bits (`msgflg & S_IRWXUGO`, as `newque` and `newary` take them).
    pub(crate) fn new(key: i32, seq: u32, flags: i32, who: Caller) -> Self {
        Self {
            key,
            seq,
            uid: who.euid,
            gid: who.egid,
            cuid: who.euid,
            cgid: who.egid,
            mode: (flags as u32) & 0o777,
        }
    }

    /// `ipc_update_perm`, for `IPC_SET`: the owner, the group and the
    /// permission bits -- `EINVAL` for -1, which is no user or group.
    pub(crate) fn update(&mut self, uid: u32, gid: u32, mode: u32) -> Result<(), i32> {
        if uid == u32::MAX || gid == u32::MAX {
            return Err(errno::EINVAL);
        }
        self.uid = uid;
        self.gid = gid;
        self.mode = mode & 0o777;
        Ok(())
    }

    /// `kernel_to_ipc64_perm`, as the C library's `struct ipc_perm`.
    pub(crate) fn to_ipc_perm(self) -> IpcPerm {
        IpcPerm {
            __ipc_perm_key: self.key,
            uid: self.uid,
            gid: self.gid,
            cuid: self.cuid,
            cgid: self.cgid,
            mode: self.mode,
            __ipc_perm_seq: (self.seq & SEQ_MASK) as i32,
            ..IpcPerm::default()
        }
    }
}

/// The caller's effective ids, as `ipcperms` reads them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Caller {
    pub(crate) euid: u32,
    pub(crate) egid: u32,
}

pub(crate) fn caller() -> Caller {
    Caller {
        euid: crate::unistd::geteuid(),
        egid: crate::unistd::getegid(),
    }
}

/// The mode bits of `p` that apply to `who`: its owner's when `who` is the
/// owner or the creator, else its group's when `who`'s group is either
/// group, else the rest's -- in the low three bits.
pub(crate) fn granted_bits(p: &Perm, who: Caller) -> u32 {
    if who.euid == p.cuid || who.euid == p.uid {
        (p.mode >> 6) & 0o7
    } else if who.egid == p.cgid || who.egid == p.gid {
        // `in_group_p`: the caller has no supplementary groups here
        // (`getgroups` reports none), so its group is its effective one.
        (p.mode >> 3) & 0o7
    } else {
        p.mode & 0o7
    }
}

/// Linux's `ipcperms`: may `who` have what `flag` asks for -- its bits in
/// any class, folded together, as `msgget`'s flags or `S_IRUGO` ask -- with
/// `CAP_IPC_OWNER` granting anything the mode does not?
pub(crate) fn permits(p: &Perm, who: Caller, flag: u32) -> bool {
    let requested = ((flag >> 6) | (flag >> 3) | flag) & 0o7;
    (requested & !granted_bits(p, who)) == 0
        || crate::sys_capability::has_capability(crate::sys_capability::CAP_IPC_OWNER)
}

/// `ipcctl_obtain_check`: `IPC_SET` and `IPC_RMID` are the owner's and the
/// creator's, or need `CAP_SYS_ADMIN`.
pub(crate) fn may_control(p: &Perm, who: Caller) -> bool {
    who.euid == p.cuid
        || who.euid == p.uid
        || crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_ADMIN)
}

/// Seconds since the epoch, for the objects' timestamps.
pub(crate) fn now_secs() -> i64 {
    crate::lowlevellock::now_on(crate::time::CLOCK_REALTIME).tv_sec
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for slot in [0, 1, 7, 31_999] {
            for seq in [0u32, 1, 0x7FFF] {
                let id = encode_id(slot, seq);
                assert!(id > 0);
                assert_eq!(decode_id(id), Some((slot, seq)));
            }
        }
        assert!(decode_id(0).is_none());
        assert!(decode_id(-1).is_none());
        assert!(decode_id(0x0001_0000).is_none(), "slot field 0");
    }

    #[test]
    fn perm_update_refuses_no_user_and_keeps_the_permission_bits() {
        let who = Caller { euid: 5, egid: 6 };
        let mut p = Perm::new(1, 0, 0o7640, who);
        assert_eq!((p.uid, p.cuid, p.gid, p.cgid, p.mode), (5, 5, 6, 6, 0o640));
        assert_eq!(p.update(u32::MAX, 1, 0), Err(errno::EINVAL));
        assert_eq!(p.update(1, u32::MAX, 0), Err(errno::EINVAL));
        assert_eq!(p.update(7, 8, 0o1755), Ok(()));
        assert_eq!((p.uid, p.gid, p.mode, p.cuid), (7, 8, 0o755, 5));
    }

    /// Restores this test thread's effective capabilities when dropped.
    struct CapGuard(u32, u32);

    fn set_effective(lo: u32, hi: u32) {
        let mut hdr = crate::sys_capability::CapUserHeader {
            version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        let data = [
            crate::sys_capability::CapUserData {
                effective: lo,
                permitted: u32::MAX,
                inheritable: 0,
            },
            crate::sys_capability::CapUserData {
                effective: hi,
                permitted: u32::MAX,
                inheritable: 0,
            },
        ];
        assert_eq!(crate::sys_capability::capset(&mut hdr, data.as_ptr()), 0);
    }

    impl Drop for CapGuard {
        fn drop(&mut self) {
            set_effective(self.0, self.1);
        }
    }

    fn without(cap: u32) -> CapGuard {
        let (lo, hi) = crate::sys_capability::current_caps_effective();
        if cap < 32 {
            set_effective(lo & !(1 << cap), hi);
        } else {
            set_effective(lo, hi & !(1 << (cap - 32)));
        }
        CapGuard(lo, hi)
    }

    #[test]
    fn granted_bits_follow_owner_then_group_then_other() {
        let mut p = Perm {
            mode: 0o751,
            uid: 10,
            cuid: 11,
            gid: 20,
            cgid: 21,
            ..Perm::EMPTY
        };
        let who = |euid, egid| Caller { euid, egid };
        assert_eq!(granted_bits(&p, who(10, 0)), 0o7, "the owner");
        assert_eq!(granted_bits(&p, who(11, 0)), 0o7, "the creator");
        assert_eq!(granted_bits(&p, who(1, 20)), 0o5, "the group");
        assert_eq!(granted_bits(&p, who(1, 21)), 0o5, "the creator's group");
        assert_eq!(granted_bits(&p, who(1, 2)), 0o1, "the rest");
        // The owner's bits apply to the owner even when the group's are wider.
        p.mode = 0o070;
        assert_eq!(granted_bits(&p, who(10, 20)), 0);
    }

    #[test]
    fn permits_folds_the_request_across_classes() {
        let _g = without(crate::sys_capability::CAP_IPC_OWNER);
        let p = Perm {
            mode: 0o640,
            uid: 10,
            cuid: 10,
            gid: 20,
            cgid: 20,
            ..Perm::EMPTY
        };
        let owner = Caller { euid: 10, egid: 0 };
        let group = Caller { euid: 1, egid: 20 };
        let other = Caller { euid: 1, egid: 2 };
        assert!(permits(&p, owner, 0o666), "rw asked; the owner has rw");
        assert!(!permits(&p, group, 0o666), "the group has only r");
        assert!(permits(&p, group, S_IRUGO));
        assert!(!permits(&p, other, S_IRUGO));
        assert!(permits(&p, other, 0), "nothing asked");
        assert!(permits(&p, owner, 0o3600), "flags above 0777 ask nothing");
    }

    #[test]
    fn cap_ipc_owner_grants_what_the_mode_does_not() {
        let p = Perm {
            mode: 0,
            uid: 10,
            cuid: 10,
            ..Perm::EMPTY
        };
        let other = Caller { euid: 1, egid: 1 };
        assert!(
            permits(&p, other, S_IWUGO),
            "a test thread holds every capability"
        );
    }

    #[test]
    fn may_control_is_owner_or_creator_or_cap_sys_admin() {
        let p = Perm {
            uid: 10,
            cuid: 11,
            ..Perm::EMPTY
        };
        let who = |euid| Caller { euid, egid: 0 };
        let caps = without(crate::sys_capability::CAP_SYS_ADMIN);
        assert!(may_control(&p, who(10)));
        assert!(may_control(&p, who(11)));
        assert!(!may_control(&p, who(12)));
        drop(caps);
        assert!(may_control(&p, who(12)));
    }

    #[test]
    fn to_ipc_perm_is_the_c_structure() {
        let p = Perm {
            key: 0x42,
            seq: 0x1_0003,
            uid: 1,
            gid: 2,
            cuid: 3,
            cgid: 4,
            mode: 0o640,
        };
        let c = p.to_ipc_perm();
        assert_eq!(
            (
                c.__ipc_perm_key,
                c.uid,
                c.gid,
                c.cuid,
                c.cgid,
                c.mode,
                c.__ipc_perm_seq
            ),
            (0x42, 1, 2, 3, 4, 0o640, 3),
            "the sequence as the id carries it"
        );
    }
}
