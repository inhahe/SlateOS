//! Capability rights — what operations a capability permits.
//!
//! Rights are a bitfield.  When delegating a capability, you can
//! only grant a subset of rights you hold (AND-mask, never add bits).
//!
//! The bits are grouped by subsystem for clarity, but any capability
//! can carry any combination of rights.

/// A set of rights that a capability grants.
///
/// Rights are a 64-bit bitfield.  Common operations (read, write,
/// etc.) occupy the low bits; subsystem-specific rights use higher
/// bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rights(u64);

impl Rights {
    // --- Common rights (bits 0–15) ---

    /// Read data from the resource.
    pub const READ: Self = Self(1 << 0);

    /// Write data to the resource.
    pub const WRITE: Self = Self(1 << 1);

    /// Execute/invoke the resource (e.g., run a program).
    pub const EXECUTE: Self = Self(1 << 2);

    /// Create child objects within the resource.
    pub const CREATE: Self = Self(1 << 3);

    /// Delete the resource or child objects within it.
    pub const DELETE: Self = Self(1 << 4);

    /// **Access** a resource's metadata — its size, times, mode, link target,
    /// extended attributes, and the filesystem statistics behind it.
    ///
    /// This bit is what `stat`, `lstat`, `readlink`, `getxattr`, `listxattr`,
    /// `statvfs` and `flock` are gated on. It is deliberately *not* implied by
    /// [`READ`](Self::READ): "may learn this file's size" and "may read this
    /// file's bytes" are different authorities, and an indexer or a `du` should
    /// be able to hold the first without the second.
    ///
    /// **It does not gate metadata *modification*.** Changing metadata —
    /// `setxattr`, `removexattr`, and in future `chmod`/`utimes` — requires
    /// [`WRITE`](Self::WRITE), on the principle that mutating a file's
    /// attributes is a write to the file's inode. This doc used to read
    /// "Modify metadata (permissions, attributes, etc.)", which described
    /// neither the gates that exist nor the ones that check this bit; the
    /// mismatch cost a cross-lane investigation, because a reader who had
    /// correctly noted that stat-by-path is gated on a *modify* right
    /// reasonably concluded that could not be the cause of an `EACCES` on a
    /// read. If a future change does want a distinct "may alter attributes"
    /// authority, give it its own bit rather than overloading this one — the
    /// same argument [`SET_CREDENTIALS`](Self::SET_CREDENTIALS) makes at
    /// length, and there are 52 free bits.
    pub const METADATA: Self = Self(1 << 5);

    /// Transfer (delegate) this capability to another task.
    pub const TRANSFER: Self = Self(1 << 6);

    /// Duplicate this capability (create another handle to the same
    /// resource, possibly with fewer rights).
    pub const DUPLICATE: Self = Self(1 << 7);

    /// Wait on this resource (register with a completion port).
    pub const WAIT: Self = Self(1 << 8);

    /// Signal this resource (e.g., write to an eventfd).
    pub const SIGNAL: Self = Self(1 << 9);

    // --- Subsystem-specific rights (bits 16–31) ---

    /// Permission to use Realtime I/O priority class.
    ///
    /// Required on an `IoScheduler` capability to submit I/O requests
    /// at the Realtime priority class.  Without this right, Realtime
    /// requests from userspace are downgraded to BestEffort.
    pub const IO_REALTIME: Self = Self(1 << 16);

    /// Debug / unilateral-introspection authority over a process.
    ///
    /// Required on a [`Process`](crate::cap::ResourceType::Process)
    /// capability to read or write the target process's memory
    /// **across address spaces** via `process_vm_readv` /
    /// `process_vm_writev` (and, in future, to `ptrace`-attach).  This
    /// is *unilateral* introspection — the target does not consent — so
    /// it must be granted explicitly (parent→child, or by a privileged
    /// debugger broker), never derived from ambient PID/uid authority.
    /// Consensual memory sharing is a separate path (channel +
    /// shared-memory IPC) that never touches this right.
    /// See design-decisions.md §24 (open-questions Q6).
    pub const DEBUG: Self = Self(1 << 17);

    /// Authority to change a process's own uid/gid credentials.
    ///
    /// Required on a [`Process`](crate::cap::ResourceType::Process)
    /// capability to be projected `CAP_SETUID`/`CAP_SETGID` in the
    /// userspace POSIX capability view (`posix::sys_capability::
    /// kernel_view::project`), which is what gates `setuid()`/`setgid()`
    /// before they issue `SYS_PROCESS_SET_CREDENTIALS`.
    ///
    /// **Why this is its own bit and not [`METADATA`](Self::METADATA).**
    /// "May become another user" is the single most escalation-prone
    /// authority in the POSIX model, and `METADATA` is a *generic* bit
    /// documented as "permissions, attributes, etc." — the bit the next
    /// person wanting "may rename this process" or "may set this
    /// process's nice value" will reach for. Because the grant sites are
    /// in the kernel and the projection is in `posix`, such a grant would
    /// silently confer root-capability with nothing in either file to say
    /// so. A distinct bit makes that aliasing impossible; `Rights` is a
    /// `u64` with 52 bits still free, so it costs nothing to be precise.
    /// This mirrors why [`DEBUG`](Self::DEBUG) is not `READ | WRITE`.
    /// See design-decisions.md §207.
    pub const SET_CREDENTIALS: Self = Self(1 << 18);

    /// Authority to lock memory beyond the per-process quota.
    ///
    /// Required on a [`ResourceLimit`](crate::cap::ResourceType::ResourceLimit)
    /// capability to be projected `CAP_IPC_LOCK`.  Locking *within* the quota
    /// needs nothing.
    ///
    /// **Why this is its own bit and not [`WRITE`](Self::WRITE).** The rest of
    /// `ResourceLimit`'s authority — raising a *hard* rlimit — really is a
    /// write to the process's limit table, and `WRITE` says so. `mlock`ing
    /// past the quota is not a write to anything; it is a claim on physical
    /// memory that the quota exists to bound. Folding it into `WRITE` would
    /// mean that granting "may raise its own file-descriptor limit" silently
    /// also granted "may pin unbounded physical memory", which is a denial-of-
    /// service primitive and a different privilege on Linux
    /// (`CAP_IPC_LOCK` vs `CAP_SYS_RESOURCE`) that ported software drops
    /// separately. Same argument as [`SET_CREDENTIALS`](Self::SET_CREDENTIALS)
    /// and [`DEBUG`](Self::DEBUG): `Rights` is a `u64` with bits to spare, and
    /// a bit that means two things is a bit that gets granted for one of them.
    /// See design-decisions.md §269 (*the capability types* — not the hrtimer
    /// §269; this bit) and §350 (lane B's projection of it onto
    /// `CAP_IPC_LOCK`).
    pub const MEMORY_LOCK: Self = Self(1 << 19);

    /// May change the system's host name or domain name.
    ///
    /// Required by `SYS_HOSTNAME_SET` and `SYS_DOMAINNAME_SET`, which are the
    /// kernel primitives behind POSIX `sethostname`/`setdomainname`.
    ///
    /// Its own bit, for the reason [`SET_CREDENTIALS`](Self::SET_CREDENTIALS),
    /// [`DEBUG`](Self::DEBUG) and [`MEMORY_LOCK`](Self::MEMORY_LOCK) have
    /// theirs: a bit that means two things is a bit that gets granted for one
    /// of them. The nearest existing candidate was `WRITE` on the process, and
    /// folding it in would mean that granting "may write its own limit table"
    /// silently also granted "may rename the machine every other process on it
    /// reports". Linux separates these too -- `CAP_SYS_ADMIN` rather than any
    /// file or process right -- and ported software drops them separately.
    ///
    /// **It is deliberately narrower than `CAP_SYS_ADMIN`.** Linux's
    /// `CAP_SYS_ADMIN` is the catch-all that grants several dozen unrelated
    /// privileges, which is a design its own documentation calls a mistake. A
    /// bit here means one thing, so lane B's projection of `CAP_SYS_ADMIN` onto
    /// it is one-way: holding this does not imply anything else Linux bundles
    /// into that capability.
    ///
    /// **Why a right and not a uid check.** The Linux-ABI handler for
    /// `sethostname` gates on `uid == 0` read from the caller's credentials,
    /// which is ambient authority -- permission you get by *being* someone
    /// rather than by holding a token. CLAUDE.md's architectural rules forbid
    /// that, and `known-issues.md` ->
    /// `A-SET-CREDENTIALS-IS-GATED-ONLY-IN-USERSPACE` is what it costs: the
    /// kernel primitive behind `setuid` once had no check at all because the
    /// policy was held in a userspace wrapper, so any ring-3 process could
    /// become uid 0 by issuing the syscall directly. A wrapper is a
    /// convenience, not a gate.
    ///
    /// A caller without this right gets `PermissionDenied`, which is
    /// permanent and distinguishable from `NoSuchSyscall` -- the distinction
    /// lane B asked for when they requested the pair, so that an unprivileged
    /// caller learns it is unprivileged rather than that the call is missing.
    pub const SET_HOSTNAME: Self = Self(1 << 20);

    /// Every distinct right, in declaration order.
    ///
    /// Exists so that [`the aliasing assertion below`](self) can be stated
    /// once over the whole set rather than pairwise by hand. Convenience
    /// *combinations* (`ALL`, `READ_ONLY`, …) are deliberately absent — they
    /// are unions of these and would defeat the check.
    const DISTINCT: [Self; 15] = [
        Self::READ,
        Self::WRITE,
        Self::EXECUTE,
        Self::CREATE,
        Self::DELETE,
        Self::METADATA,
        Self::TRANSFER,
        Self::DUPLICATE,
        Self::WAIT,
        Self::SIGNAL,
        Self::IO_REALTIME,
        Self::DEBUG,
        Self::SET_CREDENTIALS,
        Self::MEMORY_LOCK,
        Self::SET_HOSTNAME,
    ];

    // --- Convenience combinations ---

    /// All rights.
    ///
    /// **Every bit, including ones that do not exist yet**, and that is why it
    /// must not be used at a *grant* site. See [`INIT_PROCESS`](Self::INIT_PROCESS).
    pub const ALL: Self = Self(u64::MAX);

    /// What the init process is granted on [`ResourceType::Process`].
    ///
    /// **Enumerated, not `ALL`, and the distinction is the whole point.** A
    /// wildcard grant cannot tell "every right that exists" from "every right
    /// that will ever exist", so with `ALL` here the decision about who holds a
    /// new privilege is taken by whoever declares the constant — silently, and
    /// usually without noticing.
    ///
    /// That is not hypothetical. On 2026-09-10 lane A added
    /// [`SET_HOSTNAME`](Self::SET_HOSTNAME) to gate renaming the machine,
    /// recorded in `design-decisions.md` §927 that nothing held it yet, and
    /// process 1 held it the instant the bit existed — because `ALL` is
    /// `u64::MAX` and `pcb::has_capability_type` consults no resource id, so a
    /// class-wide grant satisfies any query. `fork` then clones the table, so the
    /// holder set was init plus every descendant nothing had narrowed. The entry
    /// claimed a privileged write was unreachable while PID 1 could perform it.
    /// See `known-issues.md` →
    /// `TD-A-A-NEW-RIGHT-IS-GRANTED-BEFORE-ANYONE-DECIDES-WHO-HOLDS-IT`.
    ///
    /// **Adding a right to this list is a deliberate line of code.** That is the
    /// only property being bought here: the next `SET_HOSTNAME` does not reach
    /// init until somebody writes it down. It buys nothing else — init still
    /// holds everything listed, so this changes no behaviour today.
    ///
    /// Note what it does *not* do. It is not a narrowing of init's authority and
    /// should not be read as one; the grant is still class-wide
    /// (`resource_id == 0`) because a token nobody holds is indistinguishable
    /// from leaving the operation denied, which is the reasoning recorded at the
    /// grant site itself. Narrowing *that* is a separate and much larger change.
    pub const INIT_PROCESS: Self = Self(
        Self::READ.0
            | Self::WRITE.0
            | Self::EXECUTE.0
            | Self::CREATE.0
            | Self::DELETE.0
            | Self::METADATA.0
            | Self::TRANSFER.0
            | Self::DUPLICATE.0
            | Self::WAIT.0
            | Self::SIGNAL.0
            | Self::IO_REALTIME.0
            | Self::DEBUG.0
            | Self::SET_CREDENTIALS.0
            | Self::MEMORY_LOCK.0
            | Self::SET_HOSTNAME.0,
    );

    /// No rights.
    #[allow(dead_code)] // public API; convenience constant for capability creation
    pub const NONE: Self = Self(0);

    /// Typical read-only: read + wait + duplicate.
    pub const READ_ONLY: Self = Self(Self::READ.0 | Self::WAIT.0 | Self::DUPLICATE.0);

    /// Typical read-write: read + write + wait + duplicate.
    pub const READ_WRITE: Self =
        Self(Self::READ.0 | Self::WRITE.0 | Self::WAIT.0 | Self::DUPLICATE.0);

    // --- Constructors ---

    /// Create an empty rights set.
    #[must_use]
    #[allow(dead_code)] // public API; constructor for capability creation
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Create from a raw bitfield value.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw bitfield value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    // --- Operations ---

    /// Check if this rights set contains a specific right.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Union of two rights sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Intersection of two rights sets (the subset of rights
    /// common to both).
    #[must_use]
    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Remove specific rights from this set.
    #[must_use]
    #[allow(dead_code)] // public API; counterpart to union/intersect for capability narrowing
    pub const fn remove(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Check if this rights set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Check if `self` is a subset of `other` (delegation check).
    ///
    /// A task can only delegate rights it already holds.
    #[must_use]
    pub const fn is_subset_of(self, other: Self) -> bool {
        (self.0 & !other.0) == 0
    }
}

/// Two rights must never name the same bit — checked at **compile time**.
///
/// This is the guard for the failure mode `design-decisions.md` §207 is about.
/// A right is a name for an *authority*, and two authorities sharing a bit
/// means granting one silently grants the other. That is not a bug that shows
/// up in a test run: the grant site and the check site are usually in
/// different crates (often different lanes), so no single diff contains both
/// halves, and the system behaves perfectly right up until someone is handed
/// an authority nobody meant to give them.
///
/// A `const` block rather than a `self_test()` on purpose — the boot-time
/// self-tests are the convention here, but a self-test can be skipped, run
/// late, or (as `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` found) silently not
/// run at all. An aliasing mistake is decidable from the constants alone, so
/// it should be impossible to *build*, not merely detected on boot.
const _: () = {
    let mut i = 0;
    while i < Rights::DISTINCT.len() {
        // Every right is exactly one bit.  A right that is a union has no
        // single meaning to grant or revoke, so it does not belong here.
        assert!(
            Rights::DISTINCT[i].0.count_ones() == 1,
            "each entry of Rights::DISTINCT must be a single bit"
        );
        let mut j = i + 1;
        while j < Rights::DISTINCT.len() {
            assert!(
                Rights::DISTINCT[i].0 != Rights::DISTINCT[j].0,
                "two Rights share a bit — granting one would silently grant the other"
            );
            j += 1;
        }
        i += 1;
    }
};

/// Adding a right must force a decision about whether init gets it.
///
/// [`Rights::INIT_PROCESS`] exists so that a new right does not reach the root
/// process by accident, and an enumeration on its own does not achieve that: a
/// new bit added to [`Rights::DISTINCT`] and forgotten here is simply *not*
/// granted, silently, which is the opposite failure and just as quiet.
///
/// So the count is pinned. Adding a right breaks this build, and clearing it
/// takes reading the two lines below and deciding — which is the whole
/// mechanism. `design-decisions.md` §928.
const _: () = {
    assert!(
        Rights::DISTINCT.len() == 15,
        "a right was added or removed. Decide whether the init process should \
         hold it: add it to Rights::INIT_PROCESS if so, leave it out if not, \
         and then bump this count. Do not bump the count alone — that is the \
         decision this assertion exists to make someone take."
    );

    // And no bit in the init grant may be one that is not a declared right.
    // `INIT_PROCESS` is written by hand, so a typo could set a bit that means
    // nothing today and something unintended the day it is declared.
    let mut declared: u64 = 0;
    let mut i = 0;
    while i < Rights::DISTINCT.len() {
        declared |= Rights::DISTINCT[i].0;
        i += 1;
    }
    assert!(
        Rights::INIT_PROCESS.0 & !declared == 0,
        "Rights::INIT_PROCESS sets a bit that is not a declared right"
    );
};

impl core::ops::BitOr for Rights {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl core::ops::BitAnd for Rights {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.intersect(rhs)
    }
}

impl core::fmt::Display for Rights {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut first = true;
        let flags = [
            (Self::READ, "r"),
            (Self::WRITE, "w"),
            (Self::EXECUTE, "x"),
            (Self::CREATE, "c"),
            (Self::DELETE, "d"),
            (Self::METADATA, "m"),
            (Self::TRANSFER, "t"),
            (Self::DUPLICATE, "dup"),
            (Self::WAIT, "wait"),
            (Self::SIGNAL, "sig"),
            (Self::IO_REALTIME, "io_rt"),
            (Self::DEBUG, "debug"),
            (Self::SET_CREDENTIALS, "setcred"),
            (Self::MEMORY_LOCK, "mlock"),
            (Self::SET_HOSTNAME, "sethost"),
        ];

        for (flag, name) in &flags {
            if self.contains(*flag) {
                if !first {
                    write!(f, "+")?;
                }
                write!(f, "{name}")?;
                first = false;
            }
        }

        if first {
            write!(f, "none")?;
        }

        Ok(())
    }
}
